//! The native host: tokio, SQLite, one binary, no build step beyond cargo.

use topcoat::asset::{AssetBundle, RouterBuilderAssetExt};
use topcoat::cookie::RouterBuilderCookieExt;
use topcoat::router::{Router, RouterBuilderDiscoverExt};
use topcoat::session::{RouterBuilderSessionExt, SessionConfig};

use topcoat_shop::{db, images};

#[tokio::main]
async fn main() {
    let assets = AssetBundle::load().unwrap();
    let pool = db::connect(&std::env::var("DATABASE_URL").unwrap_or("shop.db".into()))
        .await
        .expect("database");

    images::prewarm();

    let router = Router::builder()
        .discover()
        .assets(assets)
        .cookies()
        .sessions(SessionConfig::default())
        .app_context(pool)
        .build();
    topcoat::start(router).await.unwrap();
}
