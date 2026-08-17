//! Two build steps: Tailwind, and the catalog photography.
//!
//! Tailwind's input has to be named explicitly -- the default is a generated
//! file that only imports Tailwind, which would drop our theme tokens.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    println!("cargo::rerun-if-changed=assets/site.src.css");
    println!("cargo::rerun-if-changed=assets/photos");
    println!("cargo::rerun-if-changed=src");
    // sqlx::migrate! freezes the migration list at macro expansion; a new
    // file in migrations/ must force a recompile or the binary boots blind
    // to it.
    println!("cargo::rerun-if-changed=migrations");

    embed_photos();

    topcoat::tailwind::BuildConfig::new()
        .input("assets/site.src.css")
        .render()
        .unwrap();
}

/// Compiles whatever JPEGs sit in assets/photos into the binary, keyed by
/// file stem. The directory is not part of the repository, so the list is
/// discovered here rather than written by hand; with no photos on disk the
/// shop still builds and serves its flat placeholder cards.
fn embed_photos() {
    let dir = Path::new("assets/photos");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    names.sort();

    let root = std::env::current_dir().unwrap();
    let mut code = String::from("pub static EMBEDDED: &[(&str, &[u8])] = &[\n");
    for name in &names {
        let path = root.join(dir).join(format!("{name}.jpg"));
        writeln!(code, "    ({name:?}, include_bytes!({:?})),", path.display()).unwrap();
    }
    code.push_str("];\n");

    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("photos.rs");
    std::fs::write(out, code).unwrap();
}
