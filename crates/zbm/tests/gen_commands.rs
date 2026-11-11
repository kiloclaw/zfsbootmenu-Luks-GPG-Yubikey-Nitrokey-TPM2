//! Tests of the example `gen_commands`, which `cargo xtask gen-docs` runs:
//! on a copy of `zfsbootmenu(7)` whose block "shell-commands" is stale, as
//! before a new applet, `--check` fails and writes nothing, and without it
//! the example writes the block, after which `--check` passes.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "the tests read and write the page in a scratch directory and fail loudly"
)]

#[allow(dead_code, reason = "the tests call the example's run, not its main")]
#[path = "../examples/gen_commands.rs"]
mod gen_commands;

use zbm_config::docs;
use zbm_testkit::{TempDir, repo_root};

/// The page, relative to the top of the repository.
const PAGE: &str = "docs/man/zfsbootmenu.7.rst";

/// The text of the page below `root`, the top of a repository.
fn read(root: &std::path::Path) -> String {
    std::fs::read_to_string(root.join(PAGE)).unwrap()
}

/// A scratch repository whose page lists only `zluks` as the recovery
/// shell's commands.
fn stale_repository() -> TempDir {
    let page = read(&repo_root());
    let stale = docs::splice(PAGE, &page, "shell-commands", "**zluks**\n").unwrap();
    assert_ne!(stale, page);
    let dir = TempDir::new("zbm-gen-commands");
    dir.write(PAGE, stale);
    dir
}

#[test]
fn a_stale_block_fails_the_check_and_is_left_as_it_is() {
    let dir = stale_repository();
    let before = read(dir.path());
    assert_eq!(
        gen_commands::run(dir.path(), true),
        Err(format!("{PAGE} is out of date: run `cargo xtask gen-docs`"))
    );
    assert_eq!(read(dir.path()), before);
}

#[test]
fn a_stale_block_is_written_and_then_passes_the_check() {
    let dir = stale_repository();
    assert_eq!(gen_commands::run(dir.path(), false), Ok(()));
    let page = read(dir.path());
    let expected = docs::splice(PAGE, &page, "shell-commands", &zbm::shell_commands()).unwrap();
    assert_eq!(page, expected);
    assert!(page.contains("\n**zfs-chroot**\n"));
    assert_eq!(gen_commands::run(dir.path(), true), Ok(()));
}

#[test]
fn a_page_that_cannot_be_read_is_a_problem() {
    let dir = TempDir::new("zbm-gen-commands-empty");
    let problem = gen_commands::run(dir.path(), true).unwrap_err();
    assert!(
        problem.starts_with(&format!("reading {PAGE}: ")),
        "{problem}"
    );
}
