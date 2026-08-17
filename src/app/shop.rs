//! The catalog, the product page and search. Filtering and sorting are
//! links with query parameters: shareable, bookmarkable, and they work
//! before a single line of JavaScript has run.

use topcoat::context::Cx;
use topcoat::router::error::{see_other, RouterErrorExt, SeeOther};
use topcoat::router::{content::Form, page, path_param, query_params, route};
use topcoat::runtime::{procedure, shard, Event};
use topcoat::view::view;
use topcoat::Result;

use crate::app::context::{current_cart, current_user, note_seen, pool};
use crate::app::{page_heading, product_tile, BTN, BTN_OUTLINE, CARD, EYEBROW, FIELD, MUTED, SOFT};
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

/// Sets what the cart holds of one line and hands back what was kept: the
/// server clamps to the stock and retires the line at zero.
#[procedure]
async fn set_line(cx: &Cx, sku: String, size: String, quantity: f64) -> Result<f64> {
    let cart = current_cart(cx);
    Ok(db::set_quantity(pool(cx), &cart, &sku, &size, quantity as i64).await? as f64)
}

/// What the cart holds of one line. Picking another size asks for its
/// count rather than trusting what the page was rendered with.
#[procedure]
async fn in_cart(cx: &Cx, sku: String, size: String) -> Result<f64> {
    let cart = current_cart(cx);
    let lines = db::cart_lines(pool(cx), &cart).await?;
    Ok(lines.iter().find(|l| l.sku == sku && l.size == size).map_or(0, |l| l.quantity) as f64)
}

path_param!(sku);

