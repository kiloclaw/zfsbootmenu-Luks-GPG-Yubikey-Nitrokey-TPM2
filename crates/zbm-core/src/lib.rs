//! Shared types of the Rust port of ZFSBootMenu.
//!
//! The bootloader that ships is still the Bash runtime in `legacy/zfsbootmenu`.
//! `PORT-ROADMAP.md` plans how the Rust code replaces it; whatever the order,
//! the Rust code must behave like the Bash code it replaces, and the test
//! suites in `tests/` are the reference.
//!
//! This crate holds what every other crate needs and that does no I/O through
//! the system's ports: the command line as text ([`args`]), the file tests
//! of Perl and the shell ([`fs`]), the colours of the menu and prompts
//! ([`color`]), option parsing as `getopts` does it ([`getopts`]), the Bash
//! semantics the legacy code relies on ([`shell`]), the release information
//! in `/etc/zbm-release` ([`release`]) and the ZFSBootMenu version
//! ([`version`]). Programs, secrets and the runtime's error type are in
//! `zbm-platform`.

pub mod args;
pub mod color;
pub mod fs;
pub mod getopts;
pub mod release;
pub mod shell;
pub mod version;
