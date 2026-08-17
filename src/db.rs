//! The data layer: pool, migrations, and every query the shop runs. Queries
//! live here rather than in the pages so the SQL can be read in one place.

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

/// Free shipping above this; the threshold is quoted in the header banner.
pub const FREE_SHIPPING_CENTS: i64 = 5_000;

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
