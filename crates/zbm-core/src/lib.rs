//! Shared types of the Rust port of ZFSBootMenu.
//!
//! The bootloader that ships is still the Bash runtime in `legacy/zfsbootmenu`.
//! `ROADMAP.md` plans how the Rust code replaces it. Behaviour users depend
//! on stays compatible; other changes are recorded in
//! `CHANGES-FROM-LEGACY.md`.
//!
//! What is left here is the Bash semantics that configuration parsing still
//! relies on ([`shell`]); ROADMAP.md's DM11 removes it, and this crate.

pub mod shell;