#[page("/produit/{sku}")]
async fn product(cx: &Cx) -> Result {
    let sku = path_param::<Sku>(cx).to_string();
    // A hidden product has left the floor: its page answers 404 like any
    // reference that never existed.
    let p = db::product(pool(cx), &sku).await?.filter(|p| p.hidden == 0).ok_or_not_found()?;
    let variants = db::variants(pool(cx), &sku).await?;
    let related = db::related_products(pool(cx), &sku, &p.category).await?;
    let reviews = db::product_reviews(pool(cx), &sku).await?;
    let visitor = current_user(cx).await?;
    let signed_in = visitor.is_some();
    let is_admin = visitor.is_some_and(|u| u.admin != 0);

    // The shelf shows what the visitor saw before this page, not this page.
    let seen_before = note_seen(cx, &sku);
    let mut already_seen = Vec::new();
    for s in seen_before.iter().take(4) {
        if let Some(seen) = db::product(pool(cx), s).await? {
            already_seen.push(seen);
        }
    }
    let has_seen = !already_seen.is_empty();

    let state = query_params::<ProductState>(cx)?;
    let alert_thanks = state.alert.as_deref() == Some("merci");
    let review_thanks = state.review.as_deref() == Some("merci");
    let review_count = reviews.len();
    let average = if review_count > 0 {
        reviews.iter().map(|r| r.rating).sum::<i64>() as f64 / review_count as f64
    } else {
        0.0
    };
    let missing: Vec<String> = variants
        .iter()
        .filter(|v| !v.size.is_empty() && v.stock == 0)
        .map(|v| v.size.clone())
        .collect();
    let has_missing = !missing.is_empty();

    let has_sizes = variants.iter().any(|v| !v.size.is_empty());
    let sold_out = p.sold_out();
    let low = p.low_stock();
    let page_sku = p.sku.clone();
    // Preselect the first size actually in stock. The stepper's ceiling is
    // that size's stock, not the whole reserve: a size with two left must
    // not offer twelve.
    let first = variants.iter().find(|v| v.stock > 0);
    let first_size = first.map(|v| v.size.clone()).unwrap_or_default();
    let first_stock = first.map_or(p.stock, |v| v.stock) as f64;

    // The stepper counts what the cart already holds: the number on the
    // page is the line it changes.
    let held = db::cart_lines(pool(cx), &current_cart(cx)).await?;
    let held_of = |size: &str| {
        held.iter().find(|l| l.sku == sku && l.size == size).map_or(0, |l| l.quantity) as f64
    };
    let first_held = held_of(&first_size);

    view! {
        signal sku_sig = page_sku;
        signal size = first_size;
        signal size_stock = first_stock;
        signal held_qty = first_held;

        <div class="flex items-center justify-between gap-4">
            <nav class=("text-sm ".to_string() + MUTED)>
                <a href="/boutique" class="transition hover:text-gin-700">"Boutique"</a>
                " / "
                <a href=("/boutique?categorie=".to_string() + &p.category) class="transition hover:text-gin-700">(&p.category)</a>
            </nav>
            // The bridge the shopkeeper crosses; nobody else sees it.
            if is_admin {
                <a href=("/admin/produit/".to_string() + &p.sku) class=(BTN_OUTLINE)>"Éditer"</a>
            }
        </div>

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
                    if alert_thanks {
                        <p class="animate-apparition mt-4 rounded-xl bg-gin-50 px-4 py-3 text-sm text-gin-800" id="dispo">
                            "C'est noté — un mot dès que la marée le ramène."
                        </p>
                    } else {
                        <form method="post" action="/alerte" class="mt-4 flex flex-wrap items-center gap-2" id="dispo">
                            <input type="hidden" name="sku" value=(&p.sku)>
                            <input type="hidden" name="size" value="">
                            <input class="w-60 rounded-xl bg-white px-3.5 py-2.5 text-sm ring-1 ring-oat-300" type="email" name="email" required="required" placeholder="vous@exemple.fr" aria-label="Votre email">
                            <button class=(BTN_OUTLINE)>"Me prévenir du retour"</button>
                        </form>
                    }
                } else {
                    if has_sizes {
                        <div class="mt-8">
                            <div class="flex items-baseline justify-between">
                                <p class="text-sm font-medium">"Taille"</p>
                                <a href="/aide#tailles" class=("text-sm underline underline-offset-4 ".to_string() + MUTED)>"Guide des tailles"</a>
                            </div>
                            <div class="mt-3 flex flex-wrap gap-2">
                                for v in variants.iter().filter(|v| !v.size.is_empty()) {
                                    // A signal per option: a handler can capture a
                                    // signal, never a loop variable.
                                    signal label = v.size.clone();
                                    signal option_stock = v.stock as f64;

                                    if v.stock > 0 {
                                        <button
                                            :class=$(if size.get() == label.get() {
                                                "min-w-14 rounded-xl bg-oat-900 px-4 py-2.5 text-sm text-oat-50"
                                            } else {
                                                "min-w-14 rounded-xl px-4 py-2.5 text-sm ring-1 ring-oat-300 transition hover:ring-oat-900"
                                            })
                                            @click=$(async |_e| {
                                                size.set(label.get());
                                                size_stock.set(option_stock.get());
                                                held_qty.set(in_cart(sku_sig.get(), label.get()).await);
                                            })>(&v.size)</button>
                                    } else {
                                        <span class="min-w-14 rounded-xl px-4 py-2.5 text-center text-sm text-oat-400 line-through ring-1 ring-oat-200">(&v.size)</span>
                                    }
                                }
                            </div>

                            if has_missing {
                                if alert_thanks {
                                    <p class="animate-apparition mt-3 rounded-xl bg-gin-50 px-4 py-3 text-sm text-gin-800" id="dispo">
                                        "C'est noté — un mot dès que la taille revient."
                                    </p>
                                } else {
                                    <details class="mt-3" id="dispo">
                                        <summary class=("text-sm underline underline-offset-4 ".to_string() + MUTED)>
                                            "Votre taille manque ? On vous prévient dès son retour."
                                        </summary>
                                        <form method="post" action="/alerte" class="mt-3 flex flex-wrap items-center gap-2">
                                            <input type="hidden" name="sku" value=(&p.sku)>
                                            <select name="size" class="rounded-xl bg-white px-3 py-2.5 text-sm ring-1 ring-oat-300" aria-label="Taille épuisée">
                                                for s in &missing {
                                                    <option value=(s)>(s)</option>
                                                }
                                            </select>
                                            <input class="w-60 rounded-xl bg-white px-3.5 py-2.5 text-sm ring-1 ring-oat-300" type="email" name="email" required="required" placeholder="vous@exemple.fr" aria-label="Votre email">
                                            <button class=(BTN_OUTLINE)>"Me prévenir"</button>
                                        </form>
                                    </details>
                                }
                            }
                        </div>
                    }

                    <div class="mt-8 flex flex-wrap items-center gap-4">
                        // Empty cart line: the stepper has nothing to show yet
                        // and the button is the way in.
                        <button class=(BTN.to_string() + " h-11 px-8")
                                :hidden=$(held_qty.get() > 0.0)
                                @click=$(async |_e| {
                                    held_qty.set(set_line(sku_sig.get(), size.get(), 1.0).await);
                                })>"Ajouter au panier"</button>

                        <div class="inline-flex items-center overflow-hidden rounded-full ring-1 ring-oat-300"
                             :hidden=$(held_qty.get() == 0.0)>
                            <button aria-label="Retirer un exemplaire du panier"
                                    class="flex h-11 w-11 cursor-pointer select-none items-center justify-center rounded-l-full text-lg transition hover:bg-oat-100"
                                    @click=$(async |_e| {
                                        held_qty.set(set_line(sku_sig.get(), size.get(), held_qty.get() - 1.0).await);
                                    })>"−"</button>
                            <span class="w-9 text-center tabular-nums">$(held_qty.get())</span>
                            <button aria-label="Ajouter un exemplaire au panier"
                                    :class=$(if held_qty.get() >= size_stock.get() {
                                        "flex h-11 w-11 select-none items-center justify-center rounded-r-full text-lg text-oat-300"
                                    } else {
                                        "flex h-11 w-11 cursor-pointer select-none items-center justify-center rounded-r-full text-lg transition hover:bg-oat-100"
                                    })
                                    // No ceiling guard: an `if` around an await
                                    // compiles to a plain arrow the runtime
                                    // cannot run, and the server clamps anyway.
                                    @click=$(async |_e| {
                                        held_qty.set(set_line(sku_sig.get(), size.get(), held_qty.get() + 1.0).await);
                                    })>"+"</button>
                        </div>

                        <a href="/panier" class=(BTN_OUTLINE.to_string() + " h-11")
                           :hidden=$(held_qty.get() == 0.0)>"Voir le panier"</a>
                    </div>

                    <p class="mt-4 text-sm text-brique-700" :hidden=$(held_qty.get() < size_stock.get())>
                        "Vous avez tout le stock disponible dans votre panier."
                    </p>

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

        <section class="mt-24" id="avis">
            <div class="flex flex-wrap items-baseline justify-between gap-4">
                <h2 class="text-3xl">"Les avis"</h2>
                if review_count > 0 {
                    <p class=("text-sm ".to_string() + MUTED)>
                        <span class="tracking-wider text-gin-700">(stars(average.round() as i64))</span>
                        (format!(" {average:.1} sur 5 — {review_count} avis"))
                    </p>
                }
            </div>

            if review_count == 0 {
                <p class=("mt-6 ".to_string() + SOFT)>"Pas encore d'avis — cette pièce attend son premier retour."</p>
            } else {
                <ul class="mt-8 grid gap-6 lg:grid-cols-2">
                    for r in &reviews {
                        <li class=(CARD.to_string() + " p-6")>
                            <div class="flex items-baseline justify-between gap-3">
                                <span class="font-medium">(&r.author)</span>
                                <span class="text-sm tracking-wider text-gin-700" role="img" aria-label=(format!("{} sur 5", r.rating))>(stars(r.rating))</span>
                            </div>
                            <p class=("mt-3 text-sm leading-relaxed ".to_string() + SOFT)>(&r.text)</p>
                            <time class=("mt-3 block text-xs ".to_string() + MUTED)>(r.created_at.get(..10).unwrap_or_default().to_string())</time>
                        </li>
                    }
                </ul>
            }

            if review_thanks {
                <p class="animate-apparition mt-8 inline-flex rounded-full bg-gin-700 px-4 py-2 text-sm text-oat-50">"Merci pour votre avis !"</p>
            } else {
                if signed_in {
                    <form method="post" action=(format!("/produit/{}/avis", p.sku)) class=(CARD.to_string() + " mt-8 max-w-xl space-y-4 p-6")>
                        <p class="font-medium">"Votre avis"</p>
                        <div class="flex items-center gap-3">
                            <label class="text-sm" for="note">"Note"</label>
                            <select id="note" name="rating" class="rounded-xl bg-oat-50 px-3 py-2 text-sm ring-1 ring-oat-300">
                                <option value="5">"5 — Impeccable"</option>
                                <option value="4">"4 — Très bien"</option>
                                <option value="3">"3 — Correct"</option>
                                <option value="2">"2 — Déçu"</option>
                                <option value="1">"1 — Non"</option>
                            </select>
                        </div>
                        <textarea name="text" rows="3" required="required" minlength="10" class=(FIELD) placeholder="La matière, la coupe, la vie avec."></textarea>
                        <button class=(BTN)>"Publier"</button>
                    </form>
                } else {
                    <p class=("mt-8 text-sm ".to_string() + MUTED)>
                        <a href="/connexion" class="underline underline-offset-4">"Connectez-vous"</a>
                        " pour laisser un avis."
                    </p>
                }
            }
        </section>

        <section class="mt-24">
            <h2 class="text-3xl">"À voir aussi"</h2>
            <div class="mt-8 grid gap-x-6 gap-y-10 sm:grid-cols-3">
                for r in related {
                    product_tile(p: r)
                }
            </div>
        </section>

        if has_seen {
            <section class="mt-24">
                <h2 class="text-3xl">"Déjà regardés"</h2>
                <div class="mt-8 grid gap-x-6 gap-y-10 sm:grid-cols-2 lg:grid-cols-4">
                    for seen in already_seen {
                        product_tile(p: seen)
                    }
                </div>
            </section>
        }
    }
}

