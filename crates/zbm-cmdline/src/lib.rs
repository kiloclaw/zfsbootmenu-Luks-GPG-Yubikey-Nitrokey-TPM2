//! Kernel command lines in the Rust port of ZFSBootMenu.
//!
//! This is the Rust counterpart of `zfsbootmenu-kcl.sh` and
//! `zfsbootmenu-parse-commandline.sh`:
//!
//! - [`kcl`]: splitting a command line into arguments, removing and adding
//!   arguments, and joining them again.
//! - [`args`]: looking up arguments of ZFSBootMenu's own command line
//!   (`get_zbm_arg`, `get_zbm_bool`).
//! - [`options`]: the `zbm.*` options and their defaults, as the boot image
//!   reads them at start-up.
//! - [`parent`]: expanding `%{parent}` in `org.zfsbootmenu:commandline`.
//!
//! The legacy functions pass text from one to the next through pipes, and some
//! of their results depend on that: a final argument whose quote is never
//! closed has no newline, so some later steps drop it. The functions in [`kcl`]
//! therefore take and return the same text as their Bash counterparts.

pub mod args;
pub mod kcl;
pub mod options;
pub mod parent;
