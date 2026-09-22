//! Two build steps: Tailwind, and what the binary knows about its own
//! build: the commit it came from and when it was made.
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
    // The commit /api/v1/health reports. HEAD moves when the branch does,
    // and the ref file when a commit lands on it.
    println!("cargo::rerun-if-changed=.git/HEAD");
    for line in std::fs::read_to_string(".git/HEAD")
        .unwrap_or_default()
        .lines()
    {
        if let Some(reference) = line.strip_prefix("ref: ") {
            println!("cargo::rerun-if-changed=.git/{reference}");
        }
    }
    println!("cargo::rustc-env=SHOP_COMMIT={}", commit());
    // When this binary was made. The build only reruns when one of the
    // inputs above changes, so the stamp belongs to the last real build
    // rather than to the last `cargo build` that found nothing to do.
    let built = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is behind 1970")
        .as_secs();
    println!("cargo::rustc-env=SHOP_BUILDTIME={built}");

    topcoat::tailwind::BuildConfig::new()
        .input("assets/site.src.css")
        .render()
        .unwrap();
}

/// The short commit hash, or an empty string where there is no git to ask:
/// a build from a tarball, or one from a directory that was never a repo.
fn commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short=6", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|hash| hash.trim().to_string())
        .unwrap_or_default()
}
