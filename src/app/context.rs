//! What every page needs to know about the visitor. Plain functions
//! taking `cx` -- the shape Topcoat prefers over middleware.

use sqlx::SqlitePool;
use topcoat::context::{app_context, Cx};

pub fn pool(cx: &Cx) -> &SqlitePool {
    app_context::<SqlitePool>(cx)
}
