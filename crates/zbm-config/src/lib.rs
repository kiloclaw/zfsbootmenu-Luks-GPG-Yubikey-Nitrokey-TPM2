//! ZFSBootMenu's configuration: every option it reads, defined once, and the
//! parsers that read them.
//!
//! - [`spec`]: the table of options: the kernel command line, the build
//!   options, ZFS properties, crypttab options and the keys of
//!   `generate-zbm`'s configuration, each with its names, values, default and
//!   manual page entry;
//! - [`docs`]: the option sections of the manual pages, written from the
//!   table by `cargo xtask gen-docs`;
//! - [`args`]: looking options up on ZFSBootMenu's own command line, or in
//!   the files the start-up stores them in, by their specification;
//! - [`options`]: the `zbm.*` options and their defaults, as the boot image
//!   reads them at start-up, and [`origin`]: where each value came from;
//! - [`crypttab`]: the LUKS volumes to unlock and their options;
//! - [`Config`]: all of it at once, for `zbm debug config`;
//! - [`conf`]: reading `/etc/zfsbootmenu.conf`;
//! - [`parent`]: expanding `%{parent}` in `org.zfsbootmenu:commandline`.
//!
//! Command lines are split into arguments as the kernel splits them, by
//! `zbm_types::KernelCommandLine`.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests read options by name to check the lookups"
    )
)]

pub mod args;
pub mod conf;
mod config;
pub mod crypttab;
pub mod docs;
pub mod options;
pub mod origin;
pub mod parent;
pub mod spec;

pub use config::{Config, Described};
