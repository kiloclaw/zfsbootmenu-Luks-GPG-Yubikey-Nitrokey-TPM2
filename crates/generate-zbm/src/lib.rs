//! `generate-zbm`: build ZFSBootMenu images, the Rust port of `legacy/bin/generate-zbm`.
//!
//! The port follows the Perl program step by step and is checked against it by parity tests. It is not yet
//! complete; see Phase 3 of `PORT-ROADMAP.md`.

pub mod cli;
pub mod config;
pub mod dump;
pub mod glob;
pub mod images;
pub mod version;
