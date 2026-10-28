//! [`ZfsText`]: what booting, the start-up and `zbm-kcl` ask ZFS, as typed
//! calls. Each runs the text-mode command of the legacy runtime, which every
//! OpenZFS version answers the same way; the menu reads ZFS through the
//! [`Zfs`](crate::Zfs) port instead.

use std::borrow::Cow;
use std::ffi::OsString;
use std::io;

use zbm_config::parent::CommandLineSource;
use zbm_config::spec::properties::COMMANDLINE;
use zbm_platform::exec::{Output, Runner};
use zbm_types::{DatasetName, FilesystemName, PoolName, SnapshotName};

use crate::import::Import;
use crate::{Stderr, ZfsCommand, ZpoolCommand};

/// A name `zfs` takes: a dataset, a snapshot, or either.
pub trait ZfsName {
    /// The name as `zfs` takes it.
    fn zfs_name(&self) -> Cow<'_, str>;
}

impl ZfsName for DatasetName {
    fn zfs_name(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.as_str())
    }
}

impl ZfsName for SnapshotName {
    fn zfs_name(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

impl ZfsName for FilesystemName {
    fn zfs_name(&self) -> Cow<'_, str> {
        match self {
            FilesystemName::Dataset(dataset) => dataset.zfs_name(),
            FilesystemName::Snapshot(snapshot) => snapshot.zfs_name(),
        }
    }
}

/// The exit status of `run` as the shell reports it: 126 when the program
/// could not start.
fn shell_code(run: &io::Result<Output>) -> i32 {
    run.as_ref()
        .map_or(126, |output| output.status.shell_code())
}

/// The standard output of `run`, empty when the program could not start.
fn stdout(run: io::Result<Output>) -> String {
    run.map(|output| output.stdout_text()).unwrap_or_default()
}

/// The standard output of `run` without its trailing newlines, when the
/// program succeeded.
fn value(run: io::Result<Output>) -> Option<String> {
    let output = run.ok()?;
    output
        .status
        .is_success()
        .then(|| output.stdout_text().trim_end_matches('\n').to_owned())
}

/// ZFS through the text commands of `zfs` and `zpool`, run by a [`Runner`].
pub struct ZfsText<'a> {
    zfs: ZfsCommand<'a>,
    zpool: ZpoolCommand<'a>,
}

impl<'a> ZfsText<'a> {
    /// `zfs` and `zpool` from `PATH`.
    pub fn new(runner: &'a dyn Runner) -> ZfsText<'a> {
        ZfsText::with_programs(runner, "zfs", "zpool")
    }

    /// Other programs in their place, such as the scripts of a test.
    pub fn with_programs(
        runner: &'a dyn Runner,
        zfs: impl Into<OsString>,
        zpool: impl Into<OsString>,
    ) -> ZfsText<'a> {
        ZfsText {
            zfs: ZfsCommand::with_program(runner, zfs),
            zpool: ZpoolCommand::with_program(runner, zpool),
        }
    }

    /// The value of `property` of `name` (`zfs get -H -o value`), when
    /// `zfs` succeeds; its messages go where `errors` says.
    pub fn value(&self, name: &impl ZfsName, property: &str, errors: Stderr) -> Option<String> {
        let name = name.zfs_name();
        value(
            self.zfs
                .text(&["get", "-H", "-o", "value", property, &name], errors),
        )
    }

    /// Whether `dataset` exists (`zfs list -o name -H`, quietly).
    pub fn exists(&self, dataset: &DatasetName) -> bool {
        self.zfs.exists(dataset.as_str())
    }

    /// Whether `dataset` is a filesystem (`zfs list -t filesystem`,
    /// quietly).
    pub fn is_filesystem(&self, dataset: &DatasetName) -> bool {
        self.zfs.is_filesystem(dataset.as_str())
    }

    /// Sets `property` of `dataset` to `value` (`zfs set`); whether `zfs`
    /// succeeded.
    ///
    /// # Errors
    ///
    /// When `zfs` cannot be run.
    pub fn set(&self, dataset: &DatasetName, property: &str, value: &str) -> io::Result<bool> {
        self.zfs.set(property, value, dataset.as_str())
    }

    /// Removes the local value of `property` of `dataset` (`zfs inherit`);
    /// whether `zfs` succeeded.
    ///
    /// # Errors
    ///
    /// When `zfs` cannot be run.
    pub fn inherit(&self, dataset: &DatasetName, property: &str) -> io::Result<bool> {
        self.zfs.inherit(property, dataset.as_str())
    }

