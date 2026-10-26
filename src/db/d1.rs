//! The d1 backend: the same functions as sqlite.rs, same SQL dialect,
//! spoken to D1 through the Worker bindings. Each request carries its
//! handle in the request context, where `pool(cx)` finds it.

use std::sync::Arc;

use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use worker::wasm_bindgen::JsValue;
use worker::D1Database;

use super::*;

/// The stand-in for the sqlite backend's `&SqlitePool`: call sites keep
/// their shape, and take it by reference as they take the pool.
#[derive(Clone)]
pub struct Db(Arc<D1Database>);

impl Db {
    pub fn new(handle: D1Database) -> Self {
        Self(Arc::new(handle))
    }
}

fn err(e: worker::Error) -> Error {
    Error::msg(format!("D1: {e}"))
}

fn s(text: &str) -> JsValue {
    JsValue::from_str(text)
}

fn n(value: i64) -> JsValue {
    JsValue::from_f64(value as f64)
}

/// The Send bridge: D1's futures are JS promises and cannot cross threads;
/// topcoat's handlers must be Send. wasm has exactly one thread, so the
/// promise runs in a spawn_local task and the handler awaits a oneshot
/// receiver -- which is Send, and carries plain data.
fn bridge<T, F>(work: F) -> impl std::future::Future<Output = Result<T, Error>> + Send
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, Error>> + 'static,
{
    let (tx, rx) = futures_channel::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tx.send(work.await);
    });
    async move { rx.await.map_err(|_| Error::msg("bridge closed"))? }
}

fn fetch_all<T: DeserializeOwned + Send + 'static>(
    db: &Db,
    sql: &str,
    args: &[JsValue],
) -> impl std::future::Future<Output = Result<Vec<T>, Error>> + Send {
    let handle = db.0.clone();
    let sql = sql.to_string();
    let args = args.to_vec();
    bridge(async move {
        handle.prepare(&sql)
            .bind(&args)
            .map_err(err)?
            .all()
            .await
            .map_err(err)?
            .results::<T>()
            .map_err(err)
    })
}

fn fetch_first<T: DeserializeOwned + Send + 'static>(
    db: &Db,
    sql: &str,
    args: &[JsValue],
) -> impl std::future::Future<Output = Result<Option<T>, Error>> + Send {
    let handle = db.0.clone();
    let sql = sql.to_string();
    let args = args.to_vec();
    bridge(async move {
        handle.prepare(&sql).bind(&args).map_err(err)?.first::<T>(None).await.map_err(err)
    })
}

/// Runs a statement and reports how many rows it changed.
fn execute(
    db: &Db,
    sql: &str,
    args: &[JsValue],
) -> impl std::future::Future<Output = Result<u64, Error>> + Send {
    let handle = db.0.clone();
    let sql = sql.to_string();
    let args = args.to_vec();
    bridge(async move {
        let out = handle.prepare(&sql).bind(&args).map_err(err)?.run().await.map_err(err)?;
        let meta = out.meta().map_err(err)?;
        Ok(meta.and_then(|m| m.changes).unwrap_or(0) as u64)
    })
}

async fn scalar(db: &Db, sql: &str, args: &[JsValue]) -> Result<i64, Error> {
    #[derive(Deserialize)]
    struct One {
        value: f64,
    }
    Ok(fetch_first::<One>(db, &format!("select ({sql}) as value"), args)
        .await?
        .map(|o| o.value as i64)
        .unwrap_or(0))
}

// --- catalog

pub async fn catalog(db: &Db, category: &str, sort: i64) -> Result<Vec<Product>, Error> {
    let order = match sort {
        1 => "price_cents asc, name",
        2 => "price_cents desc, name",
        3 => "name",
        _ => "is_new desc, category, name",
    };
    let everything = category.is_empty() || category == "Tout";
    if everything {
        fetch_all(
            db,
            &format!("select {PRODUCT_FIELDS} from products where hidden = 0 order by {order}"),
            &[],
        )
        .await
    } else {
        fetch_all(
            db,
            &format!(
                "select {PRODUCT_FIELDS} from products where hidden = 0 and category = ?1 \
                 order by {order}"
            ),
            &[s(category)],
        )
        .await
    }
}

