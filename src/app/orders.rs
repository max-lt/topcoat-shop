//! Order confirmation and tracking. The timeline comes from the `tracking`
//! table; a procedure walks the order to its next step so the page has
//! something to show without a real warehouse behind it.

use topcoat::context::Cx;
use topcoat::router::error::{see_other, RouterErrorExt, SeeOther};
use topcoat::router::{page, path_param, route};
use topcoat::runtime::{procedure, shard};
use topcoat::view::{component, view};
use topcoat::Result;

use crate::app::context::{current_user, pool};
use crate::app::{BTN_OUTLINE, CARD, EYEBROW, MUTED, SOFT};
use crate::db::{self, format_price};

const STEPS: [(&str, &str); 4] = [
    ("paid", "Payée"),
    ("packing", "En préparation"),
    ("shipped", "Expédiée"),
    ("delivered", "Livrée"),
];

/// The one place an order status becomes a colour and a word.
#[component]
pub async fn status_badge(status: String) -> Result {
    let (label, classes) = match status.as_str() {
        "paid" => ("Payée", "bg-oat-200 text-oat-800"),
        "packing" => ("En préparation", "bg-gin-100 text-gin-800"),
        "shipped" => ("Expédiée", "bg-gin-200 text-gin-900"),
        "cancelled" => ("Annulée", "bg-brique-100 text-brique-700"),
        _ => ("Livrée", "bg-gin-700 text-oat-50"),
    };
    view! {
        <span class=("rounded-full px-3 py-1 text-xs font-medium ".to_string() + classes)>(label)</span>
    }
}

/// The ladder as a rung number: paid, packing, shipped, delivered, and
/// cancelled off the end. The buttons compare rungs rather than parse
/// words.
fn rung(status: &str) -> f64 {
    match status {
        "paid" => 0.0,
        "packing" => 1.0,
        "shipped" => 2.0,
        "delivered" => 3.0,
        _ => 4.0,
    }
}

#[procedure]
async fn advance(cx: &Cx, reference: String) -> Result<f64> {
    let user = current_user(cx).await?.ok_or_unauthorized()?;
    Ok(rung(&db::advance_order(pool(cx), user.id, &reference).await?))
}

#[shard]
async fn tracking(cx: &Cx, reference: String, version: f64) -> Result {
    let _ = version;
    let user = current_user(cx).await?.ok_or_unauthorized()?;
    let (order, _, steps) =
        db::order(pool(cx), user.id, &reference).await?.ok_or_not_found()?;
    let reached: Vec<&str> = steps.iter().map(|s| s.step.as_str()).collect();

    view! {
        <div class="flex items-center gap-3">
            <span class="text-sm font-medium">"Statut"</span>
            status_badge(status: order.status.clone())
        </div>

        <ol class="mt-8 space-y-8 pl-8">
            for (rank, (key, label)) in STEPS.iter().copied().enumerate() {
                <li class="relative">
                    // The thread runs from each dot to the next and stops at
                    // the last one: a timeline, not a plumb line. Its height
                    // covers the item plus the space-y gap after it.
                    if rank + 1 < STEPS.len() {
                        <span class="absolute -left-[31.5px] top-2 h-[calc(100%+2rem)] w-px bg-oat-200"></span>
                    }
                    <span class=(if reached.contains(&key) {
                        "absolute -left-9 top-2 h-2.5 w-2.5 rounded-full bg-gin-700 ring-4 ring-oat-50"
                    } else {
                        "absolute -left-9 top-2 h-2.5 w-2.5 rounded-full bg-oat-300 ring-4 ring-oat-50"
                    })></span>
                    <p class=(if reached.contains(&key) { "font-medium" } else { "font-medium text-oat-400" })>(label)</p>
                    for s in steps.iter().filter(|s| s.step == key) {
                        <p class=("mt-1 text-sm ".to_string() + SOFT)>(&s.note)</p>
                        <time class=("text-xs ".to_string() + MUTED)>(s.at.get(..16).unwrap_or_default().replace('T', " à "))</time>
                    }
                </li>
            }
        </ol>

        for s in steps.iter().filter(|s| s.step == "cancelled") {
            <div class="mt-8 rounded-2xl bg-brique-100 px-5 py-4">
                <p class="text-sm font-medium text-brique-700">"Commande annulée"</p>
                <p class="mt-1 text-sm text-brique-700">(&s.note)</p>
                <time class="mt-1 block text-xs text-brique-500">(s.at.get(..16).unwrap_or_default().replace('T', " à "))</time>
            </div>
        }
    }
}

path_param!(reference);

