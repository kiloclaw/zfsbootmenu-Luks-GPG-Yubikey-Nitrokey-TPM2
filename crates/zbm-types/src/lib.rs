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
//! | [`FilesystemName`] | A dataset or a snapshot, mounted to find kernels |
//! | [`KernelVersion`] | A kernel version, in `sort -V` order: `6.12.9_1` |
//! | [`KernelArg`] | One argument of a kernel command line: `root=zfs:zroot` |
//! | [`KernelCommandLine`] | A kernel command line, split as the kernel does |
//! | [`KernelSelector`] | The kernel to boot, as a regular expression: `6\.6` |
//! | [`ImportPolicy`] | How pools another host used are imported |
//! | [`Stage`] | A stage of the start-up program: `50-import-pools` |
//! | [`HookStage`] | A stage that runs hooks: `teardown.d` |
//! | [`Answer`] | What the user answered at a timed prompt |
//! | [`HostId`] | A host ID, as eight or more hex digits: `00bab10c` |
//! | [`Timeout`] | How long the menu waits: `10` seconds, or `-1` until a key |
//! | [`RetryDelay`] | The seconds between retries: `5` |
//! | [`LogLevel`] | The log level of ZFSBootMenu's messages, at least `4` |
//! | [`PoolRequirement`] | How much `zbm.prefer`'s pool is required |
//! | [`BootfsPreference`] | The boot environment `zbm.prefer` names, if any |
//!
//! [`version`] holds the version order itself (`sort -V`), and [`number`]
//! integers as the shell's `test` reads them.
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
mod cmdline;
mod device;
mod ere;
mod error;
mod filesystem;
mod hostid;
mod mapping;
pub mod number;
mod policy;
mod preference;
mod selector;
mod settings;
mod snapshot;
mod stage;
pub mod version;
mod zfs;

pub use answer::Answer;
pub use cmdline::{KernelArg, KernelCommandLine};
pub use device::DevicePath;
pub use error::{ParseError, Reason};
pub use filesystem::FilesystemName;
pub use hostid::{HostId, scrape_hostid};
pub use mapping::MappingName;
pub use policy::ImportPolicy;
pub use preference::{BootfsPreference, PoolRequirement};
pub use selector::KernelSelector;
pub use settings::{LogLevel, RetryDelay, Timeout};
pub use snapshot::SnapshotName;
pub use stage::{HookStage, Stage};
pub use version::KernelVersion;
pub use zfs::{DatasetName, PoolName};
