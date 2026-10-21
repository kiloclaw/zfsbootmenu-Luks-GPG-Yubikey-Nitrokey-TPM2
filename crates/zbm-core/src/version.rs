//! The ZFSBootMenu version.

/// The ZFSBootMenu version, the same as `VERSION` in `legacy/zfsbootmenu/zbm-release`.
///
/// `scripts/version.sh -u` rewrites this line, as it does for the legacy files.
pub const ZBM_VERSION: &str = "3.1.0+dev";

#[cfg(test)]
mod tests {
    use super::ZBM_VERSION;

    #[test]
    fn version_matches_the_legacy_release_file() {
        let release = include_str!("../../../legacy/zfsbootmenu/zbm-release");
        let expected = format!("VERSION=\"{ZBM_VERSION}\"");
        assert!(
            release.lines().any(|line| line == expected),
            "zbm-release does not contain {expected}"
        );
    }
}