pub async fn search(db: &Db, term: &str) -> Result<Vec<Product>, Error> {
    let pattern = format!("%{}%", term.trim().to_lowercase());
    fetch_all(
        db,
        &format!(
            "select {PRODUCT_FIELDS} from products \
             where hidden = 0 and (lower(name) like ?1 or lower(summary) like ?1 \
                or lower(category) like ?1 or lower(material) like ?1) \
             order by is_new desc, name"
        ),
        &[s(&pattern)],
    )
    .await
}

pub async fn categories(db: &Db) -> Result<Vec<String>, Error> {
    #[derive(Deserialize)]
    struct Row {
        category: String,
    }
    Ok(fetch_all::<Row>(db, "select distinct category from products order by category", &[])
        .await?
        .into_iter()
        .map(|r| r.category)
        .collect())
}

pub async fn product(db: &Db, sku: &str) -> Result<Option<Product>, Error> {
    fetch_first(db, &format!("select {PRODUCT_FIELDS} from products where sku = ?1"), &[s(sku)]).await
}

/// The products named, in the order they were named, skipping any that no
/// longer exist. One query rather than one per sku.
pub async fn products(db: &Db, skus: &[String]) -> Result<Vec<Product>, Error> {
    if skus.is_empty() {
        return Ok(Vec::new());
    }
    let holes = vec!["?"; skus.len()].join(", ");
    let binds: Vec<JsValue> = skus.iter().map(|sku| s(sku)).collect();
    let found: Vec<Product> = fetch_all(
        db,
        &format!("select {PRODUCT_FIELDS} from products where sku in ({holes})"),
        &binds,
    )
    .await?;
    Ok(skus.iter().filter_map(|sku| found.iter().find(|p| &p.sku == sku).cloned()).collect())
}

pub async fn related_products(db: &Db, sku: &str, category: &str) -> Result<Vec<Product>, Error> {
    fetch_all(
        db,
        &format!(
            "select {PRODUCT_FIELDS} from products where hidden = 0 and sku <> ?1 \
             order by case when category = ?2 then 0 else 1 end, is_new desc, name limit 3"
        ),
        &[s(sku), s(category)],
    )
    .await
}

pub async fn new_arrivals(db: &Db, how_many: i64) -> Result<Vec<Product>, Error> {
    fetch_all(
        db,
        &format!(
            "select {PRODUCT_FIELDS} from products where hidden = 0 \
             order by is_new desc, stock desc limit ?1"
        ),
        &[n(how_many)],
    )
    .await
}

pub async fn variants(db: &Db, sku: &str) -> Result<Vec<Variant>, Error> {
    fetch_all(db, "select size, stock from variants where sku = ?1 order by rank, size", &[s(sku)])
        .await
}

async fn variant_stock(db: &Db, sku: &str, size: &str) -> Result<i64, Error> {
    scalar(
        db,
        "select coalesce((select stock from variants where sku = ?1 and size = ?2), 0)",
        &[s(sku), s(size)],
    )
    .await
}

// --- cart

async fn create_cart(db: &Db, cart_id: &str) -> Result<(), Error> {
    execute(
        db,
        "insert or ignore into carts (id, created_at) values (?1, ?2)",
        &[s(cart_id), s(&Utc::now().to_rfc3339())],
    )
    .await?;
    Ok(())
}

pub async fn cart_lines(db: &Db, cart_id: &str) -> Result<Vec<CartLine>, Error> {
    fetch_all(
        db,
        "select l.sku, p.name, l.size, p.price_cents, l.quantity, \
                coalesce(v.stock, 0) as stock \
         from cart_lines l \
         join products p on p.sku = l.sku \
         left join variants v on v.sku = l.sku and v.size = l.size \
         where l.cart_id = ?1 order by p.name, l.size",
        &[s(cart_id)],
    )
    .await
}

pub async fn item_count(db: &Db, cart_id: &str) -> Result<i64, Error> {
    scalar(db, "select coalesce(sum(quantity), 0) from cart_lines where cart_id = ?1", &[s(cart_id)])
        .await
}

