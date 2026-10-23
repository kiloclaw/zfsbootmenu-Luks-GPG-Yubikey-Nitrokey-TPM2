//! The start-up program of the ZFSBootMenu runtime (`zfsbootmenu-init`), its
//! stages and the start-up hooks.
//!
//! A front end in the architecture of `REFACTOR-ROADMAP.md`: the multi-call
//! binary `zbm` runs it. The program is a list of [`step::Step`] values, one
//! per [`zbm_types::Stage`] ([`stages`]), run in order by
//! [`step::run_steps`]; the domain services do the work (`zbm-pool` imports
//! pools, `zbm-boot` boots). [`shell`] is the recovery shell the stages fall
//! back to, and [`preinit`] the start-up hooks that run before the program.
//!
//! ```
//! use zbm_startup::step::Rerun;
//!
//! assert_eq!(Rerun::default(), Rerun::Pending);
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod autosize;
pub mod console;
pub mod error;
pub mod preinit;
pub mod program;
pub mod shell;
pub mod stages;
pub mod step;

pub use error::StartupError;
