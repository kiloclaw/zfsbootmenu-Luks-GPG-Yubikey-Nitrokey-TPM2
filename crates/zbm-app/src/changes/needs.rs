//! What a change needs besides ZFS, from the machine: its pool imported
//! read/write, its key loaded, the kernels of a snapshot, and a mount for
//! `zfs diff`.

use std::path::Path;

use zbm_boot::boot::{self as kexec, BootSettings, Noise};
use zbm_boot::keys::{self, BootPath, Clear, KeyLoad};
use zbm_boot::writable::set_rw_pool;
use zbm_platform::System;
use zbm_state::State;
use zbm_types::{DatasetName, FilesystemName, PoolName, SnapshotName};

use crate::model::{Step, StepKind};
use crate::volumes::causes;

/// What a change needs besides ZFS. A failure is its reasons, outermost
/// first.
pub(crate) trait Needs {
    /// Imports `pool` read/write unless it is.
    fn writable(&mut self, pool: &PoolName) -> Result<(), Vec<String>>;

    /// Loads the key of the encryption root of `dataset` unless it is
    /// loaded or there is none.
    fn load_key(&mut self, dataset: &DatasetName) -> Result<(), Vec<String>>;

    /// Whether `snapshot` has a kernel, so that a rollback to it leaves
    /// something to boot.
    fn has_kernels(&mut self, snapshot: &SnapshotName) -> bool;

    /// Mounts `filesystem` read-only; where.
    fn mount(&mut self, filesystem: &DatasetName) -> Result<String, Vec<String>>;

    /// Unmounts what [`Needs::mount`] mounted at `mounted`.
    fn unmount(&mut self, mounted: &str);
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
    /// Whether the change has the console, and may ask.
    pub asks: bool,
}

impl Needs for OnMachine<'_, '_> {
    fn writable(&mut self, pool: &PoolName) -> Result<(), Vec<String>> {
        if !self.asks {
            if kexec::is_writable(self.system.runner, pool) {
                return Ok(());
            }
            return Err(vec![format!("{pool} is imported read-only")]);
        }
        set_rw_pool(self.system, self.state, pool, self.settings.import_policy)
            .map_err(|error| causes(&error))
    }

    fn load_key(&mut self, dataset: &DatasetName) -> Result<(), Vec<String>> {
        if !self.asks {
            return match kexec::locked_root(self.system.runner, dataset) {
                None => Ok(()),
                Some(root) => Err(vec![format!("the key of {root} is not loaded")]),
            };
        }
        let how = KeyLoad {
            hooks: &self.settings.hooks,
            path: BootPath::Menu,
            clear: Clear::Yes,
        };
        keys::load_key(self.system, self.state, &how, dataset).map_err(|error| causes(&error))
    }

    fn has_kernels(&mut self, snapshot: &SnapshotName) -> bool {
        let snapshot = FilesystemName::from(snapshot.clone());
        kexec::find_kernels(self.system, self.state, &snapshot).is_ok()
    }

    fn mount(&mut self, filesystem: &DatasetName) -> Result<String, Vec<String>> {
        let filesystem = FilesystemName::from(filesystem.clone());
        // Mounted until `unmount`, which unmounts what is below it too.
        kexec::mount_zfs(self.system, self.state, &filesystem, Noise::Hidden)
            .map(|mounted| mounted.leave().to_string_lossy().into_owned())
            .ok_or_else(|| vec![format!("{filesystem} could not be mounted")])
    }

    fn unmount(&mut self, mounted: &str) {
        if let Err(error) = kexec::recursive_umount(self.system, Path::new(mounted)) {
            tracing::warn!(%error, "unmounting the boot environment");
        }
    }
}

/// What a change needs, planned instead of done: a pool imported
/// read-only and a key that is not loaded become steps of the plan, and
/// what is read, such as the kernels of a snapshot, is read from the
/// machine.
pub(crate) struct Planning<'s, 'm> {
    /// The machine, without the console.
    pub machine: OnMachine<'s, 'm>,
    /// The plan's steps, which the ZFS port of the dry run adds to too.
    pub steps: &'s std::sync::Mutex<Vec<Step>>,
}

impl Planning<'_, '_> {
    fn step(&self, kind: StepKind) {
        if let Ok(mut steps) = self.steps.lock() {
            steps.push(Step {
                kind,
                command: None,
            });
        }
    }
}

impl Needs for Planning<'_, '_> {
    fn writable(&mut self, pool: &PoolName) -> Result<(), Vec<String>> {
        if !kexec::is_writable(self.machine.system.runner, pool) {
            self.step(StepKind::ImportReadWrite(pool.clone()));
        }
        Ok(())
    }

    fn load_key(&mut self, dataset: &DatasetName) -> Result<(), Vec<String>> {
        if let Some(root) = kexec::locked_root(self.machine.system.runner, dataset) {
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

    fn unmount(&mut self, mounted: &str) {
        self.machine.unmount(mounted);
    }
}

#[cfg(test)]
mod tests;
