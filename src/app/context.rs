//! What every page needs to know about the visitor: who they are, which
//! cart is theirs, and how full it is. Plain functions taking `cx` -- the
//! shape Topcoat prefers over middleware.

use sqlx::SqlitePool;
use topcoat::context::{app_context, Cx};
use topcoat::cookie::{cookie, cookies, Cookie, Cookies};
use topcoat::router::headers;
use topcoat::session;
use topcoat::Result;

use crate::db::{self, User};

pub const CART_COOKIE: &str = "cart";

pub fn pool(cx: &Cx) -> &SqlitePool {
    app_context::<SqlitePool>(cx)
}

/// The signed-in user, or `None`. An unknown or expired token hash counts as
/// signed out, never as an error.
pub async fn current_user(cx: &Cx) -> Result<Option<User>> {
    let Some(hash) = session::token_hash(cx).await? else {
        return Ok(None);
    };
    Ok(db::user_for_session(pool(cx), hash.as_ref()).await?)
}

/// The visitor's cart id, minted into a cookie on first sight so an
/// anonymous browser can fill a cart before it has an account.
pub fn current_cart(cx: &Cx) -> String {
    let jar = cookies(cx);
    if let Some(existing) = jar.get(CART_COOKIE) {
        return existing.value().to_string();
    }
    let id = random_id();
    jar.add(cookie! {
        "cart" = id.clone();
        Path = "/";
        HttpOnly;
        SameSite = Lax;
    });
    id
}

/// The badge in the header.
pub async fn cart_count(cx: &Cx) -> Result<i64> {
    let id = current_cart(cx);
    Ok(db::item_count(pool(cx), &id).await?)
}

/// Forgets the current cart cookie, after checkout.
pub fn forget_cart(cx: &Cx) {
    cookies(cx).remove(Cookie::build((CART_COOKIE, "")).path("/").build());
}

/// The absolute origin, for the few places that need one: feeds, the
/// sitemap, og:image. The tunnel terminates TLS, so the scheme comes from
/// X-Forwarded-Proto when it is there.
pub fn public_origin(cx: &Cx) -> String {
    let headers = headers(cx);
    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("localhost:3000");
    let scheme =
        headers.get("x-forwarded-proto").and_then(|h| h.to_str().ok()).unwrap_or("http");
    format!("{scheme}://{host}")
}

pub const SEEN_COOKIE: &str = "seen";

/// Records a product view in the seen cookie and returns what the visitor
/// had already seen, most recent first -- the "déjà regardés" shelf.
pub fn note_seen(cx: &Cx, sku: &str) -> Vec<String> {
    let jar = cookies(cx);
    let before: Vec<String> = jar
        .get(SEEN_COOKIE)
        .map(|c| c.value().split(',').filter(|s| !s.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    let others: Vec<String> = before.iter().filter(|s| s.as_str() != sku).cloned().collect();

    let mut updated = vec![sku.to_string()];
    updated.extend(others.iter().cloned());
    updated.truncate(9);
    jar.add(cookie! {
        "seen" = updated.join(",");
        Path = "/";
        HttpOnly;
        SameSite = Lax;
    });
    others
}

fn random_id() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut raw = [0u8; 24];
    rand::fill(&mut raw);
    raw.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect()
}
