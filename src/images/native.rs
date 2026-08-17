//! Product photography, resized on the fly, in process. Originals are
//! compiled in from assets/photos or uploaded to the base; `/img/{sku}?w=`
//! serves them at three fixed widths. A width is resized once -- Lanczos,
//! then JPEG at quality 80 -- and memoised for the rest of the process
//! lifetime, so the cost of a size is one CPU burst ever, not one per
//! request. The width whitelist is what makes this safe to expose: a public
//! URL parameter that triggers CPU work must not be free-form.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use topcoat::context::Cx;
use topcoat::router::{path_param, query_params, route, Body, IntoResponse, Response};
use topcoat::Result;

/// The only widths ever produced: catalog tile, product page, original.
const WIDTHS: [u32; 3] = [400, 900, 1600];

// The photography is not in the repository: build.rs compiles in whatever
// sits in assets/photos, keyed by file stem, and an empty directory is a
// legitimate build. See README.
include!(concat!(env!("OUT_DIR"), "/photos.rs"));

static RESIZED: LazyLock<Mutex<HashMap<(String, u32), Arc<Vec<u8>>>>> =
    LazyLock::new(Default::default);

/// Photos born after the binary: uploaded through the admin, stored in
/// the base, held here with their fingerprint once loaded. `/img` serves
/// them through the same resize pipeline as the embedded originals.
static UPLOADED: LazyLock<Mutex<HashMap<String, (u32, Arc<Vec<u8>>)>>> =
    LazyLock::new(Default::default);

/// A fingerprint of each original, folded into the URLs the templates
/// emit: `immutable` is only honest if the URL changes with the bytes.
/// Without it, swapping a photo leaves every past visitor on the old one
/// for a year.
static FINGERPRINTS: LazyLock<HashMap<&'static str, u32>> =
    LazyLock::new(|| EMBEDDED.iter().map(|(sku, bytes)| (*sku, fingerprint(bytes))).collect());

fn fingerprint(bytes: &[u8]) -> u32 {
    // FNV-1a: not cryptographic, just sensitive to every byte.
    bytes.iter().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(*b)).wrapping_mul(0x0100_0193))
}

/// The one way templates should reference a photo.
pub fn url(sku: &str, width: u32) -> String {
    let key = sku.to_ascii_lowercase();
    // An uploaded photo outranks the compiled-in one, fingerprint included:
    // replacing a photo from the admin must change the URL it is served at.
    let v = UPLOADED
        .lock()
        .unwrap()
        .get(&key)
        .map(|(f, _)| *f)
        .or_else(|| FINGERPRINTS.get(key.as_str()).copied())
        .unwrap_or(0);
    format!("/img/{sku}?w={width}&v={v:08x}")
}

/// The average colour of every photo, painted on the tile while the JPEG
/// travels. Filled during the pre-warm; until then `background` falls back
/// to the oat ground and nobody notices.
static COLORS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

pub fn background(key: &str) -> String {
    COLORS
        .lock()
        .unwrap()
        .get(key.to_ascii_lowercase().as_str())
        .cloned()
        .unwrap_or_else(|| "#f5f1e8".to_string())
}

/// Renders every served width on the blocking pool at startup, so not
/// even the first visitor after a deployment pays the cold resize. The
/// same pass distils each photo down to its average colour.
///
/// One task, sequential: warmup is background work, and decoding every
/// original at once holds tens of megabytes per image in flight -- enough
/// to get the process OOM-killed in a small VM.
pub fn prewarm(pool: sqlx::SqlitePool) {
    tokio::spawn(async move {
        // The uploaded photos register before any baking: fingerprints are
        // cheap, and url() and the route must prefer them from the very
        // first request -- not once a minute of warmup has passed.
        let uploaded = crate::db::all_photos(&pool).await.unwrap_or_default();
        let overridden: std::collections::HashSet<String> =
            uploaded.iter().map(|(sku, _)| sku.to_ascii_lowercase()).collect();
        {
            let mut store = UPLOADED.lock().unwrap();
            for (sku, bytes) in &uploaded {
                store.insert(
                    sku.to_ascii_lowercase(),
                    (fingerprint(bytes), Arc::new(bytes.clone())),
                );
            }
        }
        // Then the slow pass -- skipping the compiled-in photos an upload
        // has replaced: their stale sizes must never enter the caches.
        let _ = tokio::task::spawn_blocking(move || {
            for (key, bytes) in EMBEDDED {
                if !overridden.contains(*key) {
                    bake(key, bytes);
                }
            }
            for (sku, bytes) in uploaded {
                bake(&sku.to_ascii_lowercase(), &bytes);
            }
        })
        .await;
    });
}

