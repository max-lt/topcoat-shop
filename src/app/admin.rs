//! The back office: stock, prices, orders and accounts, behind a flag on
//! the user row. Non-admins get a 404 rather than a login prompt -- the
//! door does not advertise itself.

use topcoat::context::Cx;
use topcoat::router::error::{see_other, RouterErrorExt, SeeOther};
use topcoat::router::content::multipart::Multipart;
use topcoat::router::{content::Form, page, path_param, route};
use topcoat::view::{component, view};
use topcoat::Result;

use crate::app::context::{current_admin, pool};
use crate::app::orders::status_badge;
use crate::app::{BTN, BTN_OUTLINE, CARD, EYEBROW, FIELD, MUTED};
use crate::db::{self, format_price};

const TABS: [(&str, &str); 4] = [
    ("/admin", "Tableau de bord"),
    ("/admin/produits", "Produits"),
    ("/admin/commandes", "Commandes"),
    ("/admin/clients", "Clients"),
];

#[component]
async fn header(active: &str) -> Result {
    view! {
        <p class=(EYEBROW)>"Administration"</p>
        <h1 class="mt-3 text-4xl sm:text-5xl">"La coquille, côté cale"</h1>
        <nav class="mt-8 flex flex-wrap gap-2 border-b border-oat-200 pb-4">
            for (path, label) in TABS {
                if path == active {
                    <a href=(path) class="rounded-full bg-oat-900 px-4 py-1.5 text-sm text-oat-50" aria-current="page">(label)</a>
                } else {
                    <a href=(path) class="rounded-full px-4 py-1.5 text-sm ring-1 ring-oat-300 transition hover:bg-oat-100">(label)</a>
                }
            }
        </nav>
    }
}

#[component]
async fn stat_card(value: String, label: &'static str) -> Result {
    view! {
        <div class=(CARD.to_string() + " p-6")>
            <p class="font-display text-3xl tabular-nums">(value)</p>
            <p class=("mt-1 text-sm ".to_string() + MUTED)>(label)</p>
        </div>
    }
}

const CELL: &str = "py-3 pr-6";
const HEAD: &str = "py-3 pr-6 text-left text-xs font-medium uppercase tracking-widest text-oat-600";

#[page("/admin")]
async fn dashboard(cx: &Cx) -> Result {
    current_admin(cx).await?.ok_or_not_found()?;
    let stats = db::admin_stats(pool(cx)).await?;
    let recent: Vec<_> = db::admin_orders(pool(cx)).await?.into_iter().take(8).collect();

    view! {
        header(active: "/admin")

        <div class="mt-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            stat_card(value: format_price(stats.revenue_cents), label: "Chiffre d'affaires")
            stat_card(value: stats.orders.to_string(), label: "Commandes")
            stat_card(value: stats.customers.to_string(), label: "Comptes")
            stat_card(value: stats.products.to_string(), label: "Références au catalogue")
            stat_card(value: stats.subscribers.to_string(), label: "Abonnés à la lettre")
            stat_card(value: stats.alerts.to_string(), label: "Alertes de retour en stock")
        </div>

        <h2 class="mt-14 text-3xl">"Dernières commandes"</h2>
        <div class="mt-6 overflow-x-auto">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-oat-300">
                        <th class=(HEAD)>"Référence"</th>
                        <th class=(HEAD)>"Client"</th>
                        <th class=(HEAD)>"Statut"</th>
                        <th class=(HEAD)>"Date"</th>
                        <th class=(HEAD)>"Total"</th>
                    </tr>
                </thead>
                <tbody>
                    for o in recent {
                        <tr class="border-b border-oat-200">
                            <td class=(CELL.to_string() + " tabular-nums")>(&o.reference)</td>
                            <td class=(CELL)>(&o.customer)</td>
                            <td class=(CELL)>status_badge(status: o.status.clone())</td>
                            <td class=(CELL.to_string() + " tabular-nums")>(o.created_at.get(..10).unwrap_or_default().to_string())</td>
                            <td class=(CELL.to_string() + " tabular-nums")>(format_price(o.total_cents))</td>
                        </tr>
                    }
                </tbody>
            </table>
        </div>
    }
}

