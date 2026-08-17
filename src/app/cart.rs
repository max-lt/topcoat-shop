//! Cart and checkout. The rows and their steppers are rendered by the page,
//! because only there can a handler reach the signal that refreshes the
//! bill; the bill itself is a shard, so every total is the server's opinion
//! and never the browser's arithmetic.

use topcoat::context::Cx;

use topcoat::router::error::{see_other, RouterErrorExt, SeeOther};
use topcoat::router::{content::Form, page, query_params, route};
use topcoat::runtime::{procedure, shard};
use topcoat::view::view;
use topcoat::Result;

use crate::app::context::{current_cart, current_user, forget_cart, pool};
use crate::app::{page_heading, BTN, BTN_OUTLINE, CARD, EYEBROW, FIELD, MUTED, SOFT};
use crate::db::{self, format_price};

#[procedure]
async fn set_quantity(cx: &Cx, sku: String, size: String, quantity: f64) -> Result<f64> {
    let cart_id = current_cart(cx);
    Ok(db::set_quantity(pool(cx), &cart_id, &sku, &size, quantity as i64).await? as f64)
}

#[procedure]
async fn remove(cx: &Cx, sku: String, size: String) -> Result<f64> {
    let cart_id = current_cart(cx);
    db::remove_from_cart(pool(cx), &cart_id, &sku, &size).await?;
    Ok(0.0)
}

/// The bill: line totals, shipping, and what it all comes to. `version` is
/// the signal the steppers bump; `mode` is the shipping choice.
#[shard]
async fn bill(cx: &Cx, version: f64, mode: String) -> Result {
    let _ = version;
    let cart_id = current_cart(cx);
    let lines = db::cart_lines(pool(cx), &cart_id).await?;
    let subtotal: i64 = lines.iter().map(db::CartLine::subtotal).sum();
    let shipping = db::shipping_cents(subtotal, &mode);
    let free = shipping == 0 && subtotal > 0;
    let missing = db::FREE_SHIPPING_CENTS - subtotal;
    let empty = lines.is_empty();

    view! {
        if empty {
            <p class=("text-sm ".to_string() + MUTED)>"Rien à additionner pour l'instant."</p>
        } else {
            <ul class="space-y-3 text-sm">
                for l in lines {
                    <li class="flex items-baseline justify-between gap-3">
                        <span class=(MUTED)>
                            (&l.name)
                            if !l.size.is_empty() { " · " (&l.size) }
                            (format!(" × {}", l.quantity))
                        </span>
                        <span class="tabular-nums">(format_price(l.subtotal()))</span>
                    </li>
                }
            </ul>

            <dl class="mt-5 space-y-2 border-t border-oat-200 pt-5 text-sm">
                <div class="flex justify-between">
                    <dt class=(MUTED)>"Sous-total"</dt>
                    <dd class="tabular-nums">(format_price(subtotal))</dd>
                </div>
                <div class="flex justify-between">
                    <dt class=(MUTED)>"Livraison"</dt>
                    <dd class="tabular-nums">
                        if free { <span class="text-gin-700">"offerte"</span> } else { (format_price(shipping)) }
                    </dd>
                </div>
            </dl>

            <div class="mt-5 flex items-baseline justify-between border-t border-oat-200 pt-5">
                <span class="font-medium">"Total"</span>
                <span class="font-display text-3xl tabular-nums">(format_price(subtotal + shipping))</span>
            </div>

            if !free {
                <div class="mt-4 rounded-xl bg-gin-50 px-4 py-3">
                    <p class="text-sm text-gin-800">
                        "Plus que " (format_price(missing)) " pour la livraison offerte."
                    </p>
                    // The fill width rides a typed attr(): a dynamic style=
                    // attribute would trip the hydration bug.
                    <div class="mt-2 h-1.5 overflow-hidden rounded-full bg-gin-100">
                        <div class="h-full rounded-full bg-gin-600 transition-all duration-500"
                             data-gauge=(format!("{}%", (subtotal * 100 / db::FREE_SHIPPING_CENTS).clamp(0, 100)))></div>
                    </div>
                </div>
            }
        }
    }
}

