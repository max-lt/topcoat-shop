//! The static tree the edge serves, named once for both ends of the
//! contract: `static-bundle` writes these files, the Worker's head asks
//! for them. No content hash in the names, or the two sides have to agree
//! on one by hand.

/// Rooted here, relative to the crate. Workers Assets uploads it with the
/// deployment.
pub const DIRECTORY: &str = "public";

/// The `@font-face` sheets and Tailwind's output in one file: a browser
/// waits on every stylesheet in the head before it paints. The faces name
/// woff2 files under `/_topcoat/assets/`, which land there too.
pub const STYLESHEET: &str = "/_static/site.css";

/// Topcoat's client runtime.
pub const SCRIPT: &str = "/_static/app.js";
