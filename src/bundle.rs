//! The static tree the edge serves, named once for both ends of the
//! contract: `static-bundle` writes these files, the Worker's head asks
//! for them. No content hash in the names, or the two sides have to agree
//! on one by hand.

/// Rooted here, relative to the crate. Workers Assets uploads it with the
/// deployment.
pub const DIRECTORY: &str = "public";

pub const STYLESHEET: &str = "/_static/site.css";

pub const SCRIPT: &str = "/_static/app.js";

/// The `@font-face` sheets. Their own `src:` urls point into
/// `/_topcoat/assets/`, where the woff2 files land under hashed names.
pub const SERIF_CSS: &str = "/_static/serif.css";
pub const SANS_CSS: &str = "/_static/sans.css";
