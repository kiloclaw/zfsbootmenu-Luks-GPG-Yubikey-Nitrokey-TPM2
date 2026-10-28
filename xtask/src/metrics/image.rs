//! The C programs of the Rust image (ROADMAP.md CD1): the programs the
//! lists of `legacy/zfsbootmenu/install-helpers.sh` give a Rust image,
//! but for those written in another language. Each has an item that
//! replaces it, or a reason to stay (`docs/dev/dependencies.md`); their
//! number may only go down.

use std::collections::BTreeSet;
use std::path::Path;

use crate::error::{Error, Result};

/// The file of the lists.
pub const INSTALL_HELPERS: &str = "legacy/zfsbootmenu/install-helpers.sh";

/// The row of the metrics that holds the count.
pub const ROW: &str = "rust-image";

/// The lists of programs that a Rust image gets; the lists of the legacy
/// runtime alone (`zfsbootmenu_luks_legacy_*`) are left out.
pub const LISTS: &[&str] = &[
    "zfsbootmenu_essential_binaries",
    "zfsbootmenu_optional_binaries",
    "zfsbootmenu_luks_binaries",
    "zfsbootmenu_luks_tpm2_binaries",
    "zfsbootmenu_luks_fido2_binaries",
    "zfsbootmenu_luks_clevis_binaries",
    "zfsbootmenu_luks_yubikey_binaries",
];

/// The programs of the lists that are not C: uutils coreutils, which the
/// images take their coreutils from (Rust, decision 4), and `fzf` (Go).
pub const NOT_C: &[&str] = &[
    "cat", "chmod", "chroot", "cut", "env", "head", "hostid", "mkdir", "mktemp", "od", "sort",
    "tac", "tail", "tr", "wc", "fzf",
];

/// The programs of the array `name` in the shell text `text`: the quoted
/// words between `name=(` and the next `)`.
pub fn array(text: &str, name: &str) -> Option<Vec<String>> {
    let start = text.find(&format!("\n{name}=("))? + name.len() + 3;
    let rest = text.get(start..)?;
    let body = rest.get(..rest.find(')')?)?;
    Some(
        body.split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect(),
    )
}

/// The C programs that the lists in `text` give a Rust image.
///
/// # Errors
///
/// [`Error::Format`] when a list is missing.
pub fn c_programs(text: &str) -> Result<BTreeSet<String>> {
    let mut programs = BTreeSet::new();
    for list in LISTS {
        let names = array(text, list)
            .ok_or_else(|| Error::Format(format!("{INSTALL_HELPERS}: no list {list}")))?;
        programs.extend(names);
    }
    programs.retain(|program| !NOT_C.contains(&program.as_str()));
    Ok(programs)
}

/// The C programs of the Rust image of the checkout at `root`.
///
/// # Errors
///
/// When the lists cannot be read.
pub fn count(root: &Path) -> Result<usize> {
    let path = root.join(INSTALL_HELPERS);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| Error::Format(format!("{}: {error}", path.display())))?;
    Ok(c_programs(&text)?.len())
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    #[test]
    fn the_lists_of_a_rust_image_count_their_c_programs_once() {
        let mut text = String::from("#!/bin/bash\n");
        for list in LISTS {
            let _ = write!(text, "{list}=(\n  \"zfs\"\n  \"cat\"\n)\n");
        }
        text.push_str("zfsbootmenu_luks_legacy_binaries=(\n  \"sha256sum\"\n)\n");

        let programs = c_programs(&text).unwrap();

        assert_eq!(programs.into_iter().collect::<Vec<_>>(), ["zfs"]);
        assert!(c_programs("#!/bin/bash\n").is_err());
    }

    #[test]
    fn the_checkouts_lists_are_read() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let text = std::fs::read_to_string(root.join(INSTALL_HELPERS)).unwrap();
        let programs = c_programs(&text).unwrap();
        assert!(programs.contains("cryptsetup") && programs.contains("bash"));
        assert!(!programs.contains("cat") && !programs.contains("sha256sum"));
    }
}
