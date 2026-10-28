//! The domain types of ZFSBootMenu: names, paths and choices that are checked
//! once, when they are parsed, so that no other code can hold an invalid one.
//!
//! Each type parses with [`FromStr`](std::str::FromStr), prints with
//! [`Display`](std::fmt::Display), and the two round-trip. Nothing here does
//! I/O; where a type sits in the architecture is described in
//! `docs/dev/architecture.md`.
//!
//! | Type | What it names |
//! | --- | --- |
//! | [`DevicePath`] | A block device, as an absolute path: `/dev/sda2` |
//! | [`MappingName`] | A device-mapper mapping: `luks-0b8e…` |
//! | [`PoolName`] | A ZFS pool: `zroot` |
//! | [`DatasetName`] | A ZFS dataset in a pool: `zroot/ROOT/void` |
//! | [`SnapshotName`] | A ZFS snapshot: `zroot/ROOT/void@before-upgrade` |
//! | [`KernelVersion`] | A kernel version, in `sort -V` order: `6.12.9_1` |
//! | [`ImportPolicy`] | How pools another host used are imported |
//! | [`Stage`] | A stage of the start-up program: `50-import-pools` |
//! | [`HookStage`] | A stage that runs hooks: `teardown.d` |
//! | [`Answer`] | What the user answered at a timed prompt |
//!
//! [`version`] holds the version order itself (`sort -V`).
//!
//! ```
//! use zbm_types::{DatasetName, MappingName};
//!
//! let name: MappingName = "cryptroot".parse()?;
//! assert_eq!(name.to_string(), "cryptroot");
//!
//! let dataset: DatasetName = "zroot/ROOT/void".parse()?;
//! assert_eq!(dataset.pool().as_str(), "zroot");
//! assert!("zroot/ROOT/../etc".parse::<DatasetName>().is_err());
//! # Ok::<(), zbm_types::ParseError>(())
//! ```

mod answer;
mod device;
mod error;
mod mapping;
mod policy;
mod snapshot;
mod stage;
pub mod version;
mod zfs;

pub use answer::Answer;
pub use device::DevicePath;
pub use error::{ParseError, Reason};
pub use mapping::MappingName;
pub use policy::ImportPolicy;
pub use snapshot::SnapshotName;
pub use stage::{HookStage, Stage};
pub use version::KernelVersion;
pub use zfs::{DatasetName, PoolName};
