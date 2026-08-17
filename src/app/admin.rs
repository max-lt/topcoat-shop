//! The back office: stock, prices, orders and accounts, behind a flag on
//! the user row. Non-admins get a 404 rather than a login prompt -- the
//! door does not advertise itself.

use topcoat::context::Cx;
use topcoat::router::error::RouterErrorExt;
use topcoat::router::page;
use topcoat::view::{component, view};
use topcoat::Result;

use crate::app::context::{current_admin, pool};
use crate::app::orders::status_badge;
use crate::app::{CARD, EYEBROW, MUTED};
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

        <div class="mt-8 overflow-x-auto">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-oat-300">
                        <th class=(HEAD)>"Produit"</th>
                        <th class=(HEAD)>"Catégorie"</th>
                        <th class=(HEAD)>"Prix"</th>
                        <th class=(HEAD)>"Stock"</th>
                    </tr>
                </thead>
                <tbody>
                    for p in products {
                        <tr class="border-b border-oat-200">
                            <td class=(CELL)>
                                (&p.name)
                                <span class=("ml-2 text-xs ".to_string() + MUTED)>(&p.sku)</span>
                            </td>
                            <td class=(CELL)>(&p.category)</td>
                            <td class=(CELL.to_string() + " tabular-nums")>(format_price(p.price_cents))</td>
                            <td class=(if p.stock == 0 {
                                "py-3 pr-6 tabular-nums text-brique-700"
                            } else {
                                "py-3 pr-6 tabular-nums"
                            })>(p.stock)</td>
                        </tr>
                    }
                </tbody>
            </table>
        </div>
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
