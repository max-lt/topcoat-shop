//! The native host: tokio, SQLite, one binary, no build step beyond cargo.

use tokio::net::TcpListener;

use topcoat::asset::{AssetBundle, RouterBuilderAssetExt};
use topcoat::cookie::RouterBuilderCookieExt;
use topcoat::runtime::RouterBuilderRuntimeExt;
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
        .runtime()
        .discover()
        .assets(assets)
        .cookies()
        .sessions(SessionConfig::default())
        .layer(BodyLimit::max(PHOTO_LIMIT).at("/admin/photo"))
        .app_context(pool)
        .build();

    // `topcoat::start` reads HOST and PORT and binds on its own, and keeps the
    // address to itself. Binding here instead prints the port the listener
    // actually got, which PORT=0 makes the kernel choose.
    let host = std::env::var("HOST").unwrap_or("127.0.0.1".into());
    let port: u16 = std::env::var("PORT")
        .unwrap_or("3000".into())
        .parse()
        .unwrap_or_else(|e| panic!("PORT is not a port number: {e}"));
    let listener = TcpListener::bind((host.as_str(), port))
        .await
        .unwrap_or_else(|e| panic!("cannot listen on {host}:{port}: {e}"));
    let addr = listener.local_addr().expect("listener address");
    println!("listening on http://{addr}");

    topcoat::serve(listener, router).await.unwrap();
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
