//! Images at the edge, proxy edition: the native shop keeps the kitchen,
//! this worker forwards and lets the CDN cache. `url` and `background`
//! keep their signatures so the views compile unchanged.

use topcoat::context::Cx;
use topcoat::router::error::RouterErrorExt;
use topcoat::router::{path_param, route, Body, IntoResponse, Response};
use topcoat::Result;

pub const ORIGIN: &str = "https://shop.arq.pw";

pub fn url(sku: &str, width: u32) -> String {
    format!("/img/{sku}?w={width}")
}

/// The shell computes real dominant colours; the edge falls back to the
/// oat ground and lets the photos arrive.
pub fn background(_key: &str) -> String {
    "#f5f1e8".to_string()
}

pub struct Payload {
    data: Vec<u8>,
    content_type: String,
}

impl IntoResponse for Payload {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        Ok(Response::builder()
            .header("Content-Type", self.content_type)
            .header("Cache-Control", "public, max-age=3600")
            .body(Body::from(self.data))?)
    }
}

/// The bare JS-side fetch against the shell. Only callable from inside
/// a spawn_local task; the bridges below wrap it for handlers.
async fn fetch_origin(path: &str) -> Result<Option<Payload>> {
    let mut response = worker::Fetch::Url(
        format!("{ORIGIN}{path}")
            .parse()
            .map_err(|e| anyhow::anyhow!("url: {e}"))?,
    )
    .send()
    .await
    .map_err(|e| anyhow::anyhow!("origin: {e}"))?;
    if response.status_code() != 200 {
        return Ok(None);
    }
    let content_type = response
        .headers()
        .get("content-type")
        .ok()
        .flatten()
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let data = response.bytes().await.map_err(|e| anyhow::anyhow!("bytes: {e}"))?;
    Ok(Some(Payload { data, content_type }))
}

/// Same Send bridge as the data layer: the JS fetch runs in a
/// spawn_local task, the handler awaits plain bytes.
fn proxied(path: String) -> impl std::future::Future<Output = Result<Option<Payload>>> + Send {
    let (tx, rx) = futures_channel::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tx.send(fetch_origin(&path).await);
    });
    async move { rx.await.map_err(|_| anyhow::anyhow!("bridge closed"))? }
}

#[path_param]
struct Sku(str);

#[route(GET "/img/{sku}")]
async fn photo(cx: &Cx) -> Result<Payload> {
    let sku = path_param::<Sku>(cx).to_string();
    let query = topcoat::router::uri(cx).query().unwrap_or("").to_string();
    Ok(proxied(format!("/img/{sku}?{query}")).await?.ok_or_not_found()?)
}

#[path_param]
struct File(str);

#[route(GET "/_topcoat/assets/{file}")]
async fn asset(cx: &Cx) -> Result<Payload> {
    let file = path_param::<File>(cx).to_string();
    Ok(proxied(format!("/_topcoat/assets/{file}")).await?.ok_or_not_found()?)
}

#[route(GET "/_topcoat/fonts/{file}")]
async fn font(cx: &Cx) -> Result<Payload> {
    let file = path_param::<File>(cx).to_string();
    Ok(proxied(format!("/_topcoat/fonts/{file}")).await?.ok_or_not_found()?)
}
