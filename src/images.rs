//! Product photography. Originals are compiled in from assets/photos;
//! `/img/{sku}` serves them whole. Resizing can come later without the
//! templates changing: they already ask through `url`.

use topcoat::context::Cx;
use topcoat::router::error::RouterErrorExt;
use topcoat::router::{path_param, route, Body, IntoResponse, Response};
use topcoat::Result;

// The photography is not in the repository: build.rs compiles in whatever
// sits in assets/photos, keyed by file stem, and an empty directory is a
// legitimate build. See README.
include!(concat!(env!("OUT_DIR"), "/photos.rs"));

/// The one way templates should reference a photo.
pub fn url(sku: &str, width: u32) -> String {
    format!("/img/{sku}?w={width}")
}

/// The tile ground painted while the photo travels.
pub fn background(_key: &str) -> String {
    "#f5f1e8".to_string()
}

struct Photo(&'static [u8]);

impl IntoResponse for Photo {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        Ok(Response::builder()
            .header("Content-Type", "image/jpeg")
            .header("Cache-Control", "public, max-age=3600")
            .body(Body::from(self.0))?)
    }
}

#[path_param]
struct Sku(str);

#[route(GET "/img/{sku}")]
async fn photo(cx: &Cx) -> Result<Photo> {
    // The catalog stores SKUs in uppercase, the files are lowercase.
    let sku = path_param::<Sku>(cx).to_ascii_lowercase();
    let bytes =
        EMBEDDED.iter().find(|(s, _)| *s == sku).map(|(_, b)| *b).ok_or_not_found()?;
    Ok(Photo(bytes))
}
