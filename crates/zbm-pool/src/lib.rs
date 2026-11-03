//! Pool import for the ZFSBootMenu runtime: import policies, the retry loop of
//! the start-up stage, host IDs, and the read-write import before a change.
//!
//! A domain service of the architecture in `ROADMAP.md`: it runs
//! `zpool` through the adapters of `zbm-zfs`, keeps its findings in the state
//! directory (`zbm-state`), and leaves the console to the program that runs
//! it, through the [`Operator`] port and the `Interaction` of
//! `zbm-platform`. `zbm-startup` runs [`stage()`] as its pool import stage.
//!
//! - [`settings`]: what `/etc/zfsbootmenu.conf` says about import, as types;
//! - [`import`]: `zpool import` with typed options, and host ID matching;
//! - [`retry`]: the import loop as a state machine with pure transitions;
//! - [`mod@stage`]: the stage that drives the loop against the system;
//! - [`hostid`]: host IDs and `/etc/hostid`;
//! - [`writable`]: the read-write import before a snapshot, a rollback or
//!   a new default, with the resume question ([`resume`]) where a
//!   hibernated system may resume.
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
pub mod resume;
pub mod retry;
pub mod settings;
pub mod stage;
pub mod text;
pub mod writable;

pub use error::{POOL_CODES, PoolError, WRITABLE_CODES, WritableError};
pub use settings::{Build, ImportSettings};
pub use stage::{Imported, Late, Operator, choose_bootfs, stage};
pub use zbm_types::{BootfsPreference, HostId, PoolRequirement, RetryDelay};