#[page("/commande/{reference}")]
async fn order_page(cx: &Cx) -> Result {
    let reference = path_param::<Reference>(cx).to_string();
    let user = current_user(cx).await?.ok_or_redirect("/connexion")?;
    let (order, lines, _) =
        db::order(pool(cx), user.id, &reference).await?.ok_or_not_found()?;

    let page_reference = order.reference.clone();
    let at = rung(&order.status);
    // A shipped parcel still has a step to walk, though it can no longer
    // be cancelled.
    let can_advance = at < 3.0;
    let cancellable = at < 2.0;
    let mode = db::shipping_mode(&order.shipping);
    let subtotal = order.total_cents - order.shipping_cents;
    let free = order.shipping_cents == 0;

    view! {
        signal reference_sig = page_reference;
        signal version = 0.0;
        signal step = at;

        <div class="relative overflow-hidden rounded-3xl bg-gin-900 px-8 py-16 text-center text-gin-50">
            // The couriers on the march, blurred behind a green veil: the
            // banner keeps its original compact height.
            <img src=(crate::images::url("commande-crabes", 900))
                 alt=""
                 aria-hidden="true"
                 class="absolute inset-0 h-full w-full scale-110 object-cover blur-[3px]">
            <div class="absolute inset-0 bg-gin-900/70"></div>
            <div class="relative">
                <h1 class="text-4xl text-gin-50">"Merci !"</h1>
                <p class="mt-3 text-gin-100">
                    "Votre commande " <span class="tabular-nums">(&order.reference)</span>
                    " est enregistrée. Rien ne part vraiment — mais tout est suivi."
                </p>
            </div>
        </div>

        <div class="mt-12 grid gap-10 lg:grid-cols-[1.6fr_1fr]">
            <div class=(CARD.to_string() + " p-8")>
                <p class=(EYEBROW)>"Suivi"</p>
                <div class="mt-6">
                    tracking(reference: $(reference_sig.get()), version: $(version.get()))
                </div>

                if can_advance {
                <button class=(BTN_OUTLINE.to_string() + " mt-8")
                        :hidden=$(step.get() >= 3.0)
                        @click=$(async |_e| {
                            step.set(advance(reference_sig.get()).await);
                            version.increment();
                        })>"Faire avancer le colis"</button>
                }

                if cancellable {
                    // Cancelling stops at packing: once the parcel moves, the
                    // form goes with it.
                    <form method="post" action=(format!("/commande/{}/annuler", order.reference)) class="mt-4"
                          :hidden=$(step.get() >= 2.0)>
                        <button class=("text-sm underline underline-offset-4 transition hover:text-brique-700 ".to_string() + MUTED)>
                            "Annuler la commande"
                        </button>
                    </form>
                }
            </div>

            <aside class="space-y-6">
                <div class=(CARD.to_string() + " p-6")>
                    <p class=(EYEBROW)>"Articles"</p>
                    <ul class="mt-5 space-y-3 text-sm">
                        for l in lines {
                            <li class="flex items-baseline justify-between gap-3">
                                <span class=(MUTED)>
                                    (&l.name)
                                    if !l.size.is_empty() { " · " (&l.size) }
                                    (format!(" × {}", l.quantity))
                                </span>
                                <span class="tabular-nums">(format_price(l.price_cents * l.quantity))</span>
                            </li>
                        }
                    </ul>
                    <dl class="mt-5 space-y-2 border-t border-oat-200 pt-5 text-sm">
                        <div class="flex justify-between">
                            <dt class=(MUTED)>"Sous-total"</dt>
                            <dd class="tabular-nums">(format_price(subtotal))</dd>
                        </div>
                        <div class="flex justify-between">
                            <dt class=(MUTED)>(mode.name)</dt>
                            <dd class="tabular-nums">
                                if free { <span class="text-gin-700">"offerte"</span> } else { (format_price(order.shipping_cents)) }
                            </dd>
                        </div>
                    </dl>
                    <div class="mt-5 flex items-baseline justify-between border-t border-oat-200 pt-5">
                        <span class="font-medium">"Total"</span>
                        <span class="font-display text-2xl tabular-nums">(format_price(order.total_cents))</span>
                    </div>
                </div>

                <div class=(CARD.to_string() + " p-6")>
                    <p class=(EYEBROW)>"Livraison"</p>
                    <p class=("mt-4 whitespace-pre-line text-sm leading-relaxed ".to_string() + SOFT)>(&order.address)</p>
                    <p class=("mt-4 text-sm ".to_string() + MUTED)>(mode.name) " — " (mode.delay)</p>
                </div>

                <a href="/compte" class=(BTN_OUTLINE.to_string() + " w-full")>"Toutes mes commandes"</a>
            </aside>
        </div>
    }
}

/// A plain POST with a redirect: the page reloads on the cancelled state.
#[route(POST "/commande/{reference}/annuler")]
async fn cancel(cx: &Cx) -> Result<SeeOther> {
    let user = current_user(cx).await?.ok_or_redirect("/connexion")?;
    let reference = path_param::<Reference>(cx).to_string();
    db::cancel_order(pool(cx), user.id, &reference).await?;
    Ok(see_other(format!("/commande/{reference}")))
}