#[page("/admin/produits")]
async fn products(cx: &Cx) -> Result {
    current_admin(cx).await?.ok_or_not_found()?;
    let products = db::all_products(pool(cx)).await?;

    view! {
        header(active: "/admin/produits")

        <p class="mt-6">
            <a href="/admin/nouveau" class=(BTN)>"Nouveau produit"</a>
        </p>

        <div class="mt-4 overflow-x-auto">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-oat-300">
                        <th class=(HEAD)>"Produit"</th>
                        <th class=(HEAD)>"Catégorie"</th>
                        <th class=(HEAD)>"Prix"</th>
                        <th class=(HEAD)>"Stock"</th>
                        <th class=(HEAD)></th>
                    </tr>
                </thead>
                <tbody>
                    for p in products {
                        <tr class=(if p.hidden != 0 { "border-b border-oat-200 opacity-50" } else { "border-b border-oat-200" })>
                            <td class=(CELL)>
                                (&p.name)
                                <span class=("ml-2 text-xs ".to_string() + MUTED)>(&p.sku)</span>
                                if p.hidden != 0 {
                                    <span class="ml-2 rounded-full bg-oat-200 px-2 py-0.5 text-xs text-oat-700">"Masqué"</span>
                                }
                            </td>
                            <td class=(CELL)>(&p.category)</td>
                            <td class=(CELL.to_string() + " tabular-nums")>(format_price(p.price_cents))</td>
                            <td class=(if p.stock == 0 {
                                "py-3 pr-6 tabular-nums text-brique-700"
                            } else {
                                "py-3 pr-6 tabular-nums"
                            })>(p.stock)</td>
                            <td class=(CELL)>
                                <div class="flex items-center gap-4">
                                    <a href=("/admin/produit/".to_string() + &p.sku)
                                       class="text-gin-700 underline underline-offset-4">"Modifier"</a>
                                    <form method="post" action="/admin/masquer">
                                        <input type="hidden" name="sku" value=(&p.sku)>
                                        <button class=("underline underline-offset-4 transition hover:text-gin-700 ".to_string() + MUTED)>
                                            if p.hidden != 0 { "Remettre en boutique" } else { "Masquer" }
                                        </button>
                                    </form>
                                </div>
                            </td>
                        </tr>
                    }
                </tbody>
            </table>
        </div>
    }
}

path_param!(sku);

