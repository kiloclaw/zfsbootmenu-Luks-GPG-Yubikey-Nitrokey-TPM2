//! Shared types of the Rust port of ZFSBootMenu.
//!
//! The bootloader that ships is still the Bash runtime in `legacy/zfsbootmenu`. `PORT-ROADMAP.md` plans how the
//! Rust code replaces it; whatever the order, the Rust code must behave like the Bash code it replaces, and the test
//! suites in `tests/` are the reference.
//!
//! This crate holds what every other crate needs: running external programs behind a trait that tests can replace
//! ([`exec`]), the file tests of Perl and the shell ([`fs`]), the Bash semantics the legacy code relies on
//! ([`shell`]), the release information in `/etc/zbm-release` ([`release`]) and the ZFSBootMenu version
//! ([`version`]).

pub mod exec;
pub mod fs;
pub mod getopts;
pub mod release;
pub mod shell;
pub mod version;
