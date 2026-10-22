//! Pool import for the ZFSBootMenu runtime: import policies, the retry loop of
//! the start-up stage, and host IDs.
//!
//! A domain service of the architecture in `REFACTOR-ROADMAP.md`: it runs
//! `zpool` through the adapters of `zbm-zfs`, keeps its findings in the state
//! directory (`zbm-state`), and leaves the console to the program that runs
//! it, through the [`Operator`] port. `zbm-boot`'s start-up program runs
//! [`stage()`] as its pool import stage.
//!
//! - [`settings`]: what `/etc/zfsbootmenu.conf` says about import, as types;
//! - [`import`]: `zpool import` with typed options, and host ID matching;
//! - [`retry`]: the import loop as a state machine with pure transitions;
//! - [`mod@stage`]: the stage that drives the loop against the system;
//! - [`hostid`]: host IDs and `/etc/hostid`.
//!
//! ```
//! use zbm_pool::retry::{AfterImport, Found, ImportLoop, Next};
//! use zbm_pool::ImportSettings;
//! use zbm_types::ImportPolicy;
//!
//! let strict = ImportSettings {
//!     policy: ImportPolicy::Strict,
//!     ..ImportSettings::default()
//! };
//! let mut retry = ImportLoop::new(&strict);
//! assert_eq!(
//!     retry.imported(Found::Nothing),
//!     AfterImport::Next(Next::Failed { attempt: 1 })
//! );
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod error;
pub mod hostid;
pub mod import;
pub mod retry;
pub mod settings;
pub mod stage;
pub mod text;

pub use error::PoolError;
pub use hostid::HostId;
pub use settings::{BootfsPreference, Build, ImportSettings, PoolRef, Requirement, RetryDelay};
pub use stage::{Imported, Late, Operator, choose_bootfs, stage};
