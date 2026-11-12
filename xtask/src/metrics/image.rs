//! The C programs of the Rust image (ROADMAP.md CD1): the programs the
//! lists of `crates/zbm-host/src/image/lists.rs` give a Rust image,
//! but for those written in another language. Each has an item that
//! replaces it, or a reason to stay (`docs/dev/dependencies.md`); their
//! number may only go down.

use std::collections::BTreeSet;
use std::path::Path;

use crate::error::{Error, Result};

/// The file of the lists.
pub const LISTS_FILE: &str = "crates/zbm-host/src/image/lists.rs";

/// The row of the metrics that holds the count.
pub const ROW: &str = "rust-image";

/// The lists of programs that a Rust image gets; the lists of the legacy
/// runtime alone (`LEGACY_BINARIES`, `LUKS_LEGACY_*`) are left out.
pub const LISTS: &[&str] = &[
    "ESSENTIAL_BINARIES",
    "OPTIONAL_BINARIES",
    "LUKS_BINARIES",
    "LUKS_TPM2_BINARIES",
    "LUKS_FIDO2_BINARIES",
    "LUKS_CLEVIS_BINARIES",
    "LUKS_YUBIKEY_BINARIES",
];

/// The programs of the lists that are not C: uutils coreutils, which the
/// images take their coreutils from (Rust, decision 4).
pub const NOT_C: &[&str] = &[
    "cat", "chmod", "chroot", "cut", "env", "head", "hostid", "mkdir", "mktemp", "od", "sort",
    "tac", "tail", "tr", "wc",
];

/// The programs of the list `name` in the Rust text `text`: the quoted
/// words between `pub const NAME: &[&str] = &[` and the next `]`.
pub fn array(text: &str, name: &str) -> Option<Vec<String>> {
    let head = format!("pub const {name}: &[&str] = &[");
    let start = text.find(&head)? + head.len();
    let rest = text.get(start..)?;
    let body = rest.get(..rest.find(']')?)?;
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
            .ok_or_else(|| Error::Format(format!("{LISTS_FILE}: no list {list}")))?;
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
    let path = root.join(LISTS_FILE);
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
        let mut text = String::from("//! Lists.\n");
        for list in LISTS {
            let _ = write!(
                text,
                "pub const {list}: &[&str] = &[\n    \"zfs\", \"cat\",\n];\n"
            );
        }
        text.push_str("pub const LUKS_LEGACY_BINARIES: &[&str] = &[\"sha256sum\"];\n");

        let programs = c_programs(&text).unwrap();

        assert_eq!(programs.into_iter().collect::<Vec<_>>(), ["zfs"]);
        assert!(c_programs("//! None.\n").is_err());
    }

    #[test]
    fn the_checkouts_lists_are_read() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let text = std::fs::read_to_string(root.join(LISTS_FILE)).unwrap();
        let programs = c_programs(&text).unwrap();
        assert!(programs.contains("cryptsetup") && programs.contains("bash"));
        assert!(!programs.contains("cat") && !programs.contains("sha256sum"));
    }

    #[test]
    fn fzf_is_no_program_of_a_rust_image_and_not_counted() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let text = std::fs::read_to_string(root.join(LISTS_FILE)).unwrap();
        let rust: Vec<String> = LISTS
            .iter()
            .flat_map(|list| array(&text, list).unwrap())
            .collect();
        // `fzf` is the legacy runtime's alone (ADR 0018, point 21).
        assert!(!rust.contains(&"fzf".to_owned()), "{rust:?}");
        assert_eq!(array(&text, "LEGACY_BINARIES").unwrap(), ["fzf"]);
        assert!(!c_programs(&text).unwrap().contains("fzf"));
        // Every program set aside is one a Rust image gets.
        for program in NOT_C {
            assert!(rust.contains(&(*program).to_owned()), "{program}");
        }
    }
}
