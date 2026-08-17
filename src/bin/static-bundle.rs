//! Writes the static tree the edge deployment carries, into `public/`.
//!
//! Whatever the head asks for is answered by the shop's own router, in
//! process -- `Router::handle` is a pure function, so this needs no server
//! and no port.
//!
//! Run after `topcoat asset bundle --bin topcoat-shop`, before
//! `wrangler deploy`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use topcoat::asset::{AssetBundle, RouterBuilderAssetExt};
use topcoat::cookie::RouterBuilderCookieExt;
use topcoat::router::request::Request;
use topcoat::router::{to_bytes, Body, Router, RouterBuilderDiscoverExt, StatusCode};
use topcoat::session::{RouterBuilderSessionExt, SessionConfig};

use topcoat_shop::bundle;
use topcoat_shop::db;
use topcoat_shop::design::{SANS, SERIF};

const LIMIT: usize = 32 * 1024 * 1024;

#[tokio::main]
async fn main() {
    let assets =
        AssetBundle::load().expect("no asset bundle -- run `topcoat asset bundle` first");

    // Whatever else is left in the tree would be deployed as if it
    // belonged there.
    let _ = std::fs::remove_dir_all(bundle::DIRECTORY);

    // The home page reads the catalog before it renders a single link,
    // and the migrations seed one.
    let scratch = std::env::temp_dir().join("topcoat-shop-static-bundle.db");
    let _ = std::fs::remove_file(&scratch);
    let pool = db::connect(&scratch.to_string_lossy()).await.expect("database");

    let router = Router::builder()
        .discover()
        .assets(assets)
        .cookies()
        .sessions(SessionConfig::default())
        .app_context(pool)
        .build();

    let home = String::from_utf8(fetch(&router, "/").await).expect("the home page is utf-8");

    // A stylesheet pulls in the woff2 files, so what it points at is
    // queued in turn.
    let mut written: BTreeMap<String, usize> = BTreeMap::new();
    let mut queue: Vec<String> = referenced(&home);
    while let Some(url) = queue.pop() {
        let target = destination(&url);
        if written.contains_key(&target) {
            continue;
        }
        let bytes = fetch(&router, &url).await;
        if url.ends_with(".css") {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            queue.extend(referenced(&text));
        }
        write(&target, &bytes);
        written.insert(target, bytes.len());
    }

    for expected in [bundle::STYLESHEET, bundle::SCRIPT, bundle::SERIF_CSS, bundle::SANS_CSS] {
        assert!(written.contains_key(expected), "the head never asked for {expected}");
    }

    let _ = std::fs::remove_file(&scratch);
    for (path, size) in &written {
        println!("{path}  {size} bytes");
    }
    println!("{} files under {}/", written.len(), bundle::DIRECTORY);
}

/// Every `/_topcoat` url an html or css document points at. The shop
/// quotes all of them, which is all the parsing this needs.
fn referenced(document: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (index, _) in document.match_indices("/_topcoat/") {
        let rest = &document[index..];
        let end = rest.find(['"', '\'', ')', ' ']).unwrap_or(rest.len());
        let url = &rest[..end];
        if !found.iter().any(|seen| seen == url) {
            found.push(url.to_string());
        }
    }
    found
}

/// Where a served url lands in the tree. What the head names gets a fixed
/// name; what a stylesheet pulls in keeps the hashed path it spells.
fn destination(url: &str) -> String {
    let family_slug = |font: topcoat::font::Font| {
        font.family()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
    };

    if url.starts_with("/_topcoat/fonts/") {
        if url.contains(&family_slug(SERIF)) {
            return bundle::SERIF_CSS.to_string();
        }
        if url.contains(&family_slug(SANS)) {
            return bundle::SANS_CSS.to_string();
        }
    }
    if url.ends_with(".css") {
        return bundle::STYLESHEET.to_string();
    }
    if url.ends_with(".js") {
        return bundle::SCRIPT.to_string();
    }
    url.to_string()
}

async fn fetch(router: &Router, url: &str) -> Vec<u8> {
    let request = Request::builder().uri(url).body(Body::empty()).expect("request");
    let response = router.handle(request).await;
    assert_eq!(response.status(), StatusCode::OK, "{url} answered {}", response.status());
    to_bytes(response.into_body(), LIMIT).await.expect("body").to_vec()
}

fn write(target: &str, bytes: &[u8]) {
    let path: PathBuf = Path::new(bundle::DIRECTORY).join(target.trim_start_matches('/'));
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
    std::fs::write(&path, bytes).expect("write");
}
