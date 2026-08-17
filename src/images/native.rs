//! Product photography, read from a directory and resized on the fly, in
//! process. `PHOTOS_DIR` (default `photos/`) holds one JPEG per SKU, named
//! after it in lowercase, and the back office writes into that same
//! directory: an upload and a file dropped in by hand are one thing.
//!
//! `/img/{sku}?w=` serves three fixed widths. A width is resized once --
//! Lanczos, then JPEG at quality 80 -- and memoised for the rest of the
//! process lifetime, so the cost of a size is one CPU burst ever, not one
//! per request. The width whitelist is what makes this safe to expose: a
//! public URL parameter that triggers CPU work must not be free-form.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

use topcoat::context::Cx;
use topcoat::router::{path_param, query_params, route, Body, IntoResponse, Response};
use topcoat::Result;

/// The only widths ever produced: catalog tile, product page, original.
const WIDTHS: [u32; 3] = [400, 900, 1600];

static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var("PHOTOS_DIR").unwrap_or_else(|_| "photos".to_string()).into()
});

fn path_for(sku: &str) -> PathBuf {
    ROOT.join(format!("{}.jpg", sku.to_ascii_lowercase()))
}

static RESIZED: LazyLock<Mutex<HashMap<(String, u32), Arc<Vec<u8>>>>> =
    LazyLock::new(Default::default);

/// A fingerprint of each original, folded into the URLs the templates
/// emit: `immutable` is only honest if the URL changes with the bytes.
/// Without it, swapping a photo leaves every past visitor on the old one
/// for a year.
static FINGERPRINTS: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(Default::default);

/// The average colour of every photo, painted on the tile while the JPEG
/// travels. Filled during the pre-warm; until then `background` falls back
/// to the oat ground and nobody notices.
static COLORS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

fn fingerprint(bytes: &[u8]) -> u32 {
    // FNV-1a: not cryptographic, just sensitive to every byte.
    bytes.iter().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(*b)).wrapping_mul(0x0100_0193))
}

/// The one way templates should reference a photo.
pub fn url(sku: &str, width: u32) -> String {
    let key = sku.to_ascii_lowercase();
    let v = FINGERPRINTS.lock().unwrap().get(&key).copied().unwrap_or(0);
    format!("/img/{sku}?w={width}&v={v:08x}")
}

pub fn background(key: &str) -> String {
    COLORS
        .lock()
        .unwrap()
        .get(key.to_ascii_lowercase().as_str())
        .cloned()
        .unwrap_or_else(|| "#f5f1e8".to_string())
}

/// Renders every served width at startup, so not even the first visitor
/// after a deployment pays the cold resize. The same pass distils each
/// photo down to its average colour.
///
/// One task, sequential: warmup is background work, and decoding every
/// original at once holds tens of megabytes per image in flight -- enough
/// to get the process OOM-killed in a small VM.
pub fn prewarm() {
    tokio::task::spawn_blocking(|| {
        if let Err(e) = std::fs::create_dir_all(&*ROOT) {
            eprintln!("photos: {} is not usable: {e}", ROOT.display());
            return;
        }
        let Ok(entries) = std::fs::read_dir(&*ROOT) else { return };
        for path in entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
        {
            let Some(key) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else { continue };
            bake(&key.to_ascii_lowercase(), &bytes);
        }
    });
}

/// Distils one photo into its caches: fingerprint, average colour, served
/// widths. Blocking work -- call it from the blocking pool.
fn bake(key: &str, bytes: &[u8]) {
    FINGERPRINTS.lock().unwrap().insert(key.to_string(), fingerprint(bytes));
    let Ok(full) = image::load_from_memory(bytes) else { return };
    for width in [WIDTHS[0], WIDTHS[1]] {
        let smaller = full.resize(width, u32::MAX, image::imageops::FilterType::Lanczos3);
        if width == WIDTHS[0] {
            // Shrinking to a single pixel is the whole averaging.
            let p = smaller.resize_exact(1, 1, image::imageops::FilterType::Triangle).to_rgb8();
            let [r, g, b] = p.get_pixel(0, 0).0;
            COLORS.lock().unwrap().insert(key.to_string(), format!("#{r:02x}{g:02x}{b:02x}"));
        }
        let mut out = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
        if smaller.write_with_encoder(encoder).is_ok() {
            RESIZED.lock().unwrap().insert((key.to_string(), width), Arc::new(out));
        }
    }
}

