//! Images at the edge: an R2 bucket holds the originals, Cloudflare's
//! image binding does the resizing, and both are bindings of this Worker.
//!
//! `url`, `background` and `store` keep the signatures of the native
//! kitchen, so the views and the back office compile against either.

use std::cell::RefCell;
use std::rc::Rc;

use topcoat::context::Cx;
use topcoat::router::error::RouterErrorExt;
use topcoat::router::{path_param, route, Body, IntoResponse, Response};
use topcoat::Result;
use worker::js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use worker::wasm_bindgen::{JsCast, JsValue};

/// Same widths as the native kitchen; requests snap to the nearest so the
/// set of unique transformations stays countable on one hand.
const WIDTHS: [u32; 3] = [400, 900, 1600];

/// Anything heavier than the native jpeg at this quality would be a
/// regression; 80 lands webp well under it.
const WEBP_QUALITY: f64 = 80.0;

/// An upload is stored at the largest served width, as the native shop
/// stores it.
const UPLOAD_QUALITY: f64 = 85.0;

thread_local! {
    static KITCHEN: RefCell<Option<Rc<JsValue>>> = const { RefCell::new(None) };
    static PHOTOS: RefCell<Option<Rc<worker::Bucket>>> = const { RefCell::new(None) };
}

/// Keeps the bindings at hand for the request. Without IMAGES the
/// originals still serve, just heavier; without the bucket there is
/// nothing to serve and the photo route answers 404.
pub fn install(env: &worker::Env) {
    let raw = Reflect::get(env.as_ref(), &JsValue::from_str("IMAGES")).ok();
    KITCHEN.with(|k| *k.borrow_mut() = raw.filter(|v| v.is_object()).map(Rc::new));
    PHOTOS.with(|b| *b.borrow_mut() = env.bucket("PHOTOS").ok().map(Rc::new));
}

fn key_for(sku: &str) -> String {
    format!("{}.jpg", sku.to_ascii_lowercase())
}

pub fn url(sku: &str, width: u32) -> String {
    format!("/img/{sku}?w={width}")
}

/// The native shop computes real dominant colours during its pre-warm; the
/// edge has no decoder on board and lets the oat ground stand in.
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

/// The Send bridge, as in the data layer: R2 and the image binding hand
/// back JS promises, which cannot cross threads, while topcoat's handlers
/// must be Send. Wasm has exactly one thread, so the promise runs in a
/// spawn_local task and the handler awaits a oneshot receiver.
fn bridge<T, F>(work: F) -> impl std::future::Future<Output = Result<T>> + Send
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T>> + 'static,
{
    let (tx, rx) = futures_channel::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tx.send(work.await);
    });
    async move { rx.await.map_err(|_| anyhow::anyhow!("bridge closed"))? }
}

/// One pass through the image binding: bytes in, the asked width out, in
/// the asked format. Pure JS interop -- worker 0.8 has no typed wrapper
/// for IMAGES yet.
async fn cook(
    images: &JsValue,
    data: &[u8],
    width: u32,
    format: &str,
    quality: f64,
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
    let target = Object::new();
    Reflect::set(&target, &"format".into(), &format.into())?;
    Reflect::set(&target, &"quality".into(), &JsValue::from_f64(quality))?;
    let output = wasm_bindgen_futures::JsFuture::from(
        call(&transformed, "output", Some(&target.into()))?.dyn_into::<Promise>()?,
    )
    .await?;

    let response = call(&output, "response", None)?;
    let buffer = wasm_bindgen_futures::JsFuture::from(
        call(&response, "arrayBuffer", None)?.dyn_into::<Promise>()?,
    )
    .await?;
    Ok(Uint8Array::new(&buffer).to_vec())
}

/// The original as stored. Only callable from inside a spawn_local task.
async fn original(sku: &str) -> Result<Option<Vec<u8>>> {
    let Some(bucket) = PHOTOS.with(|b| b.borrow().clone()) else {
        return Ok(None);
    };
    let object =
        bucket.get(key_for(sku)).execute().await.map_err(|e| anyhow::anyhow!("r2 get: {e}"))?;
    // The body borrows the object, so the object has to outlive the read.
    let Some(found) = object else {
        return Ok(None);
    };
    let Some(body) = found.body() else {
        return Ok(None);
    };
    Ok(Some(body.bytes().await.map_err(|e| anyhow::anyhow!("r2 body: {e}"))?))
}

/// Pulls the original out of the bucket and serves it at the asked width.
/// A failed transform still has the original in hand: a photo does not go
/// dark for want of a resize.
fn served(
    sku: String,
    width: u32,
) -> impl std::future::Future<Output = Result<Option<Payload>>> + Send {
    bridge(async move {
        let Some(data) = original(&sku).await? else {
            return Ok(None);
        };
        let jpeg = |data: Vec<u8>| Payload { data, content_type: "image/jpeg".to_string() };

        let kitchen = KITCHEN.with(|k| k.borrow().clone());
        let Some(images) = kitchen.filter(|_| width < WIDTHS[2]) else {
            return Ok(Some(jpeg(data)));
        };
        match cook(&images, &data, width, "image/webp", WEBP_QUALITY).await {
            Ok(cooked) => {
                Ok(Some(Payload { data: cooked, content_type: "image/webp".to_string() }))
            }
            Err(_) => Ok(Some(jpeg(data))),
        }
    })
}

/// Takes an upload into the bucket, bounded to the largest served width.
pub fn store(sku: &str, raw: Vec<u8>) -> impl std::future::Future<Output = Result<()>> + Send {
    let key = key_for(sku);
    bridge(async move {
        // No decoder on board: the binding is what bounds an upload here.
        let kitchen = KITCHEN.with(|k| k.borrow().clone());
        let bytes = match kitchen {
            Some(images) => cook(&images, &raw, WIDTHS[2], "image/jpeg", UPLOAD_QUALITY)
                .await
                .unwrap_or(raw),
            None => raw,
        };
        let bucket = PHOTOS
            .with(|b| b.borrow().clone())
            .ok_or_else(|| anyhow::anyhow!("no PHOTOS bucket bound"))?;
        bucket.put(key, bytes).execute().await.map_err(|e| anyhow::anyhow!("r2 put: {e}"))?;
        Ok(())
    })
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
    Ok(served(sku, width).await?.ok_or_not_found()?)
}
