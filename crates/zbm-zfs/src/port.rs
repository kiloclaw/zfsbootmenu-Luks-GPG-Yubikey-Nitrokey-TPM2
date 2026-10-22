//! The ZFS port: what ZFSBootMenu asks of ZFS, whatever answers it.
//!
//! [`crate::ZfsCli`] answers through the `zfs` and `zpool` programs; tests
//! answer with `zbm_testkit::zfs::FakeZfs`, an in-memory model of pools and
//! datasets.

use zbm_types::{DatasetName, PoolName};

use crate::model::{Dataset, Pool, Property, Snapshot, Version};

/// Why a ZFS operation failed.
#[derive(Debug, thiserror::Error)]
pub enum ZfsError {
    /// The program could not be started.
    #[error("unable to run {program}: {source}")]
    Start {
        /// `zfs` or `zpool`.
        program: String,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// The program failed, with its message.
    #[error("{command} failed with status {status}: {message}")]
    Failed {
        /// The command line.
        command: String,
        /// Its exit status as the shell reports it.
        status: i32,
        /// What it wrote to standard error.
        message: String,
    },
    /// The output could not be understood.
    #[error("unexpected output of {command}: {detail}")]
    Output {
        /// The command line.
        command: String,
        /// What was wrong.
        detail: String,
    },
    /// The dataset or pool does not exist.
    #[error("{0} does not exist")]
    Missing(String),
}

/// What ZFSBootMenu asks of ZFS.
pub trait Zfs {
    /// The version of the tools and the kernel module.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zfs version` fails or prints something unexpected.
    fn version(&self) -> Result<Version, ZfsError>;

    /// The imported pools.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zpool list` fails or prints something unexpected.
    fn pools(&self) -> Result<Vec<Pool>, ZfsError>;

    /// The names of the imported pools, without their properties.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zpool list` fails or prints an invalid name.
    fn pool_names(&self) -> Result<Vec<PoolName>, ZfsError>;

    /// The paths of the devices in `pool`'s vdevs, resolved to the devices
    /// below any symbolic links, such as `/dev/dm-3`.
    ///
    /// # Errors
    ///
    /// [`ZfsError::Missing`] when the pool is not imported, and other
    /// [`ZfsError`]s when `zpool status` fails.
    fn devices(&self, pool: &PoolName) -> Result<Vec<String>, ZfsError>;

    /// Exports `pool`.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zpool export` fails, for example because the pool
    /// is busy.
    fn export(&self, pool: &PoolName) -> Result<(), ZfsError>;

    /// A property of `pool`, such as `readonly` or `feature@encryption`.
    ///
    /// # Errors
    ///
    /// [`ZfsError::Missing`] when the pool is not imported, and other
    /// [`ZfsError`]s when `zpool get` fails.
    fn pool_property(&self, pool: &PoolName, property: &str) -> Result<Property, ZfsError>;

    /// The filesystems below and including `root` (all of them when `None`),
    /// with `properties`.
    ///
    /// # Errors
    ///
    /// [`ZfsError::Missing`] when `root` does not exist, and other
    /// [`ZfsError`]s when `zfs list` fails.
    fn filesystems(
        &self,
        root: Option<&DatasetName>,
        properties: &[&str],
    ) -> Result<Vec<Dataset>, ZfsError>;

    /// The snapshots of `dataset` and its descendants.
    ///
    /// # Errors
    ///
    /// [`ZfsError::Missing`] when `dataset` does not exist, and other
    /// [`ZfsError`]s when `zfs list` fails.
    fn snapshots(&self, dataset: &DatasetName) -> Result<Vec<Snapshot>, ZfsError>;

    /// A property of `dataset`, with its source.
    ///
    /// # Errors
    ///
    /// [`ZfsError::Missing`] when the dataset does not exist, and other
    /// [`ZfsError`]s when `zfs get` fails.
    fn property(&self, dataset: &DatasetName, property: &str) -> Result<Property, ZfsError>;

    /// Sets `property` of `dataset` to `value`.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zfs set` fails.
    fn set(&self, dataset: &DatasetName, property: &str, value: &str) -> Result<(), ZfsError>;

    /// Removes the local value of `property`, so that `dataset` inherits it.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zfs inherit` fails.
    fn inherit(&self, dataset: &DatasetName, property: &str) -> Result<(), ZfsError>;
}