/// Sets a line's quantity and reports what was actually stored. An upsert,
/// not an update: raising the quantity of a line that was removed puts it
/// back, instead of quietly matching no row. Returns 0 when the line is
/// gone, and clamps to the stock on hand.
pub async fn set_quantity(
    db: &Db,
    cart_id: &str,
    sku: &str,
    size: &str,
    quantity: i64,
) -> Result<i64, Error> {
    if quantity <= 0 {
        remove_from_cart(db, cart_id, sku, size).await?;
        return Ok(0);
    }
    let stock = variant_stock(db, sku, size).await?;
    if stock == 0 {
        remove_from_cart(db, cart_id, sku, size).await?;
        return Ok(0);
    }
    create_cart(db, cart_id).await?;
    let kept = quantity.min(stock);
    execute(
        db,
        "insert into cart_lines (cart_id, sku, size, quantity) values (?1, ?2, ?3, ?4) \
         on conflict(cart_id, sku, size) do update set quantity = ?4",
        &[s(cart_id), s(sku), s(size), n(kept)],
    )
    .await?;
    Ok(kept)
}

pub async fn remove_from_cart(db: &Db, cart_id: &str, sku: &str, size: &str) -> Result<(), Error> {
    execute(
        db,
        "delete from cart_lines where cart_id = ?1 and sku = ?2 and size = ?3",
        &[s(cart_id), s(sku), s(size)],
    )
    .await?;
    Ok(())
}

pub async fn attach_cart(db: &Db, cart_id: &str, user_id: i64) -> Result<(), Error> {
    create_cart(db, cart_id).await?;
    execute(db, "update carts set user_id = ?2 where id = ?1", &[s(cart_id), n(user_id)]).await?;
    Ok(())
}

/// Returns true when the address is new; resubscribing changes nothing.
pub async fn subscribe(db: &Db, email: &str) -> Result<bool, Error> {
    let inserted = execute(
        db,
        "insert or ignore into subscribers (email, created_at) values (?1, ?2)",
        &[s(&email.trim().to_lowercase()), s(&Utc::now().to_rfc3339())],
    )
    .await?;
    Ok(inserted > 0)
}

// --- accounts

pub async fn register(db: &Db, email: &str, name: &str, password: &str) -> Result<User, Error> {
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| Error::msg(format!("hashing: {e}")))?
        .to_string();
    execute(
        db,
        "insert into users (email, name, password_hash, created_at) values (?1, ?2, ?3, ?4)",
        &[
            s(&email.trim().to_lowercase()),
            s(name.trim()),
            s(&hash),
            s(&Utc::now().to_rfc3339()),
        ],
    )
    .await?;
    let id =
        scalar(db, "select id from users where email = ?1", &[s(&email.trim().to_lowercase())])
            .await?;
    Ok(User { id, email: email.trim().to_lowercase(), name: name.trim().to_string(), admin: 0 })
}

pub async fn verify_credentials(
    db: &Db,
    email: &str,
    password: &str,
) -> Result<Option<User>, Error> {
    #[derive(Deserialize)]
    struct Row {
        id: i64,
        email: String,
        name: String,
        password_hash: String,
        admin: i64,
    }
    let Some(row) = fetch_first::<Row>(
        db,
        "select id, email, name, password_hash, admin from users where email = ?1",
        &[s(&email.trim().to_lowercase())],
    )
    .await?
    else {
        return Ok(None);
    };
    if Argon2::default()
        .verify_password(password.as_bytes(), row.password_hash.as_str())
        .is_err()
    {
        return Ok(None);
    }
    Ok(Some(User { id: row.id, email: row.email, name: row.name, admin: row.admin }))
}

pub async fn email_taken(db: &Db, email: &str) -> Result<bool, Error> {
    Ok(scalar(db, "select count(*) from users where email = ?1", &[s(&email.trim().to_lowercase())])
        .await?
        > 0)
}

// --- sessions

// The token hash arrives as bytes; D1 speaks JSON, so it is stored
// hex-encoded -- same information, edge-friendly clothes.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn open_session(
    db: &Db,
    token_hash: &[u8],
    user_id: i64,
    expires_at: DateTime<Utc>,
) -> Result<(), Error> {
    execute(
        db,
        "insert or replace into sessions (token_hash, user_id, expires_at) values (?1, ?2, ?3)",
        &[s(&hex(token_hash)), n(user_id), s(&expires_at.to_rfc3339())],
    )
    .await?;
    Ok(())
}