#[page("/admin/produit/{sku}")]
async fn product(cx: &Cx) -> Result {
    current_admin(cx).await?.ok_or_not_found()?;
    let sku = path_param::<Sku>(cx).to_string();
    let p = db::product(pool(cx), &sku).await?.ok_or_not_found()?;
    let variants = db::variants(pool(cx), &sku).await?;
    let categories = db::categories(pool(cx)).await?;
    let price_text = format!("{},{:02}", p.price_cents / 100, p.price_cents % 100);
    let is_new = p.is_new != 0;

    view! {
        header(active: "/admin/produits")

        <div class="mt-8 flex items-center gap-5">
            <div class="h-20 w-20 overflow-hidden rounded-2xl bg-oat-100 ring-1 ring-oat-200">
                <img src=(crate::images::url(&p.sku, 400)) alt="" class="h-full w-full object-cover">
            </div>
            <div>
                <h2 class="text-3xl">(&p.name)</h2>
                <p class=("mt-1 text-sm ".to_string() + MUTED)>
                    (&p.sku) " — "
                    <a href=("/produit/".to_string() + &p.sku) class="underline underline-offset-4">"voir la fiche publique"</a>
                </p>
                <form method="post" action="/admin/masquer" class="mt-2">
                    <input type="hidden" name="sku" value=(&p.sku)>
                    <button class=("text-sm underline underline-offset-4 transition hover:text-gin-700 ".to_string() + MUTED)>
                        if p.hidden != 0 { "Masqué — remettre en boutique" } else { "Visible — masquer de la boutique" }
                    </button>
                </form>
            </div>
        </div>

        <div class="mt-10 grid gap-8 lg:grid-cols-2">
            <section class=(CARD.to_string() + " p-6")>
                <h3 class="text-xl">"Fiche"</h3>
                <form method="post" action="/admin/fiche" class="mt-4 space-y-4">
                    <input type="hidden" name="sku" value=(&p.sku)>
                    <div>
                        <label class="text-sm font-medium">"Nom"</label>
                        <input class=(FIELD.to_string() + " mt-1") name="name" required="required" value=(&p.name)>
                    </div>
                    <div class="grid gap-4 sm:grid-cols-2">
                        <div>
                            <label class="text-sm font-medium">"Catégorie"</label>
                            <input class=(FIELD.to_string() + " mt-1") name="category" required="required" value=(&p.category) list="categories">
                            <datalist id="categories">
                                for c in categories {
                                    <option value=(&c)></option>
                                }
                            </datalist>
                        </div>
                        <div>
                            <label class="text-sm font-medium">"Matière"</label>
                            <input class=(FIELD.to_string() + " mt-1") name="material" value=(&p.material)>
                        </div>
                    </div>
                    <div>
                        <label class="text-sm font-medium">"Résumé"</label>
                        <textarea class=(FIELD.to_string() + " mt-1") name="summary" rows="2" required="required">(&p.summary)</textarea>
                    </div>
                    <div>
                        <label class="text-sm font-medium">"Description"</label>
                        <textarea class=(FIELD.to_string() + " mt-1") name="detail" rows="4" required="required">(&p.detail)</textarea>
                    </div>
                    <label class=("flex items-center gap-2 text-sm ".to_string() + MUTED)>
                        if is_new {
                            <input type="checkbox" name="is_new" value="1" checked="checked" class="h-4 w-4 accent-gin-700">
                        } else {
                            <input type="checkbox" name="is_new" value="1" class="h-4 w-4 accent-gin-700">
                        }
                        "Afficher le badge Nouveau"
                    </label>
                    <button class=(BTN)>"Enregistrer la fiche"</button>
                </form>
            </section>

            <div class="space-y-8">
                <section class=(CARD.to_string() + " p-6")>
                    <h3 class="text-xl">"Prix"</h3>
                    <form method="post" action="/admin/prix" class="mt-4 flex items-center gap-3">
                        <input type="hidden" name="sku" value=(&p.sku)>
                        <input name="price" value=(price_text) inputmode="decimal"
                               class="w-28 rounded-xl bg-oat-50 px-3 py-2 text-right tabular-nums ring-1 ring-oat-300" aria-label="Prix en euros">
                        <span class=(MUTED)>"€"</span>
                        <button class=(BTN)>"Enregistrer"</button>
                    </form>
                </section>

                <section class=(CARD.to_string() + " p-6")>
                    <h3 class="text-xl">"Stock"</h3>
                    <div class="mt-4 space-y-3">
                        for v in variants {
                            <div class="flex items-center gap-3">
                                <form method="post" action="/admin/stock" class="flex items-center gap-3">
                                    <input type="hidden" name="sku" value=(&p.sku)>
                                    <input type="hidden" name="size" value=(&v.size)>
                                    <span class="w-24 text-sm">
                                        if v.size.is_empty() { "Taille unique" } else { (&v.size) }
                                    </span>
                                    <input name="stock" type="number" min="0" value=(v.stock)
                                           class="w-24 rounded-xl bg-oat-50 px-3 py-2 text-right tabular-nums ring-1 ring-oat-300" aria-label="Stock">
                                    <button class=(BTN)>"Enregistrer"</button>
                                </form>
                                <form method="post" action="/admin/variante/retirer">
                                    <input type="hidden" name="sku" value=(&p.sku)>
                                    <input type="hidden" name="size" value=(&v.size)>
                                    <button class=("text-sm underline underline-offset-4 transition hover:text-brique-700 ".to_string() + MUTED)>"Retirer"</button>
                                </form>
                            </div>
                        }
                    </div>
                    <form method="post" action="/admin/variante" class="mt-5 flex items-center gap-3 border-t border-oat-200 pt-5">
                        <input type="hidden" name="sku" value=(&p.sku)>
                        <input name="size" placeholder="Taille (S, 42…)"
                               class="w-32 rounded-xl bg-oat-50 px-3 py-2 text-sm ring-1 ring-oat-300" aria-label="Nouvelle taille">
                        <input name="stock" type="number" min="0" value="0"
                               class="w-24 rounded-xl bg-oat-50 px-3 py-2 text-right tabular-nums ring-1 ring-oat-300" aria-label="Stock initial">
                        <button class=(BTN_OUTLINE)>"Ajouter"</button>
                    </form>
                    <p class=("mt-4 text-xs ".to_string() + MUTED)>
                        "Le stock produit est la somme des variantes ; il se recalcule seul."
                    </p>
                </section>

                <section class=(CARD.to_string() + " p-6")>
                    <h3 class="text-xl">"Photo"</h3>
                    photo_card(sku: p.sku.clone())
                </section>
            </div>
        </div>

        <p class="mt-8">
            <a href="/admin/produits" class="text-sm text-gin-700 underline underline-offset-4">"← Tous les produits"</a>
        </p>
    }
}

