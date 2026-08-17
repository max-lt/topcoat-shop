//! The data layer: pool, migrations, and every query the shop runs. Queries
//! live here rather than in the pages so the SQL can be read in one place.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Utc};
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