pub async fn close_session(db: &Db, token_hash: &[u8]) -> Result<(), Error> {
    execute(db, "delete from sessions where token_hash = ?1", &[s(&hex(token_hash))]).await?;
    Ok(())
}

pub async fn user_for_session(db: &Db, token_hash: &[u8]) -> Result<Option<User>, Error> {
    fetch_first(
        db,
        "select u.id, u.email, u.name, u.admin from sessions se \
         join users u on u.id = se.user_id \
         where se.token_hash = ?1 and se.expires_at > ?2",
        &[s(&hex(token_hash)), s(&Utc::now().to_rfc3339())],
    )
    .await
}

// --- orders

/// Clamps a cart to the stock actually on the shelves: lines whose item is
/// gone disappear, the others come down to what is left.
pub async fn clamp_cart_to_stock(db: &Db, cart_id: &str) -> Result<(), Error> {
    execute(
        db,
        "delete from cart_lines where cart_id = ?1 and coalesce((select v.stock \
         from variants v where v.sku = cart_lines.sku and v.size = cart_lines.size), 0) <= 0",
        &[s(cart_id)],
    )
    .await?;
    execute(
        db,
        "update cart_lines set quantity = (select v.stock from variants v \
         where v.sku = cart_lines.sku and v.size = cart_lines.size) \
         where cart_id = ?1 and quantity > (select v.stock from variants v \
         where v.sku = cart_lines.sku and v.size = cart_lines.size)",
        &[s(cart_id)],
    )
    .await?;
    Ok(())
}

/// The edge cannot hold an open transaction, so the walk is: guarded
/// decrements one by one, compensation if one loses the race, and only
/// then the order is written. Stock never goes negative; a lost race
/// costs a few statements, never an oversold order.
pub async fn place_order(
    db: &Db,
    user_id: i64,
    cart_id: &str,
    address: &str,
    shipping: &str,
) -> Result<Option<String>, Error> {
    let lines = cart_lines(db, cart_id).await?;
    if lines.is_empty() {
        return Err(Error::msg("empty cart"));
    }
    let subtotal: i64 = lines.iter().map(CartLine::subtotal).sum();
    let mode = shipping_mode(shipping);
    let shipping_fee = shipping_cents(subtotal, mode.key);
    let reference = order_reference();
    let now = Utc::now().to_rfc3339();

    // Take the stock, line by line, only where it still exists.
    let mut taken: Vec<&CartLine> = Vec::new();
    for line in &lines {
        let got = execute(
            db,
            "update variants set stock = stock - ?3 \
             where sku = ?1 and size = ?2 and stock >= ?3",
            &[s(&line.sku), s(&line.size), n(line.quantity)],
        )
        .await?;
        if got == 0 {
            // Compensation: hand back what this walk already took.
            for held in &taken {
                execute(
                    db,
                    "update variants set stock = stock + ?3 where sku = ?1 and size = ?2",
                    &[s(&held.sku), s(&held.size), n(held.quantity)],
                )
                .await?;
            }
            clamp_cart_to_stock(db, cart_id).await?;
            return Ok(None);
        }
        execute(
            db,
            "update products set stock = stock - ?2 where sku = ?1",
            &[s(&line.sku), n(line.quantity)],
        )
        .await?;
        taken.push(line);
    }

    execute(
        db,
        "insert into orders \
         (reference, user_id, total_cents, shipping_cents, shipping, status, address, created_at) \
         values (?1, ?2, ?3, ?4, ?5, 'paid', ?6, ?7)",
        &[
            s(&reference),
            n(user_id),
            n(subtotal + shipping_fee),
            n(shipping_fee),
            s(mode.key),
            s(address),
            s(&now),
        ],
    )
    .await?;
    let order_id = scalar(db, "select id from orders where reference = ?1", &[s(&reference)]).await?;
    for line in &lines {
        execute(
            db,
            "insert into order_lines (order_id, sku, name, size, price_cents, quantity) \
             values (?1, ?2, ?3, ?4, ?5, ?6)",
            &[
                n(order_id),
                s(&line.sku),
                s(&line.name),
                s(&line.size),
                n(line.price_cents),
                n(line.quantity),
            ],
        )
        .await?;
    }
    // A fresh order is paid and nothing more: the sweep moves it on.
    execute(
        db,
        "insert into tracking (order_id, step, note, at) values (?1, 'paid', ?2, ?3)",
        &[n(order_id), s("Paiement accepté, la commande entre en file."), s(&now)],
    )
    .await?;
    execute(db, "delete from cart_lines where cart_id = ?1", &[s(cart_id)]).await?;
    Ok(Some(reference))
}

