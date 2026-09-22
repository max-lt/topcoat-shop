//! What watches the shop from outside. No page, no shell, no session: a
//! JSON document a probe can read on either host.

use std::sync::OnceLock;

use chrono::Utc;
use serde::Serialize;
use topcoat::context::Cx;
use topcoat::router::content::Json;
use topcoat::router::route;
use topcoat::Result;

/// When this build started counting, as unix seconds.
///
/// The binary marks it before it serves, so `uptime` is the process's. A
/// Worker has no process to be up: the isolate marks it on the first
/// request it answers, and a new isolate starts the count again.
static STARTED: OnceLock<i64> = OnceLock::new();

/// Starts the uptime clock. Calling it twice keeps the first answer.
pub fn mark_start() {
    let _ = started_at();
}

fn started_at() -> i64 {
    *STARTED.get_or_init(|| Utc::now().timestamp())
}

#[derive(Serialize)]
struct Health {
    /// Empty when the build had no git to ask for it.
    commit: &'static str,
    status: &'static str,
    timestamp: i64,
    uptime: i64,
    version: &'static str,
}

#[route(GET "/api/v1/health")]
async fn health(_: &Cx) -> Result<Json<Health>> {
    let now = Utc::now().timestamp();

    Ok(Json(Health {
        commit: env!("SHOP_COMMIT"),
        status: "ok",
        timestamp: now,
        // The clock can go backwards between two reads, and a negative
        // uptime is not a thing a probe should have to parse.
        uptime: (now - started_at()).max(0),
        version: env!("CARGO_PKG_VERSION"),
    }))
}
