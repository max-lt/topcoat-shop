//! The catalog, the product page and search. Filtering and sorting are
//! links with query parameters: shareable, bookmarkable, and they work
//! before a single line of JavaScript has run.

use topcoat::context::Cx;
use topcoat::router::error::RouterErrorExt;
use topcoat::router::{page, path_param, query_params};
use topcoat::runtime::{shard, Event};
use topcoat::view::view;
use topcoat::Result;

use crate::app::context::pool;
use crate::app::{page_heading, product_tile, BTN_OUTLINE, EYEBROW, FIELD, MUTED, SOFT};
use crate::db::{self, format_price};

const SORTS: [(i64, &str); 4] = [
    (0, "Nouveautés"),
    (1, "Prix croissant"),
    (2, "Prix décroissant"),
    (3, "Alphabétique"),
];

/// The query keys stay French: they are part of the shop's public URLs.
#[query_params(error = bad_request)]
pub struct Filters {
    #[serde(rename = "categorie")]
    pub category: Option<String>,
    #[serde(rename = "tri")]
    pub sort: Option<i64>,
}

#[page("/boutique")]
async fn shop(cx: &Cx) -> Result {
    let filters = query_params::<Filters>(cx)?;
    let category = filters.category.clone().unwrap_or_default();
    let sort = filters.sort.unwrap_or(0);

    let categories = db::categories(pool(cx)).await?;
    let products = db::catalog(pool(cx), &category, sort).await?;
    let how_many = products.len();
    let empty = products.is_empty();

    // A filtered shelf is a page of its own: it deserves its name and a
    // line of introduction, not the generic all-collection lede.
    let (shelf_title, shelf_lede) = match category.as_str() {
        "Accessoires" => (
            "Accessoires",
            "Ce qui accompagne\u{202f}: la gourde des allers-retours, le tote du marché, \
             le porte-clés qui ne quitte plus la poche.",
        ),
        "Affiches" => (
            "Affiches",
            "Le benchmark encadré, la rade en carte marine, la typo au plomb\u{202f}: \
             de quoi tenir un mur de bureau en respect.",
        ),
        "Maison" => (
            "Maison",
            "Le mug des revues de code, la théière d'un litre, la bougie des soirs \
             de mise en production.",
        ),
        "Papeterie" => (
            "Papeterie",
            "Carnets de post-mortem, crayons gravés, cartes qu'on écrit pour de \
             vrai\u{202f}: du papier qui prend l'encre sans baver.",
        ),
        "Vêtements" => (
            "Vêtements",
            "Coton qu'on peut nommer, laine qui tient l'hiver breton, séries \
             courtes — le vestiaire complet, bonnet compris.",
        ),
        _ => (
            "Toute la collection",
            "Peu de références, choisies pour durer. Les stocks affichés sont \
             réels\u{202f}: quand une taille manque, c'est qu'elle est partie.",
        ),
    };

    // Each chip is a link carrying the other half of the state, so the two
    // filters compose instead of resetting one another.
    let link = |cat: &str, s: i64| format!("/boutique?categorie={cat}&tri={s}");

    view! {
        page_heading(
            eyebrow: "La boutique",
            title: shelf_title,
            lede: shelf_lede
        )

        <div class="mt-10 flex flex-wrap items-center gap-x-8 gap-y-4 border-y border-oat-200 py-4">
            <div class="flex flex-wrap items-center gap-2">
                <a href=(link("", sort))
                   class=(if category.is_empty() {
                       "rounded-full bg-oat-900 px-4 py-1.5 text-sm text-oat-50"
                   } else {
                       "rounded-full px-4 py-1.5 text-sm ring-1 ring-oat-300 transition hover:bg-oat-100"
                   })>"Tout"</a>
                for c in categories {
                    <a href=(link(&c, sort))
                       class=(if category == c {
                           "rounded-full bg-oat-900 px-4 py-1.5 text-sm text-oat-50"
                       } else {
                           "rounded-full px-4 py-1.5 text-sm ring-1 ring-oat-300 transition hover:bg-oat-100"
                       })>(&c)</a>
                }
            </div>

            <div class="ml-auto flex items-center gap-3 text-sm">
                <span class=(MUTED)>"Trier par"</span>
                <div class="flex flex-wrap gap-3">
                    for (value, label) in SORTS {
                        <a href=(link(&category, value))
                           class=(if sort == value {
                               "text-gin-700 underline underline-offset-4"
                           } else {
                               "text-oat-600 transition hover:text-gin-700"
                           })>(label)</a>
                    }
                </div>
            </div>
        </div>

        <p class=("mt-6 text-sm ".to_string() + MUTED)>
            (format!("{how_many} article{}", if how_many > 1 { "s" } else { "" }))
        </p>

        if empty {
            <p class=("mt-16 text-center text-lg ".to_string() + SOFT)>
                "Rien dans cette catégorie pour l'instant."
            </p>
        } else {
            <div class="mt-8 grid gap-x-6 gap-y-12 sm:grid-cols-2 lg:grid-cols-3">
                for p in products {
                    product_tile(p: p)
                }
            </div>
        }
    }
}