#[query_params(error = bad_request)]
struct CartState {
    #[serde(rename = "ajuste")]
    clamped: Option<String>,
}

#[page("/panier")]
async fn cart(cx: &Cx) -> Result {
    let clamped = query_params::<CartState>(cx)?.clamped.is_some();
    let id = current_cart(cx);
    let lines = db::cart_lines(pool(cx), &id).await?;
    let signed_in = current_user(cx).await?.is_some();
    let empty = lines.is_empty();

    view! {
        signal version = 0.0;
        signal standard = "standard".to_string();

        page_heading(eyebrow: "Panier", title: "Votre sélection", lede: "")

        if clamped {
            <p class="animate-apparition mt-8 rounded-2xl bg-brique-100 px-5 py-4 text-sm text-brique-700">
                "Quelqu'un a été plus rapide sur les dernières pièces : votre panier \
                 vient d'être ramené aux quantités réellement disponibles."
            </p>
        }

        if empty {
            <div class="mt-16 text-center">
                <p class="text-6xl">"🦀"</p>
                <p class=("mt-6 text-lg ".to_string() + SOFT)>"Votre panier est vide."</p>
                <a href="/boutique" class=(BTN.to_string() + " mt-8")>"Voir la collection"</a>
            </div>
        } else {
            <div class="mt-10 grid gap-10 lg:grid-cols-[1.6fr_1fr]">
                <ul class="divide-y divide-oat-200 border-y border-oat-200">
                    for l in lines {
                        // One signal per line: what a handler may capture.
                        signal line_sku = l.sku.clone();
                        signal line_size = l.size.clone();
                        signal q = l.quantity as f64;
                        signal blocked = 0.0;
                        signal line_stock = l.stock as f64;

                        <li class="flex gap-5 py-6">
                            <a href=("/produit/".to_string() + &l.sku)
                               data-bg=(crate::images::background(&l.sku))
                               class="block h-24 w-24 shrink-0 overflow-hidden rounded-2xl bg-oat-100 ring-1 ring-oat-200">
                                <img src=(crate::images::url(&l.sku, 400))
                                     alt=(&l.name)
                                     loading="lazy"
                                     class="h-full w-full object-cover">
                            </a>

                            <div class="min-w-0 flex-1">
                                <div class="flex flex-wrap items-baseline justify-between gap-2">
                                    <a href=("/produit/".to_string() + &l.sku) class="text-lg transition hover:text-gin-700">(&l.name)</a>
                                    <span class="text-sm tabular-nums">(format_price(l.price_cents)) " l'unité"</span>
                                </div>
                                if !l.size.is_empty() {
                                    <p class=("mt-1 text-sm ".to_string() + MUTED)>"Taille " (&l.size)</p>
                                }

                                <div class="mt-4 flex items-center gap-4">
                                    <div class="relative">
                                        // The server clamps to the stock; when + changes
                                        // nothing, this bubble says why instead of letting
                                        // the counter freeze in silence.
                                        <span class="animate-bulle pointer-events-none absolute -top-9 left-0 whitespace-nowrap rounded-full bg-oat-900 px-3 py-1.5 text-xs text-oat-50 shadow-sm"
                                              :hidden=$(blocked.get() == 0.0)>
                                            "Il n'y en a que " $(q.get()) " en stock."
                                        </span>
                                        <div class="inline-flex items-center overflow-hidden rounded-full ring-1 ring-oat-300">
                                            <button aria-label="Diminuer la quantité"
                                                    :class=$(if q.get() <= 0.0 {
                                                        "flex h-9 w-9 select-none items-center justify-center rounded-l-full text-oat-300"
                                                    } else {
                                                        "flex h-9 w-9 cursor-pointer select-none items-center justify-center rounded-l-full transition hover:bg-oat-100"
                                                    })
                                                    @click=$(async |_e| {
                                                        // Down to zero included: the server retires the
                                                        // line there, and asking again changes nothing.
                                                        let n = set_quantity(line_sku.get(), line_size.get(), q.get() - 1.0).await;
                                                        blocked.set(0.0);
                                                        q.set(n);
                                                        version.increment();
                                                    })>"−"</button>
                                            <span class="w-8 text-center text-sm tabular-nums">$(q.get())</span>
                                            <button aria-label="Augmenter la quantité"
                                                    :class=$(if q.get() >= line_stock.get() {
                                                        "flex h-9 w-9 select-none items-center justify-center rounded-r-full text-oat-300"
                                                    } else {
                                                        "flex h-9 w-9 cursor-pointer select-none items-center justify-center rounded-r-full transition hover:bg-oat-100"
                                                    })
                                                    @click=$(async |_e| {
                                                        // Hidden during the round-trip: the reveal
                                                        // restarts the fade-out animation each time.
                                                        blocked.set(0.0);
                                                        let n = set_quantity(line_sku.get(), line_size.get(), q.get() + 1.0).await;
                                                        blocked.set(if n == q.get() { 1.0 } else { 0.0 });
                                                        q.set(n);
                                                        version.increment();
                                                    })>"+"</button>
                                        </div>
                                    </div>
                                    <button class=("text-sm underline underline-offset-4 transition hover:text-brique-700 ".to_string() + MUTED)
                                            @click=$(async |_e| {
                                                remove(line_sku.get(), line_size.get()).await;
                                                blocked.set(0.0);
                                                q.set(0.0);
                                                version.increment();
                                            })>"Retirer"</button>
                                </div>
                            </div>
                        </li>
                    }
                </ul>

                <aside class="lg:sticky lg:top-24 lg:h-fit">
                    <div class=(CARD.to_string() + " p-6")>
                        <p class=(EYEBROW)>"Récapitulatif"</p>
                        <div class="mt-5">
                            bill(version: $(version.get()), mode: $(standard.get()))
                        </div>

                        if signed_in {
                            <a href="/commander" class=(BTN.to_string() + " mt-6 w-full")>"Passer commande"</a>
                        } else {
                            <a href="/connexion" class=(BTN.to_string() + " mt-6 w-full")>"Se connecter pour commander"</a>
                            <p class=("mt-3 text-center text-xs ".to_string() + MUTED)>"Votre panier vous suivra."</p>
                        }
                        <a href="/boutique" class=(BTN_OUTLINE.to_string() + " mt-3 w-full")>"Continuer mes achats"</a>
                    </div>
                </aside>
            </div>
        }
    }
}

