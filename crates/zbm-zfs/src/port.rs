//! The ZFS port: what ZFSBootMenu asks of ZFS, whatever answers it.
//!
//! [`crate::ZfsCli`] answers through the `zfs` and `zpool` programs; tests
//! answer with `zbm_testkit::zfs::FakeZfs`, an in-memory model of pools and
//! datasets.

use zbm_types::{DatasetName, PoolName, SnapshotName};

use crate::model::{Dataset, Difference, Pool, Property, Snapshot, Version};

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

zbm_types::error_codes! {
    ZfsError, ZFS_CODES;
    Start = 201 => "a ZFS program could not be started":
        "zfs or zpool could not be run: the image lacks them, or the system is out of memory or \
         processes. Rebuild the image with the ZFS utilities of the running kernel's modules.",
    Failed = 202 => "a ZFS command failed":
        "zfs or zpool ended with an error, which follows the code. The pool may be busy, missing a \
         device, or locked; zpool status in the recovery shell shows its state.",
    Output = 203 => "a ZFS command gave unexpected output":
        "ZFSBootMenu could not read what zfs or zpool printed. The ZFS utilities may be of a \
         version it does not know; report it with the output of zbm report.",
    Missing = 204 => "a dataset or pool does not exist":
        "The dataset or pool named does not exist, or its pool is not imported. Check the name, \
         for example the bootfs property or zbm.prefer.",
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

    /// The space each snapshot of `dataset` and its descendants holds on
    /// its own (`used`), in bytes.
    ///
    /// # Errors
    ///
    /// [`ZfsError::Missing`] when `dataset` does not exist, and other
    /// [`ZfsError`]s when `zfs list` fails.
    fn snapshot_space(&self, dataset: &DatasetName) -> Result<Vec<(SnapshotName, u64)>, ZfsError>;

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

    /// Sets `property` of `pool` to `value`, such as `bootfs`.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zpool set` fails, for example on a pool imported
    /// read-only.
    fn set_pool_property(
        &self,
        pool: &PoolName,
        property: &str,
        value: &str,
    ) -> Result<(), ZfsError>;

    /// A property of `snapshot`, with its source.
    ///
    /// # Errors
    ///
    /// [`ZfsError::Missing`] when the snapshot does not exist, and other
    /// [`ZfsError`]s when `zfs get` fails.
    fn snapshot_property(
        &self,
        snapshot: &SnapshotName,
        property: &str,
    ) -> Result<Property, ZfsError>;

    /// The properties set on `dataset` itself or received with it, by name,
    /// which a clone of it is given (`zfs get -s local,received all`).
    ///
    /// # Errors
    ///
    /// [`ZfsError::Missing`] when the dataset does not exist, and other
    /// [`ZfsError`]s when `zfs get` fails.
    fn own_properties(&self, dataset: &DatasetName) -> Result<Vec<(String, Property)>, ZfsError>;

    /// Takes `snapshot`.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zfs snapshot` fails, for example because it
    /// exists.
    fn create_snapshot(&self, snapshot: &SnapshotName) -> Result<(), ZfsError>;

    /// Rolls the filesystem of `snapshot` back to it, destroying the later
    /// snapshots (`zfs rollback -r`).
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zfs rollback` fails, for example because a later
    /// snapshot has clones.
    fn rollback(&self, snapshot: &SnapshotName) -> Result<(), ZfsError>;

    /// Clones `snapshot` as `target`, with `properties` set on it.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zfs clone` fails, for example because `target`
    /// exists.
    fn clone_snapshot(
        &self,
        snapshot: &SnapshotName,
        target: &DatasetName,
        properties: &[(String, String)],
    ) -> Result<(), ZfsError>;

    /// Promotes the clone `dataset`, so that it no longer depends on its
    /// origin (`zfs promote`).
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zfs promote` fails.
    fn promote(&self, dataset: &DatasetName) -> Result<(), ZfsError>;

    /// Copies `snapshot` into the new filesystem `target`, independent of
    /// it, with `properties` set on it: a raw send with its properties into
    /// a receive that does not mount it (`zfs send -p -w | zfs recv -u`),
    /// through `mbuffer` when the image has it. An encrypted snapshot stays
    /// encrypted with the same key, and `target` becomes an encryption
    /// root.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when the send or the receive fails.
    fn send_receive(
        &self,
        snapshot: &SnapshotName,
        target: &DatasetName,
        properties: &[(String, String)],
    ) -> Result<(), ZfsError>;

    /// Makes the encryption root `dataset` inherit the key of its parent's
    /// encryption root (`zfs change-key -i`), whose key must be loaded.
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zfs change-key` fails.
    fn inherit_key(&self, dataset: &DatasetName) -> Result<(), ZfsError>;

    /// The paths that differ between `from` and `to`, or the filesystem of
    /// `from` when `to` is `None`, which must be mounted then (`zfs diff -F
    /// -H`).
    ///
    /// # Errors
    ///
    /// [`ZfsError`] when `zfs diff` fails, for example because a snapshot
    /// does not exist or the filesystem is not mounted.
    fn diff(
        &self,
        from: &SnapshotName,
        to: Option<&SnapshotName>,
    ) -> Result<Vec<Difference>, ZfsError>;
}
