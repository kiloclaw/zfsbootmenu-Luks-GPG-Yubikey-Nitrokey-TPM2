//! Core library of the Rust port of ZFSBootMenu.
//!
//! The bootloader that ships is still the Bash runtime in `legacy/zfsbootmenu`. Code moves here one part at a time,
//! and each part must behave like the Bash code it replaces: the test suites in `tests/` are the reference.

/// Version of the Rust crates, from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_has_three_numeric_parts() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "{VERSION}");
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()), "{VERSION}");
    }
}
