//! Links `zbm` with a GNU build ID on every toolchain, so that its crash
//! reports name the debug file that resolves them (`cargo xtask symbolize`).
//! Many distributions' linkers add one by default; this makes it certain.

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "linux" {
        println!("cargo::rustc-link-arg-bins=-Wl,--build-id=sha1");
    }
}
