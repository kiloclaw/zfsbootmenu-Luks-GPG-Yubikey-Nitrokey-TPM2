//! The domain types of ZFSBootMenu: names and paths that are checked once,
//! when they are parsed, so that no other code can hold an invalid one.
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

mod device;
mod error;
mod mapping;
mod zfs;

pub use device::DevicePath;
pub use error::{ParseError, Reason};
pub use mapping::MappingName;
pub use zfs::{DatasetName, PoolName};
