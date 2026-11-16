//! Links `brush` with a GNU build ID on every toolchain, as `zbm` is, so that
//! the image check of a release (`scripts/release/debug-files.sh`) finds the
//! debug file that resolves its addresses. Many distributions' linkers add
//! one by default; this makes it certain.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    reason = "a build script reads Cargo's environment and answers on its standard output"
)]

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "linux" {
        println!("cargo::rustc-link-arg-bins=-Wl,--build-id=sha1");
    }
}
