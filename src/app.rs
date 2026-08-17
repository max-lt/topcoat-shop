//! The shell every page wears, the shared building blocks, and the home
//! page. The design language: an oat ground rather than cold grey, one
//! gin-green accent, a serif for anything that speaks and a sans for anything
//! that informs.

use topcoat::context::Cx;
use topcoat::font;
use topcoat::router::error::{see_other, NotFoundError, SeeOther};
use topcoat::router::{content::Form, layout, page, query_params, route, uri, StatusCode};
use topcoat::tailwind;
use topcoat::view::{component, view};
use topcoat::Result;

use crate::db::{self, format_price, Product};
use crate::design::{SANS, SERIF};

pub mod account;
pub mod cart;
pub mod context;
pub mod house;
pub mod journal;
pub mod orders;
pub mod seo;
pub mod shop;

use context::{cart_count, current_user, pool};

// --- tokens

/// The one dark, solid call to action.
pub const BTN: &str = "inline-flex items-center justify-center gap-2 rounded-full \
    bg-gin-700 px-5 py-2.5 text-sm font-medium text-oat-50 transition hover:bg-gin-800";
/// Its quiet counterpart: a hairline ring, no fill.
pub const BTN_OUTLINE: &str = "inline-flex items-center justify-center gap-2 rounded-full \
    px-5 py-2.5 text-sm font-medium ring-1 ring-oat-300 transition hover:bg-oat-100";
pub const CARD: &str = "rounded-2xl bg-white ring-1 ring-oat-200";
pub const FIELD: &str = "w-full rounded-xl bg-white px-3.5 py-2.5 text-sm ring-1 \
    ring-oat-300 outline-none transition placeholder:text-oat-400 focus:ring-2 \
    focus:ring-gin-600";
pub const EYEBROW: &str = "text-xs font-medium uppercase tracking-[0.2em] text-gin-700";
pub const SOFT: &str = "text-oat-700";
pub const MUTED: &str = "text-oat-600";

#[component]
pub async fn page_heading(eyebrow: &str, title: &str, lede: &str) -> Result {
    view! {
        <p class=(EYEBROW)>(eyebrow)</p>
        <h1 class="mt-3 text-4xl leading-[1.05] sm:text-5xl">(title)</h1>
        if !lede.is_empty() {
            <p class=("mt-4 max-w-2xl text-lg leading-relaxed ".to_string() + SOFT)>(lede)</p>
        }
    }
}

/// The product tile, used on the home page, the catalog and the related
/// shelf, so a product looks the same everywhere it appears.
#[component]
pub async fn product_tile(p: Product) -> Result {
    let sold_out = p.sold_out();
    let is_new = p.is_new != 0;
    view! {
        <a href=("/produit/".to_string() + &p.sku) class="group block transition duration-300 hover:-translate-y-1">
            <div class="relative aspect-square overflow-hidden rounded-2xl bg-oat-100 ring-1 ring-oat-200 transition duration-300 group-hover:shadow-xl group-hover:shadow-oat-900/10 group-hover:ring-gin-300"
                 data-vt=(&p.sku)
                 data-bg=(crate::images::background(&p.sku))>
                // Resized in-process by /img: the tile never pays for the
                // 1600 px original.
                <img src=(crate::images::url(&p.sku, 400))
                     srcset=(format!("{} 400w, {} 900w", crate::images::url(&p.sku, 400), crate::images::url(&p.sku, 900)))
                     sizes="(min-width: 1024px) 25vw, 50vw"
                     alt=(&p.name)
                     loading="lazy"
                     class="h-full w-full object-cover transition duration-500 group-hover:scale-105">
                if is_new {
                    <span class="absolute left-3 top-3 rounded-full bg-gin-700 px-2.5 py-1 text-xs font-medium uppercase tracking-widest text-oat-50">"Nouveau"</span>
                }
                if sold_out {
                    <span class="absolute inset-x-0 bottom-0 bg-oat-900/85 py-2 text-center text-xs font-medium text-oat-50">"Épuisé"</span>
                }
            </div>
            <div class="mt-4 flex items-baseline justify-between gap-3">
                <h3 class="text-lg leading-snug">(&p.name)</h3>
                <span class="shrink-0 text-sm tabular-nums">(format_price(p.price_cents))</span>
            </div>
            <p class=("mt-1 text-sm ".to_string() + MUTED)>(&p.category)</p>
        </a>
    }
}