/// Takes an upload into the directory, normalised to a JPEG no wider than
/// the largest served width.
pub async fn store(sku: &str, raw: Vec<u8>) -> Result<()> {
    let key = sku.to_ascii_lowercase();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let bytes = normalize(&raw)?;
        adopt(&key, &bytes)?;
        Ok(())
    })
    .await??;
    Ok(())
}

/// Takes a freshly normalised photo into the directory and the caches.
/// Written beside its target and renamed, so a reader never meets half a
/// JPEG. Blocking work -- call it from the blocking pool.
fn adopt(sku: &str, bytes: &[u8]) -> std::io::Result<()> {
    let key = sku.to_ascii_lowercase();
    std::fs::create_dir_all(&*ROOT)?;
    let path = path_for(&key);
    let pending = path.with_extension("pending");
    std::fs::write(&pending, bytes)?;
    std::fs::rename(&pending, &path)?;
    // The stale sizes go before the new fingerprint appears, or a fresh
    // URL could still be answered with the old bytes.
    RESIZED.lock().unwrap().retain(|(cached, _), _| cached != &key);
    bake(&key, bytes);
    Ok(())
}

/// Any upload becomes a JPEG no wider than the largest served width --
/// whatever the admin drops in, the pipeline downstream sees one format.
fn normalize(raw: &[u8]) -> image::ImageResult<Vec<u8>> {
    let full = image::load_from_memory(raw)?;
    let bounded = if full.width() > WIDTHS[2] {
        full.resize(WIDTHS[2], u32::MAX, image::imageops::FilterType::Lanczos3)
    } else {
        full
    };
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
    bounded.to_rgb8().write_with_encoder(encoder)?;
    Ok(out)
}

enum Photo {
    /// Fingerprinted URLs: the bytes behind a given v= never change.
    Served(Arc<Vec<u8>>),
    /// The flat card served while a product still waits for its photo:
    /// briefly cacheable, since the real one arrives under a new v=.
    Placeholder(Arc<Vec<u8>>),
}

impl IntoResponse for Photo {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        let (bytes, cache) = match self {
            Photo::Served(bytes) => (bytes, "public, max-age=31536000, immutable"),
            Photo::Placeholder(bytes) => (bytes, "public, max-age=60"),
        };
        Ok(Response::builder()
            .header("Content-Type", "image/jpeg")
            .header("Cache-Control", cache)
            .body(Body::from(bytes.as_ref().clone()))?)
    }
}

#[path_param]
struct Sku(str);

#[query_params(error = bad_request)]
struct Width {
    w: Option<u32>,
}

#[route(GET "/img/{sku}")]
async fn photo(cx: &Cx) -> Result<Photo> {
    // The catalog stores SKUs in uppercase, the files are lowercase.
    let sku = path_param::<Sku>(cx).to_ascii_lowercase();
    let requested = query_params::<Width>(cx)?.w.unwrap_or(900);
    let width = WIDTHS.into_iter().min_by_key(|w| w.abs_diff(requested)).unwrap();

    if let Some(bytes) = RESIZED.lock().unwrap().get(&(sku.clone(), width)) {
        return Ok(Photo::Served(bytes.clone()));
    }

    // Decode + Lanczos + encode holds a core for ~100 ms: that belongs on
    // the blocking pool, not on the reactor.
    let key = sku.clone();
    let Some(bytes) = tokio::task::spawn_blocking(move || cook(&key, width)).await? else {
        return Ok(Photo::Placeholder(blank_card()));
    };
    RESIZED.lock().unwrap().insert((sku, width), bytes.clone());
    Ok(Photo::Served(bytes))
}

/// Reads one original and produces the asked width. Blocking work.
fn cook(key: &str, width: u32) -> Option<Arc<Vec<u8>>> {
    let original = std::fs::read(path_for(key)).ok()?;
    FINGERPRINTS.lock().unwrap().insert(key.to_string(), fingerprint(&original));
    if width == WIDTHS[2] {
        return Some(Arc::new(original));
    }
    let full = image::load_from_memory(&original).ok()?;
    let smaller = full.resize(width, u32::MAX, image::imageops::FilterType::Lanczos3);
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
    smaller.write_with_encoder(encoder).ok()?;
    Some(Arc::new(out))
}

fn blank_card() -> Arc<Vec<u8>> {
    static BLANK: LazyLock<Arc<Vec<u8>>> = LazyLock::new(|| {
        let flat = image::RgbImage::from_pixel(32, 32, image::Rgb([245, 241, 232]));
        let mut out = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
        let _ = image::DynamicImage::ImageRgb8(flat).write_with_encoder(encoder);
        Arc::new(out)
    });
    BLANK.clone()
}