// --- search

#[query_params(error = bad_request)]
struct SearchQuery {
    q: Option<String>,
}

/// Re-rendered by the server on every keystroke. An empty field is not an
/// empty page: it shows a selection, so the visitor always has something to
/// look at.
#[shard]
async fn results(cx: &Cx, term: String) -> Result {
    let searching = !term.trim().is_empty();
    let products = if searching {
        db::search(pool(cx), &term).await?
    } else {
        db::new_arrivals(pool(cx), 6).await?
    };
    let how_many = products.len();
    let nothing = searching && products.is_empty();

    view! {
        if nothing {
            <div class="py-16 text-center">
                <p class="font-display text-3xl">"Rien pour « " (&term) " »"</p>
                <p class=("mt-3 ".to_string() + SOFT)>
                    "Essayez « coton », « laiton », « papier », ou parcourez toute la collection."
                </p>
                <a href="/boutique" class=(BTN_OUTLINE.to_string() + " mt-8")>"Voir la boutique"</a>
            </div>
        } else {
            <p class=("text-sm ".to_string() + MUTED)>
                if searching {
                    (format!("{how_many} résultat{}", if how_many > 1 { "s" } else { "" }))
                } else {
                    "Une sélection pour commencer"
                }
            </p>
            <div class="animate-apparition mt-6 grid gap-x-6 gap-y-12 sm:grid-cols-2 lg:grid-cols-3">
                for p in products {
                    product_tile(p: p)
                }
            </div>
        }
    }
}

#[page("/recherche")]
async fn search(cx: &Cx) -> Result {
    let initial = query_params::<SearchQuery>(cx)?.q.clone().unwrap_or_default();

    view! {
        signal term = initial;

        page_heading(
            eyebrow: "Recherche",
            title: "Trouver un article",
            lede: "Les résultats se mettent à jour pendant que vous tapez."
        )

        // Still a real form: without JavaScript, Enter submits and the page
        // re-renders from the query string.
        <form action="/recherche" method="get" class="mt-8 max-w-xl">
            <input name="q" type="search" autofocus="autofocus" autocomplete="off"
                   placeholder="coton, laiton, affiche…" class=(FIELD)
                   :value=$(term.get())
                   @input=$(|e: Event| term.set(e.target.value))>
        </form>

        <div class="mt-10">
            results(term: $(term.get()))
        </div>
    }
}

// --- product page

#[path_param]
struct Sku(str);