// --- shell

const NAV: [(&str, &str); 4] = [
    ("/boutique", "Boutique"),
    ("/journal", "Journal"),
    ("/maison", "La maison"),
    ("/aide", "Aide"),
];

/// The page's title, derived from the URL: pages render before the layout
/// wraps them and the request context is read-only, so the path is the one
/// channel that is always there.
async fn page_title(cx: &Cx) -> Result<String> {
    let path = uri(cx).path().to_string();
    Ok(match path.as_str() {
        "/" => "Bernard — la boutique de la coquille".to_string(),
        "/admin" => "Administration — Bernard".to_string(),
        "/admin/produits" => "Produits — Administration".to_string(),
        "/admin/commandes" => "Commandes — Administration".to_string(),
        "/admin/clients" => "Clients — Administration".to_string(),
        "/boutique" => match query_params::<shop::Filters>(cx)?.category.as_deref() {
            Some(cat) if !cat.is_empty() => format!("{cat} — Bernard"),
            _ => "La boutique — Bernard".to_string(),
        },
        "/recherche" => "Recherche — Bernard".to_string(),
        "/panier" => "Panier — Bernard".to_string(),
        "/commander" => "Commander — Bernard".to_string(),
        "/connexion" => "Connexion — Bernard".to_string(),
        "/compte" => "Mon compte — Bernard".to_string(),
        "/journal" => "Journal — Bernard".to_string(),
        "/maison" => "La maison — Bernard".to_string(),
        "/aide" => "Aide — Bernard".to_string(),
        "/contact" => "Contact — Bernard".to_string(),
        "/cgv" => "Conditions générales — Bernard".to_string(),
        "/mentions-legales" => "Mentions légales — Bernard".to_string(),
        other => {
            if let Some(sku) = other.strip_prefix("/produit/") {
                match db::product(pool(cx), sku).await? {
                    Some(p) => format!("{} — Bernard", p.name),
                    None => "Bernard".to_string(),
                }
            } else if let Some(slug) = other.strip_prefix("/journal/") {
                journal::POSTS
                    .iter()
                    .find(|(s, ..)| *s == slug)
                    .map(|(_, _, _, title, _)| format!("{title} — Bernard"))
                    .unwrap_or_else(|| "Journal — Bernard".to_string())
            } else if other.starts_with("/admin/") {
                "Administration — Bernard".to_string()
            } else if other.starts_with("/commande/") {
                "Suivi de commande — Bernard".to_string()
            } else {
                "Bernard — la boutique de la coquille".to_string()
            }
        }
    })
}

