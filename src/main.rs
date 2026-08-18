//! The native host: tokio, SQLite, one binary, no build step beyond cargo.

use topcoat::asset::{AssetBundle, RouterBuilderAssetExt};
use topcoat::cookie::RouterBuilderCookieExt;
use topcoat::router::{BodyLimit, Router, RouterBuilderDiscoverExt};
use topcoat::session::{RouterBuilderSessionExt, SessionConfig};

use topcoat_shop::app::admin::PHOTO_LIMIT;
use topcoat_shop::{db, images};

#[tokio::main]
async fn main() {
    let assets = AssetBundle::load().unwrap();
    let pool = db::connect(&std::env::var("DATABASE_URL").unwrap_or("shop.db".into()))
        .await
        .expect("database");

    images::prewarm();
    march_parcels(pool.clone());

    let router = Router::builder()
        .discover()
        .assets(assets)
        .cookies()
        .sessions(SessionConfig::default())
        .layer(BodyLimit::max(PHOTO_LIMIT).at("/admin/photo"))
        .app_context(pool)
        .build();
    topcoat::start(router).await.unwrap();
}

/// What the Worker gets from a cron trigger, the binary gets from a task:
/// every order that still has a rung climbs one. `ADVANCE_EVERY` is the
/// period in seconds, ten minutes by default, and a demo runs it faster.
fn march_parcels(pool: sqlx::SqlitePool) {
    let period = std::env::var("ADVANCE_EVERY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(600);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(period));
        loop {
            ticker.tick().await;
            match db::advance_pending(&pool).await {
                Ok(0) => {}
                Ok(moved) => println!("{moved} commandes avancées"),
                Err(e) => eprintln!("avancement des commandes : {e}"),
            }
        }
    });
}