/// Distils one photo into its caches: average colour and served widths.
/// Blocking work -- call it from the blocking pool.
fn bake(key: &str, bytes: &[u8]) {
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

/// Takes a freshly normalised photo into the uploaded store and its caches.
/// Blocking work -- call it from the blocking pool.
pub fn adopt(sku: String, bytes: Vec<u8>) {
    let sku = sku.to_ascii_lowercase();
    let v = fingerprint(&bytes);
    bake(&sku, &bytes);
    UPLOADED.lock().unwrap().insert(sku, (v, Arc::new(bytes)));
}

/// Any upload becomes a JPEG no wider than the largest served width --
/// whatever the admin drops in, the pipeline downstream sees one format.
pub fn normalize(raw: &[u8]) -> image::ImageResult<Vec<u8>> {
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
    Original(&'static [u8]),
    Resized(Arc<Vec<u8>>),
    /// The flat card served while a product still waits for its photo:
    /// briefly cacheable, since the real one arrives under a new v=.
    Placeholder(Arc<Vec<u8>>),
}

impl IntoResponse for Photo {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        let (body, cache) = match self {
            // Fingerprinted URLs: the bytes behind a given v= never change.
            Photo::Original(bytes) => (Body::from(bytes), "public, max-age=31536000, immutable"),
            Photo::Resized(bytes) => {
                (Body::from(bytes.as_ref().clone()), "public, max-age=31536000, immutable")
            }
            Photo::Placeholder(bytes) => {
                (Body::from(bytes.as_ref().clone()), "public, max-age=60")
            }
        };
        Ok(Response::builder()
            .header("Content-Type", "image/jpeg")
            .header("Cache-Control", cache)
            .body(body)?)
    }
}

/// Where a photo's full-size bytes come from, once the lookup is done.
enum Source {
    Uploaded(Arc<Vec<u8>>),
    Embedded(&'static [u8]),
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

    // An uploaded photo wins over the compiled-in one.
    let uploaded = UPLOADED.lock().unwrap().get(&sku).map(|(_, bytes)| bytes.clone());
    let source = match uploaded {
        Some(bytes) => Source::Uploaded(bytes),
        None => match EMBEDDED.iter().find(|(s, _)| *s == sku) {
            Some((_, bytes)) => Source::Embedded(bytes),
            // A product nobody has photographed yet: a flat oat card.
            None => return Ok(Photo::Placeholder(blank_card())),
        },
    };

    if width == WIDTHS[2] {
        return Ok(match source {
            Source::Uploaded(bytes) => Photo::Resized(bytes),
            Source::Embedded(bytes) => Photo::Original(bytes),
        });
    }
    if let Some(bytes) = RESIZED.lock().unwrap().get(&(sku.clone(), width)) {
        return Ok(Photo::Resized(bytes.clone()));
    }

    // Decode + Lanczos + encode holds a core for ~100 ms: that belongs on
    // the blocking pool, not on the reactor.
    let bytes = tokio::task::spawn_blocking(move || match source {
        Source::Uploaded(original) => resize(&original, width),
        Source::Embedded(original) => resize(original, width),
    })
    .await??;
    let bytes = Arc::new(bytes);
    RESIZED.lock().unwrap().insert((sku, width), bytes.clone());
    Ok(Photo::Resized(bytes))
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

fn resize(original: &[u8], width: u32) -> image::ImageResult<Vec<u8>> {
    let full = image::load_from_memory(original)?;
    let smaller = full.resize(width, u32::MAX, image::imageops::FilterType::Lanczos3);
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
    smaller.write_with_encoder(encoder)?;
    Ok(out)
}
