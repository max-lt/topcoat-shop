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

    // One stylesheet rather than three: each link is a request the browser
    // waits on before it paints, and the two font sheets are a kilobyte
    // between them. The faces come first, so a rule can override them.
    let mut sheet = String::new();
    let mut script = Vec::new();
    let mut written: BTreeMap<String, usize> = BTreeMap::new();

    let mut queue: Vec<String> = referenced(&home);
    let mut seen: Vec<String> = Vec::new();
    while let Some(url) = queue.pop() {
        if seen.contains(&url) {
            continue;
        }
        seen.push(url.clone());
        let bytes = fetch(&router, &url).await;

        if url.ends_with(".css") {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            // A stylesheet pulls in the woff2 files it names.
            queue.extend(referenced(&text));
            if url.starts_with("/_topcoat/fonts/") {
                sheet.insert_str(0, &text);
            } else {
                sheet.push_str(&text);
            }
        } else if url.ends_with(".js") {
            script = bytes;
        } else {
            write(&url, &bytes);
            written.insert(url, bytes.len());
        }
    }

    assert!(!sheet.is_empty(), "the head asked for no stylesheet");
    assert!(!script.is_empty(), "the head asked for no script");
    write(bundle::STYLESHEET, sheet.as_bytes());
    write(bundle::SCRIPT, &script);
    written.insert(bundle::STYLESHEET.to_string(), sheet.len());
    written.insert(bundle::SCRIPT.to_string(), script.len());

    let _ = std::fs::remove_file(&scratch);
    for (path, size) in &written {
        println!("{path}  {size} bytes");
    }
    println!("{} files under {}/", written.len(), bundle::DIRECTORY);
}

/// Every file url an html or css document points at. The shop quotes all
/// of them, which is all the parsing this needs. Only these two prefixes:
/// the runtime's own endpoints live under `/_topcoat` too and answer POST.
fn referenced(document: &str) -> Vec<String> {
    let mut found = Vec::new();
    for prefix in ["/_topcoat/assets/", "/_topcoat/fonts/"] {
        for (index, _) in document.match_indices(prefix) {
            let rest = &document[index..];
            let end = rest.find(['"', '\'', ')', ' ']).unwrap_or(rest.len());
            let url = &rest[..end];
            if !found.iter().any(|seen| seen == url) {
                found.push(url.to_string());
            }
        }
    }
    found
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