#[layout("/")]
async fn shell(cx: &Cx, slot: Result) -> Result {
    let signed_in = current_user(cx).await?.is_some();
    let items = cart_count(cx).await?;
    let title = page_title(cx).await?;

    // og:image rides the URL, like the title: a product page shows its
    // photo, an article its illustration, everything else the mascot.
    let origin = context::public_origin(cx);
    let path = uri(cx).path().to_string();
    let og_image = if let Some(sku) = path.strip_prefix("/produit/") {
        format!("{origin}/img/{sku}?w=1600")
    } else if let Some(slug) = path.strip_prefix("/journal/") {
        let key = journal::POSTS
            .iter()
            .find(|(s, ..)| *s == slug)
            .map(|(_, _, tag, ..)| journal::photo_key(tag))
            .unwrap_or("journal-crabe");
        format!("{origin}/img/{key}?w=1600")
    } else {
        format!("{origin}/img/journal-crabe?w=1600")
    };
    let og_url = format!("{origin}{path}");

    let content = match slot {
        Err(error) if error.downcast_ref::<NotFoundError>().is_some() => view! {
            (StatusCode::NOT_FOUND)
            <section class="mx-auto max-w-xl py-24 text-center">
                <p class=(EYEBROW)>"Erreur 404"</p>
                <h1 class="mt-4 text-5xl">"Coquille vide."</h1>
                <p class=("mt-5 text-lg leading-relaxed ".to_string() + SOFT)>
                    "Cette page n'existe pas, ou ne fait plus partie de la collection. \
                     Le reste de la boutique vous attend."
                </p>
                <a href="/boutique" class=(BTN.to_string() + " mt-8")>"Voir la boutique"</a>
            </section>
        },
        content => content,
    }?;

    view! {
        <!DOCTYPE html>
        <html lang="fr">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>(&title)</title>
                <meta name="description" content="Le vestiaire et la papeterie de Bernard : coton biologique, papeterie soignée, et un crabe qui s'y connaît en petites coquilles.">
                <link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Ctext y='0.9em' font-size='90'%3E🦀%3C/text%3E%3C/svg%3E">
                <meta name="theme-color" content="#fbf9f5">
                <meta property="og:site_name" content="Bernard">
                <meta property="og:type" content="website">
                <meta property="og:title" content=(&title)>
                <meta property="og:image" content=(&og_image)>
                <meta property="og:url" content=(&og_url)>
                <link rel="alternate" type="application/rss+xml" title="Le journal de Bernard" href="/journal/flux.xml">
                <link rel="sitemap" href="/sitemap.xml">
                <meta property="og:description" content="Le vestiaire et la papeterie de Bernard : coton biologique, papeterie soignée, séries courtes.">
                font::link(font: SERIF)
                font::link(font: SANS)
                <link rel="stylesheet" href=(tailwind::stylesheet!())>
                topcoat::runtime::script()
                topcoat::dev::script()
            </head>
            <body class="min-h-screen">
                <a href="#contenu" class="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-50 focus:rounded-full focus:bg-gin-800 focus:px-5 focus:py-2.5 focus:text-sm focus:text-oat-50">
                    "Aller au contenu"
                </a>
                <p class="bg-gin-800 px-6 py-2.5 text-center text-xs tracking-wide text-gin-100">
                    "Livraison offerte dès " (format_price(db::FREE_SHIPPING_CENTS)) " — retours acceptés 30 jours"
                </p>

                <header class="sticky top-0 z-40 border-b border-oat-200 bg-oat-50/90 backdrop-blur-sm">
                    <div class="mx-auto flex h-16 max-w-6xl items-center gap-6 px-6">
                        // Native <details>: the one dropdown that works without
                        // any runtime. Second tap on the burger closes it.
                        <details class="relative md:hidden">
                            <summary aria-label="Menu" class="flex h-10 w-10 cursor-pointer items-center justify-center rounded-full ring-1 ring-oat-300 transition hover:bg-oat-100">
                                <span class="flex flex-col gap-1.5">
                                    <span class="h-px w-5 bg-oat-900"></span>
                                    <span class="h-px w-5 bg-oat-900"></span>
                                    <span class="h-px w-5 bg-oat-900"></span>
                                </span>
                            </summary>
                            <div class="absolute left-0 top-full z-50 mt-3 w-72 rounded-2xl bg-white p-3 shadow-lg ring-1 ring-oat-200">
                                <form action="/recherche" method="get" class="relative px-2 pt-1">
                                    <input name="q" type="search" autocomplete="off" placeholder="Chercher"
                                           class="w-full rounded-full bg-oat-50 px-4 py-2 text-sm ring-1 ring-oat-200 outline-none transition placeholder:text-oat-500 focus:ring-2 focus:ring-gin-600">
                                </form>
                                <nav class="mt-2">
                                    for (href, label) in NAV {
                                        <a href=(href) class="block rounded-xl px-4 py-2.5 text-sm transition hover:bg-oat-100">(label)</a>
                                    }
                                    <a href="/compte" class="block rounded-xl px-4 py-2.5 text-sm transition hover:bg-oat-100">"Mon compte"</a>
                                </nav>
                            </div>
                        </details>

                        <a href="/" class="flex shrink-0 items-center gap-2">
                            <span class="text-xl">"🦀"</span>
                            <span class="font-display text-xl">"Bernard"</span>
                        </a>

                        <nav class="hidden items-center gap-7 text-sm md:flex">
                            for (href, label) in NAV {
                                <a href=(href) class="transition hover:text-gin-700">(label)</a>
                            }
                        </nav>

                        // Topcoat 0.5 hydrates the runtime inside pages only, never
                        // inside a layout: a signal declared here would never be
                        // wired up. So the header stays a real form -- Enter or the
                        // arrow opens /recherche, where results do update live.
                        <form action="/recherche" method="get" class="relative ml-auto hidden lg:block">
                            <input name="q" type="search" autocomplete="off" placeholder="Chercher"
                                   class="w-56 rounded-full bg-white py-2 pl-4 pr-10 text-sm ring-1 ring-oat-200 outline-none transition placeholder:text-oat-500 focus:ring-2 focus:ring-gin-600">
                            <button aria-label="Chercher"
                                    class="absolute inset-y-0 right-1 my-auto flex h-8 w-8 items-center justify-center rounded-full text-oat-600 transition hover:bg-oat-100 hover:text-gin-700">"→"</button>
                        </form>

                        <div class="ml-auto flex items-center gap-4 lg:ml-0">
                            <a href="/compte" class="text-sm transition hover:text-gin-700">
                                (if signed_in { "Mon compte" } else { "Connexion" })
                            </a>
                            <a href="/panier" class="inline-flex items-center gap-2 rounded-full bg-oat-900 px-4 py-2 text-sm text-oat-50 transition hover:bg-gin-800">
                                "Panier"
                                if items > 0 {
                                    <span class="inline-flex h-5 items-center justify-center rounded-full bg-oat-50 px-1.5 text-xs font-medium tabular-nums text-oat-900">(items)</span>
                                }
                            </a>
                        </div>
                    </div>
                </header>

                <main id="contenu" class="mx-auto max-w-6xl px-6 py-14">(content)</main>

                <footer class="mt-10 border-t border-oat-200 bg-oat-100">
                    <div class="mx-auto max-w-6xl px-6 py-14">
                        <div class="grid gap-10 sm:grid-cols-2 lg:grid-cols-4">
                            <div>
                                <p class="font-display text-2xl">"Bernard"</p>
                                <p class=("mt-3 text-sm leading-relaxed ".to_string() + MUTED)>
                                    "Le vestiaire et la papeterie d'un hébergeur qui aime les \
                                     petites coquilles. Dessiné à Brest, fabriqué au plus près."
                                </p>
                            </div>
                            <div>
                                <p class="text-sm font-medium">"Boutique"</p>
                                <ul class=("mt-4 space-y-2.5 text-sm ".to_string() + MUTED)>
                                    <li><a href="/boutique" class="transition hover:text-gin-700">"Tous les articles"</a></li>
                                    <li><a href="/boutique?categorie=Vêtements" class="transition hover:text-gin-700">"Vêtements"</a></li>
                                    <li><a href="/boutique?categorie=Papeterie" class="transition hover:text-gin-700">"Papeterie"</a></li>
                                    <li><a href="/recherche" class="transition hover:text-gin-700">"Recherche"</a></li>
                                </ul>
                            </div>
                            <div>
                                <p class="text-sm font-medium">"Aide"</p>
                                <ul class=("mt-4 space-y-2.5 text-sm ".to_string() + MUTED)>
                                    <li><a href="/aide" class="transition hover:text-gin-700">"Livraison et retours"</a></li>
                                    <li><a href="/aide" class="transition hover:text-gin-700">"Guide des tailles"</a></li>
                                    <li><a href="/contact" class="transition hover:text-gin-700">"Nous écrire"</a></li>
                                    <li><a href="/compte" class="transition hover:text-gin-700">"Suivre ma commande"</a></li>
                                </ul>
                            </div>
                            <div>
                                <p class="text-sm font-medium">"La maison"</p>
                                <ul class=("mt-4 space-y-2.5 text-sm ".to_string() + MUTED)>
                                    <li><a href="/maison" class="transition hover:text-gin-700">"Notre histoire"</a></li>
                                    <li><a href="/journal" class="transition hover:text-gin-700">"Journal"</a></li>
                                    <li><a href="/cgv" class="transition hover:text-gin-700">"Conditions de vente"</a></li>
                                    <li><a href="/mentions-legales" class="transition hover:text-gin-700">"Mentions légales"</a></li>
                                </ul>
                            </div>
                        </div>
                        <p class=("mt-12 border-t border-oat-200 pt-6 text-xs ".to_string() + MUTED)>
                            "© 2026 Bernard SAS — boutique de démonstration : les commandes sont \
                             réelles côté logiciel, mais aucun colis ne part et aucun paiement \
                             n'est demandé."
                        </p>
                    </div>
                </footer>
            </body>
        </html>
    }
}