pub async fn orders(db: &Db, user_id: i64) -> Result<Vec<Order>, Error> {
    fetch_all(
        db,
        &format!(
            "select {ORDER_FIELDS} from orders where user_id = ?1 order by created_at desc"
        ),
        &[n(user_id)],
    )
    .await
}

pub async fn order(
    db: &Db,
    user_id: i64,
    reference: &str,
) -> Result<Option<(Order, Vec<OrderLine>, Vec<TrackingStep>)>, Error> {
    let Some(order) = fetch_first::<Order>(
        db,
        &format!("select {ORDER_FIELDS} from orders where reference = ?1 and user_id = ?2"),
        &[s(reference), n(user_id)],
    )
    .await?
    else {
        return Ok(None);
    };

    let lines = fetch_all(
        db,
        "select sku, name, size, price_cents, quantity from order_lines where order_id = ?1",
        &[n(order.id)],
    )
    .await?;
    let tracking = fetch_all(
        db,
        "select step, note, at from tracking where order_id = ?1 order by at",
        &[n(order.id)],
    )
    .await?;
    Ok(Some((order, lines, tracking)))
}

/// Moves an order one step along, so tracking has something to show without
/// waiting on a real warehouse.
pub async fn advance_order(db: &Db, user_id: i64, reference: &str) -> Result<String, Error> {
    let Some((order, ..)) = order(db, user_id, reference).await? else {
        return Err(Error::msg("unknown order"));
    };
    let Some((next, note)) = next_step(&order.status) else {
        return Ok(order.status);
    };
    execute(db, "update orders set status = ?2 where id = ?1", &[n(order.id), s(next)]).await?;
    execute(
        db,
        "insert into tracking (order_id, step, note, at) values (?1, ?2, ?3, ?4)",
        &[n(order.id), s(next), s(note), s(&Utc::now().to_rfc3339())],
    )
    .await?;
    Ok(next.to_string())
}

/// Cancels an order while it is still in the workshop -- `paid` or
/// `packing`, nothing later -- and puts every unit back on the shelf.
/// Returns whether anything was cancelled.
pub async fn cancel_order(db: &Db, user_id: i64, reference: &str) -> Result<bool, Error> {
    let Some((order, lines, _)) = order(db, user_id, reference).await? else {
        return Ok(false);
    };
    if order.status != "paid" && order.status != "packing" {
        return Ok(false);
    }
    for line in &lines {
        execute(
            db,
            "update variants set stock = stock + ?3 where sku = ?1 and size = ?2",
            &[s(&line.sku), s(&line.size), n(line.quantity)],
        )
        .await?;
        execute(
            db,
            "update products set stock = stock + ?2 where sku = ?1",
            &[s(&line.sku), n(line.quantity)],
        )
        .await?;
    }
    execute(db, "update orders set status = 'cancelled' where id = ?1", &[n(order.id)]).await?;
    execute(
        db,
        "insert into tracking (order_id, step, note, at) values (?1, 'cancelled', ?2, ?3)",
        &[n(order.id), s("Commande annulée à votre demande."), s(&Utc::now().to_rfc3339())],
    )
    .await?;
    Ok(true)
}

// --- reviews, alerts, addresses

pub async fn product_reviews(db: &Db, sku: &str) -> Result<Vec<Review>, Error> {
    fetch_all(
        db,
        "select author, rating, text, created_at from reviews where sku = ?1 \
         order by created_at desc",
        &[s(sku)],
    )
    .await
}

pub async fn add_review(
    db: &Db,
    sku: &str,
    author: &str,
    rating: i64,
    text: &str,
) -> Result<(), Error> {
    execute(
        db,
        "insert into reviews (sku, author, rating, text, created_at) values (?1, ?2, ?3, ?4, ?5)",
        &[s(sku), s(author), n(rating.clamp(1, 5)), s(text.trim()), s(&Utc::now().to_rfc3339())],
    )
    .await?;
    Ok(())
}

