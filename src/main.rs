//! Bernard's shop: catalog, cart, accounts and order tracking, served by
//! Topcoat over a SQLite database. One binary, no build step beyond cargo.

use topcoat::asset::{AssetBundle, RouterBuilderAssetExt};
use topcoat::cookie::RouterBuilderCookieExt;
use topcoat::router::{Router, RouterBuilderDiscoverExt};

mod app;
mod db;
mod design;
mod images;

#[tokio::main]
async fn main() {
    let assets = AssetBundle::load().unwrap();
    let pool = db::connect(&std::env::var("DATABASE_URL").unwrap_or("shop.db".into()))
        .await
        .expect("database");

    let router = Router::builder()
        .discover()
        .assets(assets)
        .cookies()
        .app_context(pool)
        .build();
    topcoat::start(router).await.unwrap();
}
