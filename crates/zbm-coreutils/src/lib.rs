//! The coreutils operations ZFSBootMenu needs, done in process instead of by
//! running `cp`, `chmod` and `rm`.
//!
//! uutils coreutils is the coreutils of ZFSBootMenu's images
//! (ROADMAP.md, decision 4). Version order, the
//! `sort -V` of uutils, comes from `uucore` and lives with the kernel
//! versions it orders, in `zbm_types::version`. The file tree operations here
//! work through the
//! [`Files`](zbm_platform::Files) port, so tests run them on a fake file
//! system and the boot runtime on the real one:
//!
//! | Here | Instead of |
//! | --- | --- |
//! | [`copy_into`] | `cp -a SOURCE/. TARGET/` |
//! | [`clear_mode_bits`] | `chmod -R go-rwx PATH` and the like |
//! | [`remove_tree`] | `rm -rf PATH` |

pub mod tree;

pub use tree::{clear_mode_bits, copy_into, remove_tree};