pub async fn create_stock_alert(db: &Db, sku: &str, size: &str, email: &str) -> Result<(), Error> {
    execute(
        db,
        "insert or ignore into stock_alerts (sku, size, email, created_at) \
         values (?1, ?2, ?3, ?4)",
        &[s(sku), s(size), s(&email.trim().to_lowercase()), s(&Utc::now().to_rfc3339())],
    )
    .await?;
    Ok(())
}

pub async fn addresses(db: &Db, user_id: i64) -> Result<Vec<Address>, Error> {
    fetch_all(
        db,
        "select id, label, text, is_default from addresses \
         where user_id = ?1 order by is_default desc, id",
        &[n(user_id)],
    )
    .await
}

pub async fn address(db: &Db, user_id: i64, id: i64) -> Result<Option<Address>, Error> {
    fetch_first(
        db,
        "select id, label, text, is_default from addresses where user_id = ?1 and id = ?2",
        &[n(user_id), n(id)],
    )
    .await
}

/// The first address a visitor saves becomes their default one.
pub async fn add_address(db: &Db, user_id: i64, label: &str, text: &str) -> Result<(), Error> {
    execute(
        db,
        "insert into addresses (user_id, label, text, is_default) values (?1, ?2, ?3, \
         (select count(*) = 0 from addresses where user_id = ?1))",
        &[n(user_id), s(label.trim()), s(&text.replace('\r', ""))],
    )
    .await?;
    Ok(())
}

pub async fn remove_address(db: &Db, user_id: i64, id: i64) -> Result<(), Error> {
    execute(db, "delete from addresses where user_id = ?1 and id = ?2", &[n(user_id), n(id)])
        .await?;
    Ok(())
}

pub async fn set_default_address(db: &Db, user_id: i64, id: i64) -> Result<(), Error> {
    execute(db, "update addresses set is_default = 0 where user_id = ?1", &[n(user_id)]).await?;
    execute(
        db,
        "update addresses set is_default = 1 where user_id = ?1 and id = ?2",
        &[n(user_id), n(id)],
    )
    .await?;
    Ok(())
}

// --- admin

pub async fn admin_stats(db: &Db) -> Result<AdminStats, Error> {
    Ok(AdminStats {
        products: scalar(db, "select count(*) from products", &[]).await?,
        orders: scalar(db, "select count(*) from orders", &[]).await?,
        customers: scalar(db, "select count(*) from users", &[]).await?,
        subscribers: scalar(db, "select count(*) from subscribers", &[]).await?,
        alerts: scalar(db, "select count(*) from stock_alerts", &[]).await?,
        revenue_cents: scalar(
            db,
            "select coalesce(sum(total_cents), 0) from orders where status != 'cancelled'",
            &[],
        )
        .await?,
    })
}

pub async fn admin_orders(db: &Db) -> Result<Vec<AdminOrder>, Error> {
    fetch_all(
        db,
        "select o.reference, o.status, o.total_cents, o.created_at, \
                u.name as customer, u.email \
         from orders o join users u on u.id = o.user_id \
         order by o.created_at desc",
        &[],
    )
    .await
}

pub async fn admin_customers(db: &Db) -> Result<Vec<AdminCustomer>, Error> {
    fetch_all(
        db,
        "select u.name, u.email, u.admin, count(o.id) as orders, \
                coalesce(sum(case when o.status != 'cancelled' then o.total_cents end), 0) \
                    as total_cents \
         from users u left join orders o on o.user_id = u.id \
         group by u.id order by u.created_at",
        &[],
    )
    .await
}

