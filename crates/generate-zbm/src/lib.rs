//! `generate-zbm`: build ZFSBootMenu images, the Rust port of
//! `legacy/bin/generate-zbm`.
//!
//! The port follows the Perl program step by step and is checked against it by
//! parity tests; see Phase 3 of `PORT-ROADMAP.md`. [`generate::run`] is the
//! program; the other modules hold the parts of Perl's behaviour it depends on.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod cli;
pub mod config;
pub mod dump;
pub mod efi;
pub mod generate;
pub mod glob;
pub mod gpu;
pub mod images;
pub mod kernel;
pub mod perl;
pub mod version;