#[page("/admin/nouveau")]
async fn new_product(cx: &Cx) -> Result {
    current_admin(cx).await?.ok_or_not_found()?;
    let categories = db::categories(pool(cx)).await?;

    view! {
        header(active: "/admin/produits")

        <h2 class="mt-8 text-3xl">"Nouveau produit"</h2>
        <form method="post" action="/admin/creer" class=(CARD.to_string() + " mt-6 max-w-2xl space-y-4 p-6")>
            <div class="grid gap-4 sm:grid-cols-2">
                <div>
                    <label class="text-sm font-medium">"Référence (SKU)"</label>
                    <input class=(FIELD.to_string() + " mt-1") name="sku" required="required" placeholder="COQ-GILET">
                </div>
                <div>
                    <label class="text-sm font-medium">"Nom"</label>
                    <input class=(FIELD.to_string() + " mt-1") name="name" required="required" placeholder="Gilet de quart">
                </div>
            </div>
            <div class="grid gap-4 sm:grid-cols-3">
                <div>
                    <label class="text-sm font-medium">"Catégorie"</label>
                    <input class=(FIELD.to_string() + " mt-1") name="category" required="required" list="categories">
                    <datalist id="categories">
                        for c in categories {
                            <option value=(&c)></option>
                        }
                    </datalist>
                </div>
                <div>
                    <label class="text-sm font-medium">"Prix (€)"</label>
                    <input class=(FIELD.to_string() + " mt-1") name="price" required="required" inputmode="decimal" placeholder="49,00">
                </div>
                <div>
                    <label class="text-sm font-medium">"Stock initial"</label>
                    <input class=(FIELD.to_string() + " mt-1") name="stock" type="number" min="0" value="0">
                </div>
            </div>
            <div>
                <label class="text-sm font-medium">"Matière"</label>
                <input class=(FIELD.to_string() + " mt-1") name="material" placeholder="Laine bouillie, boutons corozo">
            </div>
            <div>
                <label class="text-sm font-medium">"Résumé"</label>
                <textarea class=(FIELD.to_string() + " mt-1") name="summary" rows="2" required="required"></textarea>
            </div>
            <div>
                <label class="text-sm font-medium">"Description"</label>
                <textarea class=(FIELD.to_string() + " mt-1") name="detail" rows="4" required="required"></textarea>
            </div>
            <label class=("flex items-center gap-2 text-sm ".to_string() + MUTED)>
                <input type="checkbox" name="is_new" value="1" checked="checked" class="h-4 w-4 accent-gin-700">
                "Afficher le badge Nouveau"
            </label>
            <button class=(BTN)>"Créer le produit"</button>
            <p class=("text-xs ".to_string() + MUTED)>"La photo se téléverse à l'étape suivante, sur la fiche."</p>
        </form>
    }
}

#[page("/admin/commandes")]
async fn orders(cx: &Cx) -> Result {
    current_admin(cx).await?.ok_or_not_found()?;
    let orders = db::admin_orders(pool(cx)).await?;

    view! {
        header(active: "/admin/commandes")

        <div class="mt-8 overflow-x-auto">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-oat-300">
                        <th class=(HEAD)>"Référence"</th>
                        <th class=(HEAD)>"Client"</th>
                        <th class=(HEAD)>"Email"</th>
                        <th class=(HEAD)>"Statut"</th>
                        <th class=(HEAD)>"Date"</th>
                        <th class=(HEAD)>"Total"</th>
                    </tr>
                </thead>
                <tbody>
                    for o in orders {
                        <tr class="border-b border-oat-200">
                            <td class=(CELL.to_string() + " tabular-nums")>(&o.reference)</td>
                            <td class=(CELL)>(&o.customer)</td>
                            <td class=(CELL)>(&o.email)</td>
                            <td class=(CELL)>status_badge(status: o.status.clone())</td>
                            <td class=(CELL.to_string() + " tabular-nums")>(o.created_at.get(..10).unwrap_or_default().to_string())</td>
                            <td class=(CELL.to_string() + " tabular-nums")>(format_price(o.total_cents))</td>
                        </tr>
                    }
                </tbody>
            </table>
        </div>
    }
}

