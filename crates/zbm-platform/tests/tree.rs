//! Tests of the file tree operations on the fake file system.
//!
//! They are integration tests because the fakes of `zbm-testkit` implement
//! the traits of this crate as other crates see it.

#![allow(
    clippy::unwrap_used,
    reason = "tests fail loudly when a step cannot be done"
)]

use std::io;
use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeFiles;

use zbm_platform::files::Files as _;
use zbm_platform::tree::{MAX_DEPTH, clear_mode_bits, copy_into, remove_tree};

fn mode(files: &FakeFiles, path: &str) -> u32 {
    files.metadata(Path::new(path)).unwrap().mode
}

#[test]
fn copying_keeps_modes_and_replaces_files() {
    let files = FakeFiles::new();
    files
        .file_with_mode("/etc/gnupg/pubring.kbx", "keys", 0o644)
        .file_with_mode("/etc/gnupg/private-keys-v1.d/a.key", "secret", 0o600)
        .socket("/etc/gnupg/S.gpg-agent")
        .symlink("/etc/gnupg/link.kbx", "/etc/gnupg/pubring.kbx")
        .file("/run/home/pubring.kbx", "old")
        .dir("/run/home/private-keys-v1.d");

    copy_into(&files, Path::new("/etc/gnupg"), Path::new("/run/home")).unwrap();

    assert_eq!(files.text("/run/home/pubring.kbx").as_deref(), Some("keys"));
    assert_eq!(files.text("/run/home/link.kbx").as_deref(), Some("keys"));
    assert_eq!(
        files.text("/run/home/private-keys-v1.d/a.key").as_deref(),
        Some("secret")
    );
    assert_eq!(mode(&files, "/run/home/private-keys-v1.d/a.key"), 0o600);
    assert_eq!(mode(&files, "/run/home/private-keys-v1.d"), 0o755);
    assert!(!files.exists(Path::new("/run/home/S.gpg-agent")));
}

#[test]
fn copying_stops_at_the_first_error() {
    let files = FakeFiles::new();
    files
        .file("/src/a", "a")
        .dir("/src/sub")
        .fail("/dst/sub", io::ErrorKind::PermissionDenied);
    files.dir("/dst");

    let error = copy_into(&files, Path::new("/src"), Path::new("/dst")).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(copy_into(&files, Path::new("/missing"), Path::new("/dst")).is_err());
}

#[test]
fn link_loops_and_deep_trees_end_with_an_error() {
    let files = FakeFiles::new();
    files
        .dir("/loop/dir")
        .symlink("/loop/dir/again", "/loop/dir");
    files.dir("/dst");
    assert!(copy_into(&files, Path::new("/loop"), Path::new("/dst")).is_err());
    assert!(clear_mode_bits(&files, Path::new("/loop"), 0o077).is_err());

    let deep = format!("/deep{}", "/d".repeat(MAX_DEPTH + 2));
    files.dir(&deep);
    let error = copy_into(&files, Path::new("/deep"), Path::new("/dst")).unwrap_err();
    assert!(
        error.to_string().contains("more than 64 directories deep"),
        "{error}"
    );
    let error = clear_mode_bits(&files, Path::new("/deep"), 0o077).unwrap_err();
    assert!(
        error.to_string().contains("more than 64 directories deep"),
        "{error}"
    );
}

#[test]
fn mode_bits_are_cleared_on_the_whole_tree() {
    let files = FakeFiles::new();
    files
        .file_with_mode("/home/a", "", 0o644)
        .file_with_mode("/home/sub/b", "", 0o755);

    clear_mode_bits(&files, Path::new("/home"), 0o077).unwrap();

    assert_eq!(mode(&files, "/home"), 0o700);
    assert_eq!(mode(&files, "/home/a"), 0o600);
    assert_eq!(mode(&files, "/home/sub"), 0o700);
    assert_eq!(mode(&files, "/home/sub/b"), 0o700);
    assert!(clear_mode_bits(&files, Path::new("/missing"), 0o077).is_err());
}

#[test]
fn trees_are_removed_with_their_links_but_not_link_targets() {
    let files = FakeFiles::new();
    files
        .file("/home/a", "")
        .file("/home/sub/b", "")
        .file("/keep/c", "")
        .symlink("/home/link", "/keep");

    remove_tree(&files, Path::new("/home")).unwrap();

    assert!(!files.exists(Path::new("/home")));
    assert!(files.exists(Path::new("/keep/c")));
    remove_tree(&files, Path::new("/home")).unwrap();
    files.file("/file", "");
    remove_tree(&files, Path::new("/file")).unwrap();
    assert!(!files.exists(Path::new("/file")));
}

#[test]
fn removal_goes_on_after_a_failure() {
    let files = FakeFiles::new();
    files
        .file("/home/a", "")
        .file("/home/z", "")
        .fail("/home/a", io::ErrorKind::PermissionDenied);

    let error = remove_tree(&files, Path::new("/home")).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(!files.exists(Path::new("/home/z")));
    files.fail("/locked", io::ErrorKind::PermissionDenied);
    assert!(remove_tree(&files, Path::new("/locked")).is_err());
}