#[page("/produit/{sku}")]
async fn product(cx: &Cx) -> Result {
    let sku = path_param::<Sku>(cx).to_string();
    // A hidden product has left the floor: its page answers 404 like any
    // reference that never existed.
    let p = db::product(pool(cx), &sku).await?.filter(|p| p.hidden == 0).ok_or_not_found()?;
    let variants = db::variants(pool(cx), &sku).await?;
    let related = db::related_products(pool(cx), &sku, &p.category).await?;

    let has_sizes = variants.iter().any(|v| !v.size.is_empty());
    let sold_out = p.sold_out();
    let low = p.low_stock();

    view! {
        <nav class=("text-sm ".to_string() + MUTED)>
            <a href="/boutique" class="transition hover:text-gin-700">"Boutique"</a>
            " / "
            <a href=("/boutique?categorie=".to_string() + &p.category) class="transition hover:text-gin-700">(&p.category)</a>
        </nav>

        <div class="mt-6 grid gap-12 lg:grid-cols-2">
            // data-vt plus a static CSS attr() rule carries the morph name: a
            // dynamic style= attribute silently kills hydration of everything
            // after it (topcoat-view 0.5 bug, bisected down to the one line).
            <div class="relative aspect-square overflow-hidden rounded-3xl bg-oat-100 ring-1 ring-oat-200"
                 data-vt=(&p.sku)
                 data-bg=(crate::images::background(&p.sku))>
                // The 400 px tile the visitor just clicked is already in the
                // browser cache: blurred underneath, it bridges the wait -- a
                // loading <img> paints nothing, so the small one shows through
                // until the big one covers it.
                <img src=(crate::images::url(&p.sku, 400))
                     alt=""
                     aria-hidden="true"
                     class="absolute inset-0 h-full w-full scale-105 object-cover blur-sm">
                <img src=(crate::images::url(&p.sku, 900))
                     srcset=(format!("{} 900w, {} 1600w", crate::images::url(&p.sku, 900), crate::images::url(&p.sku, 1600)))
                     sizes="(min-width: 1024px) 50vw, 100vw"
                     alt=(&p.name)
                     fetchpriority="high"
                     class="relative h-full w-full object-cover">
            </div>

            <div class="lg:pt-6">
                if p.is_new != 0 {
                    <p class=(EYEBROW)>"Nouveauté"</p>
                }
                <h1 class="mt-2 text-4xl leading-tight sm:text-5xl">(&p.name)</h1>
                <p class="mt-4 font-display text-4xl">(format_price(p.price_cents))</p>
                <p class=("mt-6 leading-relaxed ".to_string() + SOFT)>(&p.detail)</p>

                <dl class="mt-6 flex gap-8 border-y border-oat-200 py-4 text-sm">
                    <div>
                        <dt class=(MUTED)>"Matière"</dt>
                        <dd class="mt-1">(&p.material)</dd>
                    </div>
                    <div>
                        <dt class=(MUTED)>"Référence"</dt>
                        <dd class="mt-1 tabular-nums">(&p.sku)</dd>
                    </div>
                </dl>

                if sold_out {
                    <p class="mt-8 rounded-2xl bg-brique-100 px-5 py-4 text-sm text-brique-700">
                        "Épuisé pour le moment. La prochaine série arrive avec la marée."
                    </p>
                } else {
                    if has_sizes {
                        <div class="mt-8">
                            <div class="flex items-baseline justify-between">
                                <p class="text-sm font-medium">"Taille"</p>
                                <a href="/aide#tailles" class=("text-sm underline underline-offset-4 ".to_string() + MUTED)>"Guide des tailles"</a>
                            </div>
                            <div class="mt-3 flex flex-wrap gap-2">
                                for v in variants.iter().filter(|v| !v.size.is_empty()) {
                                    if v.stock > 0 {
                                        <span class="min-w-14 rounded-xl px-4 py-2.5 text-center text-sm ring-1 ring-oat-300">(&v.size)</span>
                                    } else {
                                        <span class="min-w-14 rounded-xl px-4 py-2.5 text-center text-sm text-oat-400 line-through ring-1 ring-oat-200">(&v.size)</span>
                                    }
                                }
                            </div>
                        </div>
                    }

                    if low {
                        <p class="mt-4 text-sm text-brique-700">
                            (format!("Plus que {} exemplaires", p.stock))
                        </p>
                    }

                }

                <ul class=("mt-10 space-y-2 text-sm ".to_string() + MUTED)>
                    <li>"Livraison offerte dès " (format_price(db::FREE_SHIPPING_CENTS))</li>
                    <li>"Retour accepté 30 jours, échange de taille compris"</li>
                    <li>"Expédié de Brest sous 48 heures"</li>
                </ul>
            </div>
        </div>

        <section class="mt-24">
            <h2 class="text-3xl">"À voir aussi"</h2>
            <div class="mt-8 grid gap-x-6 gap-y-10 sm:grid-cols-3">
                for r in related {
                    product_tile(p: r)
                }
            </div>
        </section>
    }
}