/// Every product, hidden ones included: the back office has no curtain.
/// Walks every order that still has a rung to climb, one rung per call.
/// The tracking line is the one a visitor gets by advancing a parcel by
/// hand.
pub async fn advance_pending(db: &Db) -> Result<u64, Error> {
    #[derive(Deserialize)]
    struct Waiting {
        id: i64,
        status: String,
    }

    let waiting: Vec<Waiting> = fetch_all(
        db,
        "select id, status from orders where status in ('paid', 'packing', 'shipped')",
        &[],
    )
    .await?;

    let mut moved = 0;
    for order in waiting {
        let Some((next, note)) = next_step(&order.status) else { continue };
        execute(db, "update orders set status = ?2 where id = ?1", &[n(order.id), s(next)]).await?;
        execute(
            db,
            "insert into tracking (order_id, step, note, at) values (?1, ?2, ?3, ?4)",
            &[n(order.id), s(next), s(note), s(&Utc::now().to_rfc3339())],
        )
        .await?;
        moved += 1;
    }
    Ok(moved)
}

pub async fn all_products(db: &Db) -> Result<Vec<Product>, Error> {
    fetch_all(
        db,
        &format!("select {PRODUCT_FIELDS} from products order by hidden, category, name"),
        &[],
    )
    .await
}

pub async fn toggle_hidden(db: &Db, sku: &str) -> Result<(), Error> {
    execute(db, "update products set hidden = 1 - hidden where sku = ?1", &[s(sku)]).await?;
    Ok(())
}

pub async fn set_price(db: &Db, sku: &str, price_cents: i64) -> Result<(), Error> {
    execute(
        db,
        "update products set price_cents = ?2 where sku = ?1",
        &[s(sku), n(price_cents.max(0))],
    )
    .await?;
    Ok(())
}

async fn resum_stock(db: &Db, sku: &str) -> Result<(), Error> {
    execute(
        db,
        "update products set stock = \
         (select coalesce(sum(stock), 0) from variants where sku = ?1) where sku = ?1",
        &[s(sku)],
    )
    .await?;
    Ok(())
}

pub async fn set_stock(db: &Db, sku: &str, size: &str, stock: i64) -> Result<(), Error> {
    execute(
        db,
        "update variants set stock = ?3 where sku = ?1 and size = ?2",
        &[s(sku), s(size), n(stock.max(0))],
    )
    .await?;
    resum_stock(db, sku).await
}

#[allow(clippy::too_many_arguments)]
pub async fn update_product(
    db: &Db,
    sku: &str,
    name: &str,
    summary: &str,
    detail: &str,
    category: &str,
    material: &str,
    is_new: i64,
) -> Result<(), Error> {
    execute(
        db,
        "update products set name = ?2, summary = ?3, detail = ?4, category = ?5, \
         material = ?6, is_new = ?7 where sku = ?1",
        &[
            s(sku),
            s(name.trim()),
            s(summary.trim()),
            s(detail.trim()),
            s(category.trim()),
            s(material.trim()),
            n(is_new),
        ],
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn create_product(
    db: &Db,
    sku: &str,
    name: &str,
    summary: &str,
    detail: &str,
    price_cents: i64,
    category: &str,
    material: &str,
    is_new: i64,
    stock: i64,
) -> Result<(), Error> {
    execute(
        db,
        "insert into products (sku, name, summary, detail, price_cents, stock, category, \
         is_new, material) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        &[
            s(sku),
            s(name.trim()),
            s(summary.trim()),
            s(detail.trim()),
            n(price_cents.max(0)),
            n(stock.max(0)),
            s(category.trim()),
            n(is_new),
            s(material.trim()),
        ],
    )
    .await?;
    // The one-size row the cart reads its stock from; sizes come later.
    execute(
        db,
        "insert into variants (sku, size, stock, rank) values (?1, '', ?2, 1)",
        &[s(sku), n(stock.max(0))],
    )
    .await?;
    Ok(())
}

pub async fn add_variant(db: &Db, sku: &str, size: &str, stock: i64) -> Result<(), Error> {
    execute(
        db,
        "insert into variants (sku, size, stock, rank) values (?1, ?2, ?3, \
         (select coalesce(max(rank), 0) + 1 from variants where sku = ?1)) \
         on conflict(sku, size) do update set stock = ?3",
        &[s(sku), s(size.trim()), n(stock.max(0))],
    )
    .await?;
    resum_stock(db, sku).await
}

pub async fn remove_variant(db: &Db, sku: &str, size: &str) -> Result<(), Error> {
    execute(db, "delete from variants where sku = ?1 and size = ?2", &[s(sku), s(size)]).await?;
    resum_stock(db, sku).await
}
