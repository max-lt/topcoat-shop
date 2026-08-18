//! The sqlite backend: every query the shop runs, over a tokio pool.
//! Queries live here rather than in the pages so the SQL can be read in
//! one place -- and so swapping the store means swapping one file.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{AssertSqlSafe, SqlitePool};

use super::*;

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

    Ok(User { id, email: email.trim().to_lowercase(), name: name.trim().to_string(), admin: 0 })
}

pub async fn verify_credentials(
    pool: &SqlitePool,
    email: &str,
    password: &str,
) -> Result<Option<User>, Error> {
    let row: Option<(i64, String, String, String, i64)> = sqlx::query_as(
        "select id, email, name, password_hash, admin from users where email = ?1",
    )
    .bind(email.trim().to_lowercase())
    .fetch_optional(pool)
    .await?;

    let Some((id, email, name, hash, admin)) = row else {
        return Ok(None);
    };
    let expected = PasswordHash::new(&hash).map_err(|e| anyhow::anyhow!("hash: {e}"))?;
    if Argon2::default().verify_password(password.as_bytes(), &expected).is_err() {
        return Ok(None);
    }
    Ok(Some(User { id, email, name, admin }))
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
        "select u.id, u.email, u.name, u.admin from sessions s \
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

    // A fresh order is paid and nothing more: the sweep moves it on.
    sqlx::query("insert into tracking (order_id, step, note, at) values (?1, 'paid', ?2, ?3)")
        .bind(order_id)
        .bind("Paiement accepté, la commande entre en file.")
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

    sqlx::query("delete from cart_lines where cart_id = ?1")
        .bind(cart_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(Some(reference))
}


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
    let Some((next, note)) = next_step(&order.status) else {
        return Ok(order.status);
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

// --- admin

pub async fn admin_stats(pool: &SqlitePool) -> Result<AdminStats, Error> {
    let one = |sql: &'static str| sqlx::query_scalar::<_, i64>(sql).fetch_one(pool);
    Ok(AdminStats {
        products: one("select count(*) from products").await?,
        orders: one("select count(*) from orders").await?,
        customers: one("select count(*) from users").await?,
        subscribers: one("select count(*) from subscribers").await?,
        alerts: one("select count(*) from stock_alerts").await?,
        revenue_cents: one(
            "select coalesce(sum(total_cents), 0) from orders where status != 'cancelled'",
        )
        .await?,
    })
}

pub async fn admin_orders(pool: &SqlitePool) -> Result<Vec<AdminOrder>, Error> {
    Ok(sqlx::query_as::<_, AdminOrder>(
        "select o.reference, o.status, o.total_cents, o.created_at, \
                u.name as customer, u.email \
         from orders o join users u on u.id = o.user_id \
         order by o.created_at desc",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn admin_customers(pool: &SqlitePool) -> Result<Vec<AdminCustomer>, Error> {
    Ok(sqlx::query_as::<_, AdminCustomer>(
        "select u.name, u.email, u.admin, \
                count(o.id) as orders, \
                coalesce(sum(case when o.status != 'cancelled' then o.total_cents end), 0) \
                    as total_cents \
         from users u left join orders o on o.user_id = u.id \
         group by u.id order by u.created_at",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn set_price(pool: &SqlitePool, sku: &str, price_cents: i64) -> Result<(), Error> {
    sqlx::query("update products set price_cents = ?2 where sku = ?1")
        .bind(sku)
        .bind(price_cents.max(0))
        .execute(pool)
        .await?;
    Ok(())
}

/// Sets one variant's stock and keeps the product total in step -- the
/// product row is a cache of the sum, never the source of truth.
pub async fn set_stock(
    pool: &SqlitePool,
    sku: &str,
    size: &str,
    stock: i64,
) -> Result<(), Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("update variants set stock = ?3 where sku = ?1 and size = ?2")
        .bind(sku)
        .bind(size)
        .bind(stock.max(0))
        .execute(&mut *tx)
        .await?;
    resum_stock(&mut tx, sku).await?;
    tx.commit().await?;
    Ok(())
}

/// Walks every order that still has a rung to climb, one rung per call.
/// The tracking line is the one a visitor gets by advancing a parcel by
/// hand.
pub async fn advance_pending(pool: &SqlitePool) -> Result<u64, Error> {
    let waiting: Vec<(i64, String)> =
        sqlx::query_as("select id, status from orders where status in ('paid', 'packing', 'shipped')")
            .fetch_all(pool)
            .await?;

    let mut moved = 0;
    for (id, status) in waiting {
        let Some((next, note)) = next_step(&status) else { continue };
        let mut tx = pool.begin().await?;
        sqlx::query("update orders set status = ?2 where id = ?1")
            .bind(id)
            .bind(next)
            .execute(&mut *tx)
            .await?;
        sqlx::query("insert into tracking (order_id, step, note, at) values (?1, ?2, ?3, ?4)")
            .bind(id)
            .bind(next)
            .bind(note)
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        moved += 1;
    }
    Ok(moved)
}

// --- admin: products

/// Every product, hidden ones included: the back office has no curtain.
pub async fn all_products(pool: &SqlitePool) -> Result<Vec<Product>, Error> {
    Ok(sqlx::query_as::<_, Product>(AssertSqlSafe(format!(
        "select {PRODUCT_FIELDS} from products order by hidden, category, name"
    )))
    .fetch_all(pool)
    .await?)
}

pub async fn toggle_hidden(pool: &SqlitePool, sku: &str) -> Result<(), Error> {
    sqlx::query("update products set hidden = 1 - hidden where sku = ?1")
        .bind(sku)
        .execute(pool)
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn create_product(
    pool: &SqlitePool,
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
    let mut tx = pool.begin().await?;
    sqlx::query(
        "insert into products (sku, name, summary, detail, price_cents, stock, category, \
         is_new, material) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(sku)
    .bind(name.trim())
    .bind(summary.trim())
    .bind(detail.trim())
    .bind(price_cents.max(0))
    .bind(stock.max(0))
    .bind(category.trim())
    .bind(is_new)
    .bind(material.trim())
    .execute(&mut *tx)
    .await?;
    // The one-size row the cart reads its stock from; sizes come later.
    sqlx::query("insert into variants (sku, size, stock, rank) values (?1, '', ?2, 1)")
        .bind(sku)
        .bind(stock.max(0))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_product(
    pool: &SqlitePool,
    sku: &str,
    name: &str,
    summary: &str,
    detail: &str,
    category: &str,
    material: &str,
    is_new: i64,
) -> Result<(), Error> {
    sqlx::query(
        "update products set name = ?2, summary = ?3, detail = ?4, category = ?5, \
         material = ?6, is_new = ?7 where sku = ?1",
    )
    .bind(sku)
    .bind(name.trim())
    .bind(summary.trim())
    .bind(detail.trim())
    .bind(category.trim())
    .bind(material.trim())
    .bind(is_new)
    .execute(pool)
    .await?;
    Ok(())
}

async fn resum_stock(tx: &mut sqlx::SqliteConnection, sku: &str) -> Result<(), Error> {
    sqlx::query(
        "update products set stock = \
         (select coalesce(sum(stock), 0) from variants where sku = ?1) where sku = ?1",
    )
    .bind(sku)
    .execute(tx)
    .await?;
    Ok(())
}

pub async fn add_variant(
    pool: &SqlitePool,
    sku: &str,
    size: &str,
    stock: i64,
) -> Result<(), Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "insert into variants (sku, size, stock, rank) values (?1, ?2, ?3, \
         (select coalesce(max(rank), 0) + 1 from variants where sku = ?1)) \
         on conflict(sku, size) do update set stock = ?3",
    )
    .bind(sku)
    .bind(size.trim())
    .bind(stock.max(0))
    .execute(&mut *tx)
    .await?;
    resum_stock(&mut tx, sku).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn remove_variant(pool: &SqlitePool, sku: &str, size: &str) -> Result<(), Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("delete from variants where sku = ?1 and size = ?2")
        .bind(sku)
        .bind(size)
        .execute(&mut *tx)
        .await?;
    resum_stock(&mut tx, sku).await?;
    tx.commit().await?;
    Ok(())
}