// --- home

const PROMISES: [(&str, &str, &str); 4] = [
    ("📦", "Livraison offerte", "Dès 50 € d'achat, partout en France."),
    ("↩️", "Trente jours", "Retours acceptés, échange de taille compris."),
    ("🧵", "Matières choisies", "Coton biologique, papiers épais, rien de creux."),
    ("🦀", "Fait à Brest", "Dessiné au bord de l'eau, produit au plus près."),
];

#[query_params(error = bad_request)]
struct NewsletterState {
    lettre: Option<String>,
}

#[derive(serde::Deserialize)]
struct Subscription {
    email: String,
}

/// A plain form POST with a redirect: refresh-proof, JavaScript-free.
#[route(POST "/newsletter")]
async fn subscribe(cx: &Cx, Form(f): Form<Subscription>) -> Result<SeeOther> {
    if !f.email.contains('@') {
        return Ok(see_other("/?lettre=invalide#lettre"));
    }
    db::subscribe(pool(cx), &f.email).await?;
    Ok(see_other("/?lettre=merci#lettre"))
}

#[page("/")]
async fn home(cx: &Cx) -> Result {
    let featured = db::new_arrivals(pool(cx), 4).await?;
    let categories = db::categories(pool(cx)).await?;
    let letter = query_params::<NewsletterState>(cx)?.lettre.clone().unwrap_or_default();
    let thanks = letter == "merci";
    let invalid = letter == "invalide";

    view! {
        <section class="grid items-center gap-12 lg:grid-cols-2">
            <div>
                <p class=(EYEBROW)>"Collection de saison"</p>
                <h1 class="mt-4 text-5xl leading-tight sm:text-6xl">
                    "Des objets qui tiennent" <br> "dans une petite coquille."
                </h1>
                <p class=("mt-6 max-w-xl text-lg leading-relaxed ".to_string() + SOFT)>
                    "Le vestiaire et la papeterie de Bernard : peu de pièces, choisies pour \
                     durer, dans des matières qu'on peut nommer. Ce qui n'est pas nécessaire \
                     n'a pas été ajouté — c'est la même règle que pour nos images."
                </p>
                <div class="mt-9 flex flex-wrap items-center gap-3">
                    <a href="/boutique" class=(BTN)>"Découvrir la collection"</a>
                    <a href="/journal" class=(BTN_OUTLINE)>"Lire le journal"</a>
                </div>
            </div>

            <div class="grid grid-cols-2 gap-4">
                for (i, p) in featured.iter().enumerate() {
                    // The second tile drops for a hand-stacked, off-grid look.
                    <a href=("/produit/".to_string() + &p.sku)
                       data-bg=(crate::images::background(&p.sku))
                       class=(if i % 2 == 1 {
                           "block aspect-square overflow-hidden rounded-2xl bg-oat-100 ring-1 ring-oat-200 transition duration-300 hover:-translate-y-1 hover:shadow-xl hover:shadow-oat-900/10 sm:translate-y-6"
                       } else {
                           "block aspect-square overflow-hidden rounded-2xl bg-oat-100 ring-1 ring-oat-200 transition duration-300 hover:-translate-y-1 hover:shadow-xl hover:shadow-oat-900/10"
                       })>
                        <img src=(crate::images::url(&p.sku, 400))
                             srcset=(format!("{} 400w, {} 900w", crate::images::url(&p.sku, 400), crate::images::url(&p.sku, 900)))
                             sizes="(min-width: 1024px) 25vw, 50vw"
                             alt=(&p.name)
                             class="h-full w-full object-cover">
                    </a>
                }
            </div>
        </section>

        <section class="mt-24">
            <div class="flex flex-wrap items-end justify-between gap-4">
                <h2 class="text-3xl sm:text-4xl">"Les catégories"</h2>
                <a href="/boutique" class="text-sm text-gin-700 underline underline-offset-4">"Tout voir"</a>
            </div>
            <div class="mt-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                for c in categories {
                    <a href=("/boutique?categorie=".to_string() + &c)
                       class="group flex items-center justify-between rounded-2xl bg-white px-5 py-6 ring-1 ring-oat-200 transition hover:ring-gin-400">
                        <span class="text-lg">(&c)</span>
                        <span class="text-gin-700 transition group-hover:translate-x-1">"→"</span>
                    </a>
                }
            </div>
        </section>

        <section class="mt-24">
            <div class="flex flex-wrap items-end justify-between gap-4">
                <div>
                    <p class=(EYEBROW)>"Sélection"</p>
                    <h2 class="mt-3 text-3xl sm:text-4xl">"Ce qui vient d'arriver"</h2>
                </div>
                <a href="/boutique" class="text-sm text-gin-700 underline underline-offset-4">"Toute la boutique"</a>
            </div>
            <div class="mt-10 grid gap-x-6 gap-y-10 sm:grid-cols-2 lg:grid-cols-4">
                for p in featured {
                    product_tile(p: p)
                }
            </div>
        </section>

        <section class="mt-24 rounded-3xl bg-oat-100 px-6 py-12 ring-1 ring-oat-200 sm:px-10">
            <div class="grid gap-8 sm:grid-cols-2 lg:grid-cols-4">
                for (icon, title, text) in PROMISES {
                    <div>
                        <span class="text-2xl">(icon)</span>
                        <p class="mt-3 font-medium">(title)</p>
                        <p class=("mt-1 text-sm leading-relaxed ".to_string() + MUTED)>(text)</p>
                    </div>
                }
            </div>
        </section>

        <section id="lettre" class="relative mt-24 overflow-hidden rounded-3xl bg-gin-900 px-6 py-14 text-center sm:px-10">
            // The tide itself, blurred behind the same green veil as the
            // order banner: one letter per tide, and here is the tide.
            <img src=(crate::images::url("lettre-maree", 900))
                 alt=""
                 aria-hidden="true"
                 loading="lazy"
                 class="absolute inset-0 h-full w-full scale-110 object-cover blur-[3px]">
            <div class="absolute inset-0 bg-gin-900/70"></div>
            <div class="relative">
            <p class="text-xs font-medium uppercase tracking-[0.2em] text-gin-200">"La lettre de la coquille"</p>
            <h2 class="mt-3 text-3xl text-oat-50 sm:text-4xl">"Une lettre par marée, pas plus"</h2>
            <p class="mx-auto mt-4 max-w-xl leading-relaxed text-gin-100">
                "Les nouvelles séries, les retours en stock et un billet du journal. \
                 Environ une par mois, désinscription en un clic."
            </p>
            if thanks {
                <p class="animate-apparition mx-auto mt-8 inline-flex items-center gap-2 rounded-full bg-oat-50 px-5 py-2.5 text-sm font-medium text-gin-800">
                    "✓ Bienvenue à bord — première lettre à la prochaine marée."
                </p>
            } else {
                <form method="post" action="/newsletter" class="mx-auto mt-8 flex max-w-md gap-3">
                    <input name="email" type="email" required="required" placeholder="vous@exemple.fr"
                           class="w-full rounded-full bg-gin-900/60 px-5 py-2.5 text-sm text-oat-50 ring-1 ring-gin-600 outline-none transition placeholder:text-gin-300 focus:ring-2 focus:ring-oat-50">
                    <button class="shrink-0 rounded-full bg-oat-50 px-5 py-2.5 text-sm font-medium text-gin-900 transition hover:bg-oat-100">"S'abonner"</button>
                </form>
                if invalid {
                    <p class="mt-4 text-sm text-brique-100">"Cette adresse ne ressemble pas à une adresse."</p>
                }
            }
            </div>
        </section>

        <section class="mt-24 grid items-center gap-10 lg:grid-cols-2">
            <div class="aspect-video overflow-hidden rounded-3xl bg-oat-100 ring-1 ring-oat-200"
                 data-bg=(crate::images::background("journal-coton"))>
                <img src=(crate::images::url("journal-coton", 900))
                     srcset=(format!("{} 900w, {} 1600w", crate::images::url("journal-coton", 900), crate::images::url("journal-coton", 1600)))
                     sizes="(min-width: 1024px) 50vw, 100vw"
                     alt="Fleurs de coton brut"
                     loading="lazy"
                     class="h-full w-full object-cover">
            </div>
            <div>
                <p class=(EYEBROW)>"Le journal"</p>
                <h2 class="mt-3 text-3xl sm:text-4xl">"D'où viennent nos matières"</h2>
                <p class=("mt-5 text-lg leading-relaxed ".to_string() + SOFT)>
                    "On raconte ce qu'on mesure et ce qu'on choisit : le coton, l'encre, le \
                     papier, et pourquoi une petite série vaut mieux qu'un grand entrepôt."
                </p>
                <a href="/journal" class=(BTN_OUTLINE.to_string() + " mt-8")>"Lire le journal"</a>
            </div>
        </section>
    }
}
