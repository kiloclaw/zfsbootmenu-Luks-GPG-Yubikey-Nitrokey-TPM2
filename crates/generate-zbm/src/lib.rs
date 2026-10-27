//! `generate-zbm`: build ZFSBootMenu images, the Rust port of the Perl
//! `generate-zbm` of upstream ZFSBootMenu.
//!
//! The port follows the Perl program step by step, and parity tests check it
//! against the results recorded from it; see Phase 3 of `PORT-ROADMAP.md`. [`generate::run`] is the
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
