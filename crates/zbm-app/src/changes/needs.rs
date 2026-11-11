//! What a change needs besides ZFS, from the machine: its pool imported
//! read/write, its key loaded, the kernels of an environment or a
//! snapshot, a mount for `zfs diff`, a read/write one to restore files
//! into, and the menu's imports of a pool.

use std::path::Path;

use zbm_boot::BootSettings;
use zbm_bootenv::Noise;
use zbm_platform::{Context, System};
use zbm_pool::ImportSettings;
use zbm_pool::writable::set_rw_pool;
use zbm_state::State;
use zbm_tools::console::ConsoleInteraction;
use zbm_types::{DatasetName, FilesystemName, PoolName, Printable, SnapshotName, Untrusted};

use super::restore::Restore;
use crate::boot::load_menu_key;
use crate::model::{ImportOptions, Step, StepKind};
use crate::volumes::causes;

/// What a change needs besides ZFS. A failure is its reasons, outermost
/// first.
pub(crate) trait Needs {
    /// Imports `pool` read/write unless it is.
    fn writable(&mut self, pool: &PoolName) -> Result<(), Vec<String>>;

    /// Imports `pool` with `options`, exporting it first if it is
    /// imported (`changes::import`).
    fn import(&mut self, pool: &PoolName, options: ImportOptions) -> Result<(), Vec<String>>;

    /// Loads the key of the encryption root of `dataset` unless it is
    /// loaded or there is none.
    fn load_key(&mut self, dataset: &DatasetName) -> Result<(), Vec<String>>;

    /// Whether `snapshot` has a kernel, so that a rollback to it leaves
    /// something to boot.
    fn has_kernels(&mut self, snapshot: &SnapshotName) -> bool;

    /// Mounts `filesystem` read-only; where.
    fn mount(&mut self, filesystem: &DatasetName) -> Result<String, Vec<String>>;

    /// Mounts the environment `environment` read/write; where.
    fn mount_writable(&mut self, environment: &DatasetName) -> Result<String, Vec<String>>;

    /// Unmounts what [`Needs::mount`] or [`Needs::mount_writable`] mounted
    /// at `mounted`.
    fn unmount(&mut self, mounted: &str);

    /// The file name in `/boot` of the kernel of version `version` of
    /// `environment`: `vmlinuz-6.12.9_1`.
    fn kernel_file(
        &mut self,
        environment: &DatasetName,
        version: &str,
    ) -> Result<String, Vec<String>>;

    /// Fails, with the reason for each path, unless each of `paths` is in
    /// `snapshot` and can be restored into its environment, mounted
    /// read/write at `mounted`, without following a symbolic link.
    fn check_restore(
        &mut self,
        mounted: &str,
        snapshot: &SnapshotName,
        paths: &[String],
    ) -> Result<(), Vec<String>>;

    /// Copies `paths` from `snapshot` into its environment, mounted
    /// read/write at `mounted`, with their owner, mode and times; how many
    /// were restored.
    fn restore(
        &mut self,
        mounted: &str,
        snapshot: &SnapshotName,
        paths: &[String],
    ) -> Result<usize, Vec<String>>;

    /// The time, in seconds since 1970, that a safety snapshot is named
    /// for.
    fn now(&self) -> u64;

    /// Records `environment` as the default boot environment, which the
    /// hooks get as `BOOTFS` from then on, as the legacy menu wrote
    /// `${BASE}/bootfs`.
    fn record_default(&mut self, environment: &DatasetName);
}

/// The machine's needs, on the console when it has the console: the
/// read/write import asks about hibernation where it must, and keys are
/// asked for. Without it, a read-only pool or a locked key fails the
/// change, which the menu then performs on the console.
pub(crate) struct OnMachine<'s, 'a> {
    /// The machine.
    pub system: &'s System<'a>,
    /// The state directory.
    pub state: &'s State<'a>,
    /// The hooks and the import policy.
    pub settings: &'s BootSettings,
    /// The settings of a first import, which host ID matching reads.
    pub import: &'s ImportSettings,
    /// Whether the change has the console, and may ask.
    pub asks: bool,
}

impl Needs for OnMachine<'_, '_> {
    fn writable(&mut self, pool: &PoolName) -> Result<(), Vec<String>> {
        if !self.asks {
            if zbm_pool::writable::is_writable(self.system.runner, pool) {
                return Ok(());
            }
            return Err(vec![format!("{pool} is imported read-only")]);
        }
        let context = Context::new(*self.system);
        let interaction = ConsoleInteraction::new(*self.system);
        let policy = self.settings.import_policy;
        set_rw_pool(&context, &interaction, self.state, pool, policy)
            .map_err(|error| causes(&error))
    }

    fn import(&mut self, pool: &PoolName, options: ImportOptions) -> Result<(), Vec<String>> {
        super::import::import(self, pool, options)
    }

    fn load_key(&mut self, dataset: &DatasetName) -> Result<(), Vec<String>> {
        if !self.asks {
            return match zbm_bootenv::locked_root(self.system.runner, dataset) {
                None => Ok(()),
                Some(root) => Err(vec![format!("the key of {root} is not loaded")]),
            };
        }
        let context = Context::new(*self.system);
        let interaction = ConsoleInteraction::new(*self.system);
        load_menu_key(&context, &interaction, self.state, self.settings, dataset)
            .map_err(|error| causes(&error))
    }

    fn has_kernels(&mut self, snapshot: &SnapshotName) -> bool {
        let snapshot = FilesystemName::from(snapshot.clone());
        zbm_bootenv::find_kernels(&Context::new(*self.system), self.state, &snapshot).is_ok()
    }