// --- reviews and stock alerts

#[query_params(error = bad_request)]
struct ProductState {
    #[serde(rename = "alerte")]
    alert: Option<String>,
    #[serde(rename = "avis")]
    review: Option<String>,
}

fn stars(rating: i64) -> String {
    let full = rating.clamp(0, 5) as usize;
    "★".repeat(full) + &"☆".repeat(5 - full)
}

/// "Camille Rivoal" becomes "Camille R." -- a review signs with a first
/// name, not a directory entry.
fn short_name(name: &str) -> String {
    let mut words = name.split_whitespace();
    let first = words.next().unwrap_or("Client");
    match words.last().and_then(|rest| rest.chars().next()) {
        Some(initial) => format!("{first} {initial}."),
        None => first.to_string(),
    }
}

#[derive(serde::Deserialize)]
struct AlertRequest {
    sku: String,
    #[serde(default)]
    size: String,
    email: String,
}

/// PRG: the alert lands in the base, the visitor lands back on the page.
#[route(POST "/alerte")]
async fn stock_alert(cx: &Cx, Form(f): Form<AlertRequest>) -> Result<SeeOther> {
    db::product(pool(cx), &f.sku).await?.ok_or_not_found()?;
    db::create_stock_alert(pool(cx), &f.sku, &f.size, &f.email).await?;
    Ok(see_other(format!("/produit/{}?alerte=merci#dispo", f.sku)))
}

#[derive(serde::Deserialize)]
struct NewReview {
    rating: i64,
    text: String,
}

#[route(POST "/produit/{sku}/avis")]
async fn publish_review(cx: &Cx, Form(f): Form<NewReview>) -> Result<SeeOther> {
    let user = current_user(cx).await?.ok_or_redirect("/connexion")?;
    let sku = path_param::<Sku>(cx).to_string();
    db::product(pool(cx), &sku).await?.ok_or_not_found()?;
    if f.text.trim().len() < 10 {
        return Ok(see_other(format!("/produit/{sku}#avis")));
    }
    db::add_review(pool(cx), &sku, &short_name(&user.name), f.rating, &f.text).await?;
    Ok(see_other(format!("/produit/{sku}?avis=merci#avis")))
}