    /// Loads the key of the encryption root `root` from `location` (`zfs
    /// load-key -L`) on the console, where `zfs` asks for a passphrase
    /// itself; whether the key is loaded.
    ///
    /// # Errors
    ///
    /// The error that kept `zfs` from starting.
    pub fn load_key(&self, root: &DatasetName, location: &str) -> io::Result<bool> {
        self.zfs.load_key(root.as_str(), location)
    }

    /// The value of the pool property `property` of `pool` (`zpool get -H
    /// -o value`), when `zpool` succeeds; its messages go where `errors`
    /// says.
    pub fn pool_value(&self, pool: &PoolName, property: &str, errors: Stderr) -> Option<String> {
        value(self.zpool.text(
            &["get", "-H", "-o", "value", property, pool.as_str()],
            errors,
        ))
    }

    /// The `columns` of every imported pool (`zpool list -H -o`): one line
    /// per pool, fields separated by tabs; what `zpool` printed even when it
    /// failed, nothing when it could not start.
    pub fn pools_list(&self, columns: &[&str], errors: Stderr) -> String {
        let columns = columns.join(",");
        stdout(self.zpool.text(&["list", "-H", "-o", &columns], errors))
    }

    /// The `columns` of `pool`, or of every imported pool after `--` when
    /// `None`, as [`ZfsText::pools_list`] gives them.
    pub fn pool_list(&self, pool: Option<&PoolName>, columns: &[&str], errors: Stderr) -> String {
        let columns = columns.join(",");
        let pool = pool.map_or("--", PoolName::as_str);
        stdout(
            self.zpool
                .text(&["list", "-H", "-o", &columns, pool], errors),
        )
    }

    /// The imported pools (`zpool list -H -o name`).
    pub fn imported(&self, errors: Stderr) -> Vec<PoolName> {
        self.pools_list(&["name"], errors)
            .lines()
            .filter_map(|line| line.trim_matches([' ', '\t']).parse().ok())
            .collect()
    }

    /// Every property of every imported pool, `name` and `property` (`zpool
    /// get all -H -o name,property`): what `zpool` printed.
    pub fn pool_properties(&self, errors: Stderr) -> String {
        stdout(
            self.zpool
                .text(&["get", "all", "-H", "-o", "name,property"], errors),
        )
    }

    /// Exports `pool` (`zpool export`), its messages on the console; the
    /// exit status as the shell reports it.
    pub fn export(&self, pool: &PoolName) -> i32 {
        shell_code(&self.zpool.text(&["export", pool.as_str()], Stderr::Console))
    }

    /// Runs `import`: the exit status as the shell reports it, and what
    /// `zpool` printed, its messages included.
    pub fn import(&self, import: Import<'_>) -> (i32, String) {
        match self.zpool.text(&import.arguments(), Stderr::Merged) {
            Ok(output) => (output.status.shell_code(), output.stdout_text()),
            Err(error) => (126, error.to_string()),
        }
    }

    /// What `zpool import` says of the pools that could be imported.
    pub fn importable(&self) -> String {
        stdout(self.zpool.text(&["import"], Stderr::Hidden))
    }

    /// Tries `pool` read-only (`zpool import -o readonly=on -N`), for what
    /// `zpool` says when another host owns it: its output, messages
    /// included.
    pub fn probe_import(&self, pool: &PoolName) -> String {
        stdout(self.zpool.text(
            &["import", "-o", "readonly=on", "-N", pool.as_str()],
            Stderr::Merged,
        ))
    }

    /// Tries the pool with the numeric ID `id` (`zpool import -N`), for why
    /// it cannot be imported: its output, messages included.
    pub fn probe_import_id(&self, id: &str) -> String {
        stdout(self.zpool.text(&["import", "-N", id], Stderr::Merged))
    }

    /// The status of the imported pools, with device paths (`zpool status
    /// -P`).
    pub fn status(&self) -> String {
        stdout(self.zpool.text(&["status", "-P"], Stderr::Hidden))
    }
}

impl CommandLineSource for ZfsText<'_> {
    fn commandline(&self, filesystem: &str) -> Option<String> {
        self.zfs.get_value(COMMANDLINE.name, filesystem)
    }

    fn inherited_source(&self, filesystem: &str) -> Option<String> {
        self.zfs.inherited_source(COMMANDLINE.name, filesystem)
    }
}

#[cfg(test)]
mod tests;