    fn mount(&mut self, filesystem: &DatasetName) -> Result<String, Vec<String>> {
        let filesystem = FilesystemName::from(filesystem.clone());
        // Mounted until `unmount`, which unmounts what is below it too.
        zbm_bootenv::mount_zfs(
            &Context::new(*self.system),
            self.state,
            &filesystem,
            Noise::Hidden,
        )
        .map(|mounted| mounted.leave().to_string_lossy().into_owned())
        .ok_or_else(|| vec![format!("{filesystem} could not be mounted")])
    }

    fn mount_writable(&mut self, environment: &DatasetName) -> Result<String, Vec<String>> {
        // Mounted until `unmount`, as `mount` is.
        zbm_bootenv::mount_zfs_writable(
            &Context::new(*self.system),
            self.state,
            environment,
            Noise::Hidden,
        )
        .map(|mounted| mounted.leave().to_string_lossy().into_owned())
        .ok_or_else(|| vec![format!("{environment} could not be mounted read/write")])
    }

    fn unmount(&mut self, mounted: &str) {
        let context = Context::new(*self.system);
        if let Err(error) = zbm_bootenv::recursive_umount(&context, Path::new(mounted)) {
            tracing::warn!(%error, "unmounting the boot environment");
        }
    }

    fn kernel_file(
        &mut self,
        environment: &DatasetName,
        version: &str,
    ) -> Result<String, Vec<String>> {
        let boot = crate::boot::Boot {
            system: self.system,
            state: self.state,
            settings: self.settings,
        };
        boot.kernel_file(environment, version).ok_or_else(|| {
            vec![format!(
                "{environment} has no kernel {}",
                Printable(version)
            )]
        })
    }

    fn check_restore(
        &mut self,
        mounted: &str,
        snapshot: &SnapshotName,
        paths: &[String],
    ) -> Result<(), Vec<String>> {
        Restore::new(self.system.files, self.system.runner, mounted, snapshot).check(paths)
    }

    fn restore(
        &mut self,
        mounted: &str,
        snapshot: &SnapshotName,
        paths: &[String],
    ) -> Result<usize, Vec<String>> {
        Restore::new(self.system.files, self.system.runner, mounted, snapshot).copy(paths)
    }

    fn now(&self) -> u64 {
        self.system
            .clock
            .wall()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_secs())
    }

    fn record_default(&mut self, environment: &DatasetName) {
        if let Err(error) = self.state.set_bootfs(environment.as_str()) {
            tracing::warn!(%error, %environment, "recording the default boot environment");
        }
    }
}

/// What a change needs, planned instead of done: a pool imported
/// read-only, a key that is not loaded, an import and a restore become
/// steps of the plan, and what is read, such as the kernels of a snapshot
/// or the files to restore, is read from the machine, mounted read-only.
pub(crate) struct Planning<'s, 'm> {
    /// The machine, without the console.
    pub machine: OnMachine<'s, 'm>,
    /// The plan's steps, which the ZFS port of the dry run adds to too.
    pub steps: &'s std::sync::Mutex<Vec<Step>>,
}

impl Planning<'_, '_> {
    fn step(&self, kind: StepKind) {
        self.push(Step {
            kind,
            command: None,
        });
    }

    fn push(&self, step: Step) {
        if let Ok(mut steps) = self.steps.lock() {
            steps.push(step);
        }
    }
}

impl Needs for Planning<'_, '_> {
    fn writable(&mut self, pool: &PoolName) -> Result<(), Vec<String>> {
        if !zbm_pool::writable::is_writable(self.machine.system.runner, pool) {
            self.step(StepKind::ImportReadWrite(pool.clone()));
        }
        Ok(())
    }

    fn import(&mut self, pool: &PoolName, options: ImportOptions) -> Result<(), Vec<String>> {
        for step in super::import::steps(&self.machine, pool, options)? {
            self.push(step);
        }
        Ok(())
    }

    fn load_key(&mut self, dataset: &DatasetName) -> Result<(), Vec<String>> {
        if let Some(root) = zbm_bootenv::locked_root(self.machine.system.runner, dataset) {
            self.step(StepKind::LoadKey(root));
        }
        Ok(())
    }

    fn has_kernels(&mut self, snapshot: &SnapshotName) -> bool {
        self.machine.has_kernels(snapshot)
    }

    fn mount(&mut self, filesystem: &DatasetName) -> Result<String, Vec<String>> {
        self.machine.mount(filesystem)
    }

    /// A plan writes nothing: the environment is mounted read-only.
    fn mount_writable(&mut self, environment: &DatasetName) -> Result<String, Vec<String>> {
        self.machine.mount(environment)
    }

    fn unmount(&mut self, mounted: &str) {
        self.machine.unmount(mounted);
    }

    fn kernel_file(
        &mut self,
        environment: &DatasetName,
        version: &str,
    ) -> Result<String, Vec<String>> {
        self.machine.kernel_file(environment, version)
    }

    fn check_restore(
        &mut self,
        mounted: &str,
        snapshot: &SnapshotName,
        paths: &[String],
    ) -> Result<(), Vec<String>> {
        self.machine.check_restore(mounted, snapshot, paths)
    }

    fn restore(
        &mut self,
        _mounted: &str,
        snapshot: &SnapshotName,
        paths: &[String],
    ) -> Result<usize, Vec<String>> {
        self.step(StepKind::Restore {
            snapshot: snapshot.clone(),
            paths: paths.iter().cloned().map(Untrusted::new).collect(),
        });
        Ok(paths.len())
    }

    fn now(&self) -> u64 {
        self.machine.now()
    }

    /// A plan records nothing.
    fn record_default(&mut self, _environment: &DatasetName) {}
}

#[cfg(test)]
mod tests;