#[page("/admin/clients")]
async fn customers(cx: &Cx) -> Result {
    current_admin(cx).await?.ok_or_not_found()?;
    let customers = db::admin_customers(pool(cx)).await?;

    view! {
        header(active: "/admin/clients")

        <div class="mt-8 overflow-x-auto">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-oat-300">
                        <th class=(HEAD)>"Nom"</th>
                        <th class=(HEAD)>"Email"</th>
                        <th class=(HEAD)>"Commandes"</th>
                        <th class=(HEAD)>"Total dépensé"</th>
                        <th class=(HEAD)></th>
                    </tr>
                </thead>
                <tbody>
                    for c in customers {
                        <tr class="border-b border-oat-200">
                            <td class=(CELL)>(&c.name)</td>
                            <td class=(CELL)>(&c.email)</td>
                            <td class=(CELL.to_string() + " tabular-nums")>(c.orders)</td>
                            <td class=(CELL.to_string() + " tabular-nums")>(format_price(c.total_cents))</td>
                            <td class=(CELL)>
                                if c.admin != 0 {
                                    <span class="rounded-full bg-gin-100 px-2.5 py-0.5 text-xs font-medium text-gin-800">"Admin"</span>
                                }
                            </td>
                        </tr>
                    }
                </tbody>
            </table>
        </div>
    }
}

// --- actions

/// "24,00", "24.00" or "24" -- anything a shopkeeper would type.
fn cents(text: &str) -> Option<i64> {
    let clean = text.trim().replace('€', "").replace(['\u{a0}', '\u{202f}', ' '], "").replace(',', ".");
    let value: f64 = clean.parse().ok()?;
    if !(0.0..=100_000.0).contains(&value) {
        return None;
    }
    Some((value * 100.0).round() as i64)
}

#[derive(serde::Deserialize)]
struct PriceUpdate {
    sku: String,
    price: String,
}

#[route(POST "/admin/prix")]
async fn update_price(cx: &Cx, Form(f): Form<PriceUpdate>) -> Result<SeeOther> {
    current_admin(cx).await?.ok_or_not_found()?;
    if let Some(price_cents) = cents(&f.price) {
        db::set_price(pool(cx), &f.sku, price_cents).await?;
    }
    Ok(see_other(format!("/admin/produit/{}", f.sku)))
}

#[derive(serde::Deserialize)]
struct StockUpdate {
    sku: String,
    #[serde(default)]
    size: String,
    stock: i64,
}

#[route(POST "/admin/stock")]
async fn update_stock(cx: &Cx, Form(f): Form<StockUpdate>) -> Result<SeeOther> {
    current_admin(cx).await?.ok_or_not_found()?;
    db::set_stock(pool(cx), &f.sku, &f.size, f.stock).await?;
    Ok(see_other(format!("/admin/produit/{}", f.sku)))
}

#[derive(serde::Deserialize)]
struct ProductUpdate {
    sku: String,
    name: String,
    summary: String,
    detail: String,
    category: String,
    #[serde(default)]
    material: String,
    #[serde(default)]
    is_new: String,
}

#[route(POST "/admin/fiche")]
async fn update_product(cx: &Cx, Form(f): Form<ProductUpdate>) -> Result<SeeOther> {
    current_admin(cx).await?.ok_or_not_found()?;
    db::update_product(
        pool(cx),
        &f.sku,
        &f.name,
        &f.summary,
        &f.detail,
        &f.category,
        &f.material,
        i64::from(f.is_new == "1"),
    )
    .await?;
    Ok(see_other(format!("/admin/produit/{}", f.sku)))
}

#[derive(serde::Deserialize)]
struct NewVariant {
    sku: String,
    size: String,
    stock: i64,
}

