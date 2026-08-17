//! One build step: Tailwind.
//!
//! Tailwind's input has to be named explicitly -- the default is a generated
//! file that only imports Tailwind, which would drop our theme tokens.

fn main() {
    println!("cargo::rerun-if-changed=assets/site.src.css");
    println!("cargo::rerun-if-changed=src");
    // sqlx::migrate! freezes the migration list at macro expansion; a new
    // file in migrations/ must force a recompile or the binary boots blind
    // to it.
    println!("cargo::rerun-if-changed=migrations");

    topcoat::tailwind::BuildConfig::new()
        .input("assets/site.src.css")
        .render()
        .unwrap();
}
