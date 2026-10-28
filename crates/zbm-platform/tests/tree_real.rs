//! The file tree operations on the real file system, in a temporary directory.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "the test sets up and checks real files, and a failing operation fails it"
)]

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_platform::tree::{clear_mode_bits, copy_into, remove_tree};
use zbm_platform::files::RealFiles;
use zbm_testkit::TempDir;

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn a_gnupg_home_is_copied_made_private_and_removed() {
    let dir = TempDir::new("zbm-tree");
    let source = dir.join("gnupg");
    let home = dir.join("home");
    std::fs::create_dir_all(source.join("private-keys-v1.d")).unwrap();
    std::fs::write(source.join("pubring.kbx"), "keys").unwrap();
    std::fs::write(source.join("private-keys-v1.d/a.key"), "secret").unwrap();
    std::fs::create_dir(&home).unwrap();
    let files = RealFiles;

    copy_into(&files, &source, &home).unwrap();
    clear_mode_bits(&files, &home, 0o077).unwrap();

    assert_eq!(
        std::fs::read_to_string(home.join("pubring.kbx")).unwrap(),
        "keys"
    );
    assert_eq!(mode(&home.join("pubring.kbx")) & 0o077, 0);
    assert_eq!(mode(&home.join("private-keys-v1.d")) & 0o077, 0);
    symlink(&source, home.join("link")).unwrap();
    remove_tree(&files, &home).unwrap();
    assert!(!home.exists());
    assert!(source.join("pubring.kbx").exists());
}