#[route(POST "/admin/variante")]
async fn add_variant(cx: &Cx, Form(f): Form<NewVariant>) -> Result<SeeOther> {
    current_admin(cx).await?.ok_or_not_found()?;
    if !f.size.trim().is_empty() {
        db::add_variant(pool(cx), &f.sku, &f.size, f.stock).await?;
    }
    Ok(see_other(format!("/admin/produit/{}", f.sku)))
}

#[derive(serde::Deserialize)]
struct VariantTarget {
    sku: String,
    #[serde(default)]
    size: String,
}

#[route(POST "/admin/variante/retirer")]
async fn remove_variant(cx: &Cx, Form(f): Form<VariantTarget>) -> Result<SeeOther> {
    current_admin(cx).await?.ok_or_not_found()?;
    db::remove_variant(pool(cx), &f.sku, &f.size).await?;
    Ok(see_other(format!("/admin/produit/{}", f.sku)))
}

#[component]
async fn photo_card(sku: String) -> Result {
    view! {
        <form method="post" action="/admin/photo" enctype="multipart/form-data" class="mt-4 flex flex-wrap items-center gap-3">
            <input type="hidden" name="sku" value=(&sku)>
            <input type="file" name="file" accept="image/*" required="required"
                   class="text-sm" aria-label="Nouvelle photo">
            <button class=(BTN)>"Téléverser"</button>
        </form>
        <p class=("mt-3 text-xs ".to_string() + MUTED)>
            "JPEG ou PNG ; recadrée en JPEG, 1600 px de large au plus, servie par /img comme les autres."
        </p>
    }
}

/// A phone shot passes the router's 2 MiB default without trying, and
/// would come back a 413.
pub const PHOTO_LIMIT: usize = 16 * 1024 * 1024;

/// The one multipart route of the shop: a photo comes in whatever the
/// admin has on disk, and leaves under its SKU in whichever store the
/// host keeps -- a directory natively, a bucket at the edge.
#[route(POST "/admin/photo")]
async fn upload_photo(cx: &Cx, mut body: Multipart) -> Result<SeeOther> {
    current_admin(cx).await?.ok_or_not_found()?;

    let mut sku = String::new();
    let mut file: Vec<u8> = Vec::new();
    while let Some(field) = body.next_field().await? {
        let name = field.name().map(str::to_string);
        match name.as_deref() {
            Some("sku") => sku = String::from_utf8_lossy(&field.bytes().await?).into_owned(),
            Some("file") => file = field.bytes().await?.to_vec(),
            _ => {}
        }
    }
    db::product(pool(cx), &sku).await?.ok_or_not_found()?;

    if !file.is_empty() {
        crate::images::store(&sku, file).await?;
    }
    Ok(see_other(format!("/admin/produit/{sku}")))
}

#[derive(serde::Deserialize)]
struct NewProduct {
    sku: String,
    name: String,
    category: String,
    price: String,
    #[serde(default)]
    stock: i64,
    #[serde(default)]
    material: String,
    summary: String,
    detail: String,
    #[serde(default)]
    is_new: String,
}

#[route(POST "/admin/creer")]
async fn create_product(cx: &Cx, Form(f): Form<NewProduct>) -> Result<SeeOther> {
    current_admin(cx).await?.ok_or_not_found()?;
    let sku = f.sku.trim().to_uppercase().replace(' ', "-");
    if sku.is_empty() || db::product(pool(cx), &sku).await?.is_some() {
        return Ok(see_other("/admin/nouveau"));
    }
    let price_cents = cents(&f.price).unwrap_or(0);
    db::create_product(
        pool(cx),
        &sku,
        &f.name,
        &f.summary,
        &f.detail,
        price_cents,
        &f.category,
        &f.material,
        i64::from(f.is_new == "1"),
        f.stock,
    )
    .await?;
    Ok(see_other(format!("/admin/produit/{sku}")))
}

#[derive(serde::Deserialize)]
struct ProductTarget {
    sku: String,
}

#[route(POST "/admin/masquer")]
async fn toggle_hidden(cx: &Cx, Form(f): Form<ProductTarget>) -> Result<SeeOther> {
    current_admin(cx).await?.ok_or_not_found()?;
    db::toggle_hidden(pool(cx), &f.sku).await?;
    Ok(see_other("/admin/produits"))
}
