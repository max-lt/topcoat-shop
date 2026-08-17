//! The data layer: pool, migrations, and every query the shop runs. Queries
//! live here rather than in the pages so the SQL can be read in one place.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Duration, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{AssertSqlSafe, FromRow, SqlitePool};

/// anyhow, so `?` converts straight into topcoat's error type at the
/// call sites without a per-module helper.
pub type Error = anyhow::Error;

// --- model

#[derive(Debug, Clone, FromRow)]
pub struct Product {
    pub sku: String,
    pub name: String,
    pub detail: String,
    pub price_cents: i64,
    pub stock: i64,
    pub category: String,
    pub is_new: i64,
    pub material: String,
    pub hidden: i64,
}

impl Product {
    pub fn sold_out(&self) -> bool {
        self.stock == 0
    }
    /// Below this, the product page says so rather than staying silent.
    pub fn low_stock(&self) -> bool {
        self.stock > 0 && self.stock <= 5
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct Variant {
    pub size: String,
    pub stock: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct CartLine {
    pub sku: String,
    pub name: String,
    pub size: String,
    pub price_cents: i64,
    pub quantity: i64,
    pub stock: i64,
}

impl CartLine {
    pub fn subtotal(&self) -> i64 {
        self.price_cents * self.quantity
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct Order {
    pub id: i64,
    pub reference: String,
    pub total_cents: i64,
    pub shipping_cents: i64,
    pub shipping: String,
    pub status: String,
    pub address: String,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct OrderLine {
    pub sku: String,
    pub name: String,
    pub size: String,
    pub price_cents: i64,
    pub quantity: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct TrackingStep {
    pub step: String,
    pub note: String,
    pub at: String,
}

/// Free shipping above this; the threshold is quoted in the header banner.
pub const FREE_SHIPPING_CENTS: i64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShippingMode {
    pub key: &'static str,
    pub name: &'static str,
    pub delay: &'static str,
    pub cents: i64,
}

pub const SHIPPING_MODES: [ShippingMode; 2] = [
    ShippingMode { key: "standard", name: "Standard", delay: "3 à 5 jours ouvrés", cents: 490 },
    ShippingMode { key: "express", name: "Express", delay: "24 à 48 heures", cents: 1_190 },
];

pub fn shipping_mode(key: &str) -> ShippingMode {
    SHIPPING_MODES.iter().copied().find(|m| m.key == key).unwrap_or(SHIPPING_MODES[0])
}

/// Shipping is free once the basket passes the threshold.
pub fn shipping_cents(subtotal: i64, key: &str) -> i64 {
    if subtotal >= FREE_SHIPPING_CENTS { 0 } else { shipping_mode(key).cents }
}

/// Prices are integers everywhere; this is the only place they become text.
pub fn format_price(cents: i64) -> String {
    format!("{},{:02}\u{a0}€", cents / 100, (cents % 100).abs())
}

// --- setup

pub async fn connect(url: &str) -> Result<SqlitePool, Error> {
    let options = SqliteConnectOptions::new()
        .filename(url)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(5).connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

// --- catalog

const PRODUCT_FIELDS: &str =
    "sku, name, summary, detail, price_cents, stock, category, is_new, material, hidden";

/// The catalog, filtered by category and ordered by the visitor's choice.
/// `sort`: 0 newest first, 1 price ascending, 2 price descending, 3 alphabetical.
pub async fn catalog(
    pool: &SqlitePool,
    category: &str,
    sort: i64,
) -> Result<Vec<Product>, Error> {
    let order = match sort {
        1 => "price_cents asc, name",
        2 => "price_cents desc, name",
        3 => "name",
        _ => "is_new desc, category, name",
    };
    let everything = category.is_empty() || category == "Tout";
    let filter = if everything { "where hidden = 0" } else { "where hidden = 0 and category = ?1" };
    let sql = format!("select {PRODUCT_FIELDS} from products {filter} order by {order}");

    let query = sqlx::query_as::<_, Product>(AssertSqlSafe(sql));
    let query = if everything { query } else { query.bind(category) };
    Ok(query.fetch_all(pool).await?)
}

pub async fn search(pool: &SqlitePool, term: &str) -> Result<Vec<Product>, Error> {
    let pattern = format!("%{}%", term.trim().to_lowercase());
    Ok(sqlx::query_as::<_, Product>(AssertSqlSafe(format!(
        "select {PRODUCT_FIELDS} from products \
         where hidden = 0 and (lower(name) like ?1 or lower(summary) like ?1 \
            or lower(category) like ?1 or lower(material) like ?1) \
         order by is_new desc, name"
    )))
    .bind(pattern)
    .fetch_all(pool)
    .await?)
}

pub async fn categories(pool: &SqlitePool) -> Result<Vec<String>, Error> {
    Ok(sqlx::query_scalar("select distinct category from products order by category")
        .fetch_all(pool)
        .await?)
}

pub async fn product(pool: &SqlitePool, sku: &str) -> Result<Option<Product>, Error> {
    Ok(sqlx::query_as::<_, Product>(AssertSqlSafe(format!(
        "select {PRODUCT_FIELDS} from products where sku = ?1"
    )))
    .bind(sku)
    .fetch_optional(pool)
    .await?)
}

/// Same shelf first, then the rest of the shop.
pub async fn related_products(
    pool: &SqlitePool,
    sku: &str,
    category: &str,
) -> Result<Vec<Product>, Error> {
    Ok(sqlx::query_as::<_, Product>(AssertSqlSafe(format!(
        "select {PRODUCT_FIELDS} from products where hidden = 0 and sku <> ?1 \
         order by case when category = ?2 then 0 else 1 end, is_new desc, name limit 3"
    )))
    .bind(sku)
    .bind(category)
    .fetch_all(pool)
    .await?)
}

pub async fn new_arrivals(pool: &SqlitePool, how_many: i64) -> Result<Vec<Product>, Error> {
    Ok(sqlx::query_as::<_, Product>(AssertSqlSafe(format!(
        "select {PRODUCT_FIELDS} from products where hidden = 0 order by is_new desc, stock desc limit ?1"
    )))
    .bind(how_many)
    .fetch_all(pool)
    .await?)
}

pub async fn variants(pool: &SqlitePool, sku: &str) -> Result<Vec<Variant>, Error> {
    Ok(sqlx::query_as::<_, Variant>(
        "select size, stock from variants where sku = ?1 order by rank, size",
    )
    .bind(sku)
    .fetch_all(pool)
    .await?)
}

async fn variant_stock(pool: &SqlitePool, sku: &str, size: &str) -> Result<i64, Error> {
    Ok(sqlx::query_scalar("select stock from variants where sku = ?1 and size = ?2")
        .bind(sku)
        .bind(size)
        .fetch_optional(pool)
        .await?
        .unwrap_or(0))
}

// --- accounts

pub async fn register(
    pool: &SqlitePool,
    email: &str,
    name: &str,
    password: &str,
) -> Result<User, Error> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hashing: {e}"))?
        .to_string();

    let id: i64 = sqlx::query_scalar(
        "insert into users (email, name, password_hash, created_at) \
         values (?1, ?2, ?3, ?4) returning id",
    )
    .bind(email.trim().to_lowercase())
    .bind(name.trim())
    .bind(hash)
    .bind(Utc::now().to_rfc3339())
    .fetch_one(pool)
    .await?;

    Ok(User { id, email: email.trim().to_lowercase(), name: name.trim().to_string() })
}

pub async fn verify_credentials(
    pool: &SqlitePool,
    email: &str,
    password: &str,
) -> Result<Option<User>, Error> {
    let row: Option<(i64, String, String, String)> = sqlx::query_as(
        "select id, email, name, password_hash from users where email = ?1",
    )
    .bind(email.trim().to_lowercase())
    .fetch_optional(pool)
    .await?;

    let Some((id, email, name, hash)) = row else {
        return Ok(None);
    };
    let expected = PasswordHash::new(&hash).map_err(|e| anyhow::anyhow!("hash: {e}"))?;
    if Argon2::default().verify_password(password.as_bytes(), &expected).is_err() {
        return Ok(None);
    }
    Ok(Some(User { id, email, name }))
}

pub async fn email_taken(pool: &SqlitePool, email: &str) -> Result<bool, Error> {
    let n: i64 = sqlx::query_scalar("select count(*) from users where email = ?1")
        .bind(email.trim().to_lowercase())
        .fetch_one(pool)
        .await?;
    Ok(n > 0)
}

// --- sessions

pub async fn open_session(
    pool: &SqlitePool,
    token_hash: &[u8],
    user_id: i64,
    expires_at: DateTime<Utc>,
) -> Result<(), Error> {
    sqlx::query(
        "insert or replace into sessions (token_hash, user_id, expires_at) \
         values (?1, ?2, ?3)",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(expires_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Resolves a session token hash to its user, treating an expired record as
/// no session at all.
pub async fn user_for_session(
    pool: &SqlitePool,
    token_hash: &[u8],
) -> Result<Option<User>, Error> {
    Ok(sqlx::query_as::<_, User>(
        "select u.id, u.email, u.name from sessions s \
         join users u on u.id = s.user_id \
         where s.token_hash = ?1 and s.expires_at > ?2",
    )
    .bind(token_hash)
    .bind(Utc::now().to_rfc3339())
    .fetch_optional(pool)
    .await?)
}

pub async fn close_session(pool: &SqlitePool, token_hash: &[u8]) -> Result<(), Error> {
    sqlx::query("delete from sessions where token_hash = ?1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

// --- cart

pub async fn create_cart(pool: &SqlitePool, id: &str) -> Result<(), Error> {
    sqlx::query("insert or ignore into carts (id, created_at) values (?1, ?2)")
        .bind(id)
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?;
    Ok(())
}

/// Signing in claims whatever the visitor had in their anonymous cart.
pub async fn attach_cart(pool: &SqlitePool, cart_id: &str, user_id: i64) -> Result<(), Error> {
    sqlx::query("update carts set user_id = ?2 where id = ?1")
        .bind(cart_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn cart_lines(pool: &SqlitePool, cart_id: &str) -> Result<Vec<CartLine>, Error> {
    Ok(sqlx::query_as::<_, CartLine>(
        "select p.sku, p.name, l.size, p.price_cents, l.quantity, \
                coalesce(v.stock, p.stock) as stock \
         from cart_lines l \
         join products p on p.sku = l.sku \
         left join variants v on v.sku = l.sku and v.size = l.size \
         where l.cart_id = ?1 order by p.name, l.size",
    )
    .bind(cart_id)
    .fetch_all(pool)
    .await?)
}

pub async fn add_to_cart(
    pool: &SqlitePool,
    cart_id: &str,
    sku: &str,
    size: &str,
    quantity: i64,
) -> Result<i64, Error> {
    create_cart(pool, cart_id).await?;
    let stock = variant_stock(pool, sku, size).await?;
    if stock == 0 {
        return item_count(pool, cart_id).await;
    }

    sqlx::query(
        "insert into cart_lines (cart_id, sku, size, quantity) values (?1, ?2, ?3, ?4) \
         on conflict(cart_id, sku, size) do update set quantity = min(quantity + ?4, ?5)",
    )
    .bind(cart_id)
    .bind(sku)
    .bind(size)
    .bind(quantity.clamp(1, stock))
    .bind(stock)
    .execute(pool)
    .await?;

    item_count(pool, cart_id).await
}

/// Sets a line's quantity and reports what was actually stored. An upsert,
/// not an update: raising the quantity of a line that was removed puts it
/// back, instead of quietly matching no row. Returns 0 when the line is
/// gone, and clamps to the stock on hand.
pub async fn set_quantity(
    pool: &SqlitePool,
    cart_id: &str,
    sku: &str,
    size: &str,
    quantity: i64,
) -> Result<i64, Error> {
    if quantity <= 0 {
        remove_from_cart(pool, cart_id, sku, size).await?;
        return Ok(0);
    }
    let stock = variant_stock(pool, sku, size).await?;
    if stock == 0 {
        remove_from_cart(pool, cart_id, sku, size).await?;
        return Ok(0);
    }

    create_cart(pool, cart_id).await?;
    let kept = quantity.min(stock);
    sqlx::query(
        "insert into cart_lines (cart_id, sku, size, quantity) values (?1, ?2, ?3, ?4) \
         on conflict(cart_id, sku, size) do update set quantity = ?4",
    )
    .bind(cart_id)
    .bind(sku)
    .bind(size)
    .bind(kept)
    .execute(pool)
    .await?;
    Ok(kept)
}

pub async fn remove_from_cart(
    pool: &SqlitePool,
    cart_id: &str,
    sku: &str,
    size: &str,
) -> Result<(), Error> {
    sqlx::query("delete from cart_lines where cart_id = ?1 and sku = ?2 and size = ?3")
        .bind(cart_id)
        .bind(sku)
        .bind(size)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn item_count(pool: &SqlitePool, cart_id: &str) -> Result<i64, Error> {
    Ok(
        sqlx::query_scalar("select coalesce(sum(quantity), 0) from cart_lines where cart_id = ?1")
            .bind(cart_id)
            .fetch_one(pool)
            .await?,
    )
}

/// Returns true when the address is new; resubscribing changes nothing.
pub async fn subscribe(pool: &SqlitePool, email: &str) -> Result<bool, Error> {
    let inserted = sqlx::query("insert or ignore into subscribers (email, created_at) values (?1, ?2)")
        .bind(email.trim().to_lowercase())
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?
        .rows_affected();
    Ok(inserted > 0)
}

// --- reviews

#[derive(Debug, Clone, FromRow)]
pub struct Review {
    pub author: String,
    pub rating: i64,
    pub text: String,
    pub created_at: String,
}

/// Newest first. The shop is small enough to show every review in full.
pub async fn product_reviews(pool: &SqlitePool, sku: &str) -> Result<Vec<Review>, Error> {
    Ok(sqlx::query_as::<_, Review>(
        "select author, rating, text, created_at from reviews where sku = ?1 \
         order by created_at desc",
    )
    .bind(sku)
    .fetch_all(pool)
    .await?)
}

pub async fn add_review(
    pool: &SqlitePool,
    sku: &str,
    author: &str,
    rating: i64,
    text: &str,
) -> Result<(), Error> {
    sqlx::query(
        "insert into reviews (sku, author, rating, text, created_at) values (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(sku)
    .bind(author)
    .bind(rating.clamp(1, 5))
    .bind(text.trim())
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Back-in-stock alert. Asking twice for the same size is a no-op.
pub async fn create_stock_alert(
    pool: &SqlitePool,
    sku: &str,
    size: &str,
    email: &str,
) -> Result<(), Error> {
    sqlx::query(
        "insert or ignore into stock_alerts (sku, size, email, created_at) values (?1, ?2, ?3, ?4)",
    )
    .bind(sku)
    .bind(size)
    .bind(email.trim().to_lowercase())
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

// --- addresses

#[derive(Debug, Clone, FromRow)]
pub struct Address {
    pub id: i64,
    pub label: String,
    pub text: String,
    pub is_default: i64,
}

pub async fn addresses(pool: &SqlitePool, user_id: i64) -> Result<Vec<Address>, Error> {
    Ok(sqlx::query_as::<_, Address>(
        "select id, label, text, is_default from addresses \
         where user_id = ?1 order by is_default desc, id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

pub async fn address(pool: &SqlitePool, user_id: i64, id: i64) -> Result<Option<Address>, Error> {
    Ok(sqlx::query_as::<_, Address>(
        "select id, label, text, is_default from addresses where user_id = ?1 and id = ?2",
    )
    .bind(user_id)
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

/// The first address a visitor saves becomes their default one.
pub async fn add_address(
    pool: &SqlitePool,
    user_id: i64,
    label: &str,
    text: &str,
) -> Result<(), Error> {
    sqlx::query(
        "insert into addresses (user_id, label, text, is_default) values (?1, ?2, ?3, \
         (select count(*) = 0 from addresses where user_id = ?1))",
    )
    .bind(user_id)
    .bind(label.trim())
    // Textareas submit CRLF; the base keeps plain newlines.
    .bind(text.replace('\r', "").trim())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remove_address(pool: &SqlitePool, user_id: i64, id: i64) -> Result<(), Error> {
    sqlx::query("delete from addresses where user_id = ?1 and id = ?2")
        .bind(user_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_default_address(pool: &SqlitePool, user_id: i64, id: i64) -> Result<(), Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("update addresses set is_default = 0 where user_id = ?1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update addresses set is_default = 1 where user_id = ?1 and id = ?2")
        .bind(user_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

// --- orders

/// Clamps a cart to the stock actually on the shelves: lines whose item is
/// gone disappear, the others come down to what is left.
pub async fn clamp_cart_to_stock(pool: &SqlitePool, cart_id: &str) -> Result<(), Error> {
    // Delete first: the quantity > 0 check forbids clamping a line to zero.
    sqlx::query(
        "delete from cart_lines where cart_id = ?1 and coalesce((select v.stock \
         from variants v where v.sku = cart_lines.sku and v.size = cart_lines.size), 0) <= 0",
    )
    .bind(cart_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "update cart_lines set quantity = (select v.stock from variants v \
         where v.sku = cart_lines.sku and v.size = cart_lines.size) \
         where cart_id = ?1 and quantity > (select v.stock from variants v \
         where v.sku = cart_lines.sku and v.size = cart_lines.size)",
    )
    .bind(cart_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Walks the cart into an order, in one transaction. `None` means the stock
/// moved under the visitor's feet: nothing was written, their cart was
/// clamped to what is really left, and the caller should send them back to
/// look at it.
pub async fn place_order(
    pool: &SqlitePool,
    user_id: i64,
    cart_id: &str,
    address: &str,
    shipping: &str,
) -> Result<Option<String>, Error> {
    let lines = cart_lines(pool, cart_id).await?;
    if lines.is_empty() {
        return Err(anyhow::anyhow!("empty cart"));
    }
    let subtotal: i64 = lines.iter().map(CartLine::subtotal).sum();
    let mode = shipping_mode(shipping);
    let shipping_fee = shipping_cents(subtotal, mode.key);
    let reference = order_reference();
    let now = Utc::now();

    let mut tx = pool.begin().await?;

    let order_id: i64 = sqlx::query_scalar(
        "insert into orders \
         (reference, user_id, total_cents, shipping_cents, shipping, status, address, created_at) \
         values (?1, ?2, ?3, ?4, ?5, 'paid', ?6, ?7) returning id",
    )
    .bind(&reference)
    .bind(user_id)
    .bind(subtotal + shipping_fee)
    .bind(shipping_fee)
    .bind(mode.key)
    .bind(address)
    .bind(now.to_rfc3339())
    .fetch_one(&mut *tx)
    .await?;

    for line in &lines {
        sqlx::query(
            "insert into order_lines (order_id, sku, name, size, price_cents, quantity) \
             values (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(order_id)
        .bind(&line.sku)
        .bind(&line.name)
        .bind(&line.size)
        .bind(line.price_cents)
        .bind(line.quantity)
        .execute(&mut *tx)
        .await?;

        // The moment of truth: take the stock only if it is still there.
        // Two carts can hold the same last items; only one gets them.
        let taken = sqlx::query(
            "update variants set stock = stock - ?3 \
             where sku = ?1 and size = ?2 and stock >= ?3",
        )
        .bind(&line.sku)
        .bind(&line.size)
        .bind(line.quantity)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if taken == 0 {
            // Explicit rollback: dropping the transaction rolls back lazily,
            // and the clamp below would wait on its own lock.
            tx.rollback().await?;
            clamp_cart_to_stock(pool, cart_id).await?;
            return Ok(None);
        }

        sqlx::query("update products set stock = stock - ?2 where sku = ?1")
            .bind(&line.sku)
            .bind(line.quantity)
            .execute(&mut *tx)
            .await?;
    }

    for (offset, step, note) in [
        (0i64, "paid", "Paiement accepté, la commande entre en file."),
        (35, "packing", "Les articles sortent du stock de la coquille."),
    ] {
        sqlx::query("insert into tracking (order_id, step, note, at) values (?1, ?2, ?3, ?4)")
            .bind(order_id)
            .bind(step)
            .bind(note)
            .bind((now + Duration::minutes(offset)).to_rfc3339())
            .execute(&mut *tx)
            .await?;
    }

    sqlx::query("update orders set status = 'packing' where id = ?1")
        .bind(order_id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("delete from cart_lines where cart_id = ?1")
        .bind(cart_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(Some(reference))
}

const ORDER_FIELDS: &str =
    "id, reference, total_cents, shipping_cents, shipping, status, address, created_at";

pub async fn orders(pool: &SqlitePool, user_id: i64) -> Result<Vec<Order>, Error> {
    Ok(sqlx::query_as::<_, Order>(AssertSqlSafe(format!(
        "select {ORDER_FIELDS} from orders where user_id = ?1 order by created_at desc"
    )))
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

pub async fn order(
    pool: &SqlitePool,
    user_id: i64,
    reference: &str,
) -> Result<Option<(Order, Vec<OrderLine>, Vec<TrackingStep>)>, Error> {
    let Some(order) = sqlx::query_as::<_, Order>(AssertSqlSafe(format!(
        "select {ORDER_FIELDS} from orders where reference = ?1 and user_id = ?2"
    )))
    .bind(reference)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };

    let lines = sqlx::query_as::<_, OrderLine>(
        "select sku, name, size, price_cents, quantity from order_lines where order_id = ?1",
    )
    .bind(order.id)
    .fetch_all(pool)
    .await?;

    let tracking = sqlx::query_as::<_, TrackingStep>(
        "select step, note, at from tracking where order_id = ?1 order by at",
    )
    .bind(order.id)
    .fetch_all(pool)
    .await?;

    Ok(Some((order, lines, tracking)))
}

/// Moves an order one step along, so tracking has something to show without
/// waiting on a real warehouse.
pub async fn advance_order(
    pool: &SqlitePool,
    user_id: i64,
    reference: &str,
) -> Result<String, Error> {
    let Some((order, ..)) = order(pool, user_id, reference).await? else {
        return Err(anyhow::anyhow!("unknown order"));
    };
    let (next, note) = match order.status.as_str() {
        "paid" => ("packing", "Les articles sortent du stock de la coquille."),
        "packing" => ("shipped", "Colis remis au transporteur, suivi actif."),
        "shipped" => ("delivered", "Livré. Bon déballage."),
        _ => return Ok(order.status),
    };

    let mut tx = pool.begin().await?;
    sqlx::query("update orders set status = ?2 where id = ?1")
        .bind(order.id)
        .bind(next)
        .execute(&mut *tx)
        .await?;
    sqlx::query("insert into tracking (order_id, step, note, at) values (?1, ?2, ?3, ?4)")
        .bind(order.id)
        .bind(next)
        .bind(note)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(next.to_string())
}

/// Cancels an order while it is still in the workshop -- `paid` or
/// `packing`, nothing later -- and puts every unit back on the shelf.
/// Returns whether anything was cancelled.
pub async fn cancel_order(
    pool: &SqlitePool,
    user_id: i64,
    reference: &str,
) -> Result<bool, Error> {
    let Some((order, lines, _)) = order(pool, user_id, reference).await? else {
        return Ok(false);
    };
    if order.status != "paid" && order.status != "packing" {
        return Ok(false);
    }

    let mut tx = pool.begin().await?;
    for line in &lines {
        sqlx::query("update variants set stock = stock + ?3 where sku = ?1 and size = ?2")
            .bind(&line.sku)
            .bind(&line.size)
            .bind(line.quantity)
            .execute(&mut *tx)
            .await?;
        sqlx::query("update products set stock = stock + ?2 where sku = ?1")
            .bind(&line.sku)
            .bind(line.quantity)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("update orders set status = 'cancelled' where id = ?1")
        .bind(order.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("insert into tracking (order_id, step, note, at) values (?1, 'cancelled', ?2, ?3)")
        .bind(order.id)
        .bind("Commande annulée à votre demande.")
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

fn order_reference() -> String {
    const ALPHABET: &[u8] = b"ACDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut raw = [0u8; 6];
    rand::fill(&mut raw);
    let suffix: String =
        raw.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect();
    format!("BER-{suffix}")
}
