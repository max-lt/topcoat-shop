//! Images at the edge: the shell keeps the originals, the IMAGES
//! binding does the resizing here. The worker pulls the full-size
//! photo once, hands it to Cloudflare's kitchen, serves webp. When the
//! binding is missing (or a transform fails) it falls back to plain
//! proxying, so images never go dark. `url` and `background` keep their
//! signatures so the views compile unchanged.

use std::cell::RefCell;
use std::rc::Rc;

use topcoat::context::Cx;
use topcoat::router::error::RouterErrorExt;
use topcoat::router::{path_param, route, Body, IntoResponse, Response};
use topcoat::Result;
use worker::js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use worker::wasm_bindgen::{JsCast, JsValue};

pub const ORIGIN: &str = "https://shop.arq.pw";

/// Same widths as the shell's kitchen; requests snap to the nearest so
/// the set of unique transformations stays countable on one hand.
const WIDTHS: [u32; 3] = [400, 900, 1600];

/// Anything heavier than the shell's jpeg at this quality would be a
/// regression; 80 lands webp well under it.
const WEBP_QUALITY: f64 = 80.0;

thread_local! {
    static KITCHEN: RefCell<Option<Rc<JsValue>>> = const { RefCell::new(None) };
}

/// Keeps the IMAGES binding at hand for the request. Absent binding is
/// fine: the routes fall back to proxying the shell's kitchen.
pub fn install(env: &worker::Env) {
    let raw = Reflect::get(env.as_ref(), &JsValue::from_str("IMAGES")).ok();
    KITCHEN.with(|k| *k.borrow_mut() = raw.filter(|v| v.is_object()).map(Rc::new));
}

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

/// One pass through the binding: bytes in, webp of the asked width out.
/// Pure JS interop -- worker 0.8 has no typed wrapper for IMAGES yet.
async fn cook(
    images: &JsValue,
    data: &[u8],
    width: u32,
) -> std::result::Result<Vec<u8>, JsValue> {
    let call = |on: &JsValue, name: &str, arg: Option<&JsValue>| {
        let f: Function = Reflect::get(on, &name.into())?.dyn_into()?;
        match arg {
            Some(a) => f.call1(on, a),
            None => f.call0(on),
        }
    };

    // input() wants a byte stream; a throwaway Response supplies one.
    let response_ctor: Function =
        Reflect::get(&worker::js_sys::global(), &"Response".into())?.dyn_into()?;
    let carrier =
        Reflect::construct(&response_ctor, &Array::of1(&Uint8Array::from(data).into()))?;
    let stream = Reflect::get(&carrier, &"body".into())?;

    let input = call(images, "input", Some(&stream))?;
    let size = Object::new();
    Reflect::set(&size, &"width".into(), &JsValue::from_f64(f64::from(width)))?;
    let transformed = call(&input, "transform", Some(&size.into()))?;
    let format = Object::new();
    Reflect::set(&format, &"format".into(), &"image/webp".into())?;
    Reflect::set(&format, &"quality".into(), &JsValue::from_f64(WEBP_QUALITY))?;
    let output = wasm_bindgen_futures::JsFuture::from(
        call(&transformed, "output", Some(&format.into()))?.dyn_into::<Promise>()?,
    )
    .await?;

    let response = call(&output, "response", None)?;
    let buffer = wasm_bindgen_futures::JsFuture::from(
        call(&response, "arrayBuffer", None)?.dyn_into::<Promise>()?,
    )
    .await?;
    Ok(Uint8Array::new(&buffer).to_vec())
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

/// Fetches the full-size photo from the shell and cooks it here when
/// the binding is on board; otherwise the shell's kitchen serves, as
/// before. Same Send bridge as everything JS-side.
fn cooked_photo(
    sku: String,
    width: u32,
) -> impl std::future::Future<Output = Result<Option<Payload>>> + Send {
    let (tx, rx) = futures_channel::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let kitchen = KITCHEN.with(|k| k.borrow().clone());
        let result: Result<Option<Payload>> = async {
            // No binding, or the largest width asked: plain proxy.
            let Some(images) = kitchen.filter(|_| width < WIDTHS[2]) else {
                return fetch_origin(&format!("/img/{sku}?w={width}")).await;
            };
            let Some(original) = fetch_origin(&format!("/img/{sku}?w={}", WIDTHS[2])).await?
            else {
                return Ok(None);
            };
            match cook(&images, &original.data, width).await {
                Ok(data) => {
                    Ok(Some(Payload { data, content_type: "image/webp".to_string() }))
                }
                // A failed transform still has the original in hand.
                Err(_) => Ok(Some(original)),
            }
        }
        .await;
        let _ = tx.send(result);
    });
    async move { rx.await.map_err(|_| anyhow::anyhow!("bridge closed"))? }
}

#[path_param]
struct Sku(str);

#[route(GET "/img/{sku}")]
async fn photo(cx: &Cx) -> Result<Payload> {
    let sku = path_param::<Sku>(cx).to_string();
    let requested: u32 = topcoat::router::uri(cx)
        .query()
        .and_then(|q| {
            q.split('&').find_map(|p| p.strip_prefix("w=").and_then(|v| v.parse().ok()))
        })
        .unwrap_or(900);
    let width = WIDTHS.into_iter().min_by_key(|w| w.abs_diff(requested)).unwrap();
    Ok(cooked_photo(sku, width).await?.ok_or_not_found()?)
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