// --- checkout

#[page("/commander")]
async fn checkout(cx: &Cx) -> Result {
    let user = current_user(cx).await?.ok_or_redirect("/connexion")?;
    let id = current_cart(cx);
    let lines = db::cart_lines(pool(cx), &id).await?;
    if lines.is_empty() {
        return view! {
            page_heading(eyebrow: "Commande", title: "Rien à commander", lede: "Votre panier est vide.")
            <a href="/boutique" class=(BTN.to_string() + " mt-8")>"Voir la collection"</a>
        };
    }

    view! {
        signal mode = "standard".to_string();
        signal standard = "standard".to_string();
        signal express = "express".to_string();

        page_heading(
            eyebrow: "Commande",
            title: "Livraison et paiement",
            lede: "Dernière étape. Aucun paiement réel n'est demandé : cette boutique est \
                   une démonstration, et le crabe ne prend pas la carte."
        )

        <form method="post" action="/commander" class="mt-10 grid gap-10 lg:grid-cols-[1.6fr_1fr]">
            <div class="space-y-8">
                <section>
                    <h2 class="text-2xl">"Adresse de livraison"</h2>
                    <p class=("mt-1 text-sm ".to_string() + MUTED)>"Commande au nom de " (&user.name) "."</p>

                    <textarea name="address" required="required" rows="4" class=(FIELD.to_string() + " mt-4")
                              placeholder="12 rue de la Marée&#10;29200 Brest">"12 rue de la Marée\n29200 Brest"</textarea>
                </section>

                <section>
                    <h2 class="text-2xl">"Mode de livraison"</h2>
                    <div class="mt-4 space-y-3">
                        <label :class=$(if mode.get() == standard.get() {
                                   "flex cursor-pointer items-center gap-4 rounded-2xl bg-white p-5 ring-2 ring-gin-600"
                               } else {
                                   "flex cursor-pointer items-center gap-4 rounded-2xl bg-white p-5 ring-1 ring-oat-200"
                               })>
                            <input type="radio" name="shipping" value="standard" checked="checked"
                                   class="h-4 w-4 accent-gin-700"
                                   @change=$(|_e| mode.set(standard.get()))>
                            <span class="flex-1">
                                <span class="block font-medium">"Standard"</span>
                                <span class=("block text-sm ".to_string() + MUTED)>"3 à 5 jours ouvrés"</span>
                            </span>
                            <span class="tabular-nums">"4,90 €"</span>
                        </label>

                        <label :class=$(if mode.get() == express.get() {
                                   "flex cursor-pointer items-center gap-4 rounded-2xl bg-white p-5 ring-2 ring-gin-600"
                               } else {
                                   "flex cursor-pointer items-center gap-4 rounded-2xl bg-white p-5 ring-1 ring-oat-200"
                               })>
                            <input type="radio" name="shipping" value="express"
                                   class="h-4 w-4 accent-gin-700"
                                   @change=$(|_e| mode.set(express.get()))>
                            <span class="flex-1">
                                <span class="block font-medium">"Express"</span>
                                <span class=("block text-sm ".to_string() + MUTED)>"24 à 48 heures"</span>
                            </span>
                            <span class="tabular-nums">"11,90 €"</span>
                        </label>
                    </div>
                    <p class=("mt-3 text-sm ".to_string() + MUTED)>
                        "Au-delà de " (format_price(db::FREE_SHIPPING_CENTS)) ", la livraison est offerte quel que soit le mode."
                    </p>
                </section>

                <section>
                    <h2 class="text-2xl">"Paiement"</h2>
                    <p class=("mt-3 rounded-2xl bg-oat-100 px-5 py-4 text-sm leading-relaxed ".to_string() + SOFT)>
                        "Aucun moyen de paiement n'est demandé : valider enregistre la commande, \
                         décrémente le stock et ouvre son suivi, sans qu'un centime ne circule."
                    </p>
                </section>
            </div>

            <aside class="lg:sticky lg:top-24 lg:h-fit">
                <div class=(CARD.to_string() + " p-6")>
                    <p class=(EYEBROW)>"Votre commande"</p>
                    <div class="mt-5">
                        bill(version: $(0.0), mode: $(mode.get()))
                    </div>
                    <button class=(BTN.to_string() + " mt-6 w-full")>"Valider la commande"</button>
                    <p class=("mt-3 text-center text-xs ".to_string() + MUTED)>"Retour accepté 30 jours."</p>
                </div>
            </aside>
        </form>
    }
}

#[derive(serde::Deserialize)]
struct CheckoutForm {
    address: String,
    #[serde(default)]
    shipping: String,
}

/// A plain form POST: it works without JavaScript, and the redirect
/// afterwards keeps a refresh from ordering twice.
#[route(POST "/commander")]
async fn place_order(cx: &Cx, Form(choice): Form<CheckoutForm>) -> Result<SeeOther> {
    let user = current_user(cx).await?.ok_or_redirect("/connexion")?;
    let id = current_cart(cx);

    let address = choice.address.trim().to_string();

    let Some(reference) =
        db::place_order(pool(cx), user.id, &id, &address, &choice.shipping).await?
    else {
        // Someone else took the last units first: the cart was clamped,
        // the visitor goes back to see what is really left.
        return Ok(see_other("/panier?ajuste=1"));
    };
    forget_cart(cx);
    Ok(see_other(&format!("/commande/{reference}")))
}
