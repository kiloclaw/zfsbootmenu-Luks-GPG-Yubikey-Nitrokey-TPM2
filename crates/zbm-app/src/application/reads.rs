//! What [`App`] performs without the console ([`App::quietly`]), and the
//! pools it reads: those of each snapshot, imported ones and those the
//! last scan found, and the scan itself ([`Action::ScanPools`]), which
//! looks at every device and so runs only when asked, and again after an
//! action that may have imported or exported a pool, or opened what holds
//! one; and a device's kernel messages ([`Action::LoadDeviceLog`]).

use zbm_platform::System;
use zbm_types::{DatasetName, Untrusted};
use zbm_zfs::{Zfs, ZfsError};

use super::App;
use crate::model::{Action, Event, Pool};
use crate::{auth, changes, read};

/// Whether `action` may change the pools a scan finds: it imports or
/// exports a pool, opens a volume that may hold one, or opens a shell that
/// may.
pub(super) fn may_move_pools(action: &Action) -> bool {
    matches!(
        action,
        Action::Import(..)
            | Action::Export(_)
            | Action::Open { .. }
            | Action::UnlockAll
            | Action::Chroot(_)
    )
}

impl App<'_> {
    /// Applies `action` without taking the console: the volumes', the
    /// changes of package D ([`Action::is_change`]) whose pool is writable
    /// and whose key is loaded, and what only reads the machine, as the
    /// dry runs of [`Action::Plan`] ([`App::plan`]), the scan of the pools
    /// and a device's kernel messages, which the lock leaves free.
    pub(super) fn quietly(&self, action: &Action, console: System<'_>) -> Vec<Event> {
        match action {
            Action::Plan(planned) => return self.planned(planned, console),
            Action::ScanPools => return self.scan(),
            Action::LoadDeviceLog(device) => return self.device_log(device),
            _ => {}
        }
        // What the lock keeps takes the console, where the PIN is asked;
        // a menu that thought it open is told.
        let state = zbm_state::State::new(console.files, &self.runtime.base);
        let bootfs: Option<DatasetName> = state.bootfs().and_then(|name| name.parse().ok());
        if action.is_kept_by_lock(bootfs.as_ref()) && self.is_menu_locked(console.files) {
            return auth::refused();
        }
        if !action.is_change() {
            return self.alone(action);
        }
        let system = console.with_runner(self.context.runner());
        self.change(action, &system, false)
    }

    /// Performs the change `action` on `system`, asking on its console
    /// when `asks`.
    pub(super) fn change(&self, action: &Action, system: &System<'_>, asks: bool) -> Vec<Event> {
        let state = zbm_state::State::new(system.files, &self.runtime.base);
        let zfs = zbm_zfs::ZfsCli::new(system.runner);
        let mut needs = changes::OnMachine {
            system,
            state: &state,
            settings: &self.runtime.boot,
            import: &self.runtime.import,
            asks,
        };
        let mut changes = changes::Changes {
            zfs: &zfs,
            needs: &mut needs,
        };
        changes
            .perform(action)
            .unwrap_or_else(|| self.alone(action))
    }

    /// The pools of the snapshot: the imported ones, then those the last
    /// scan found that could be imported, which no snapshot looks for.
    ///
    /// # Errors
    ///
    /// When `zpool list` fails or prints something unexpected.
    pub(super) fn pools(&self, zfs: &dyn Zfs) -> Result<Vec<Pool>, ZfsError> {
        let imported = read::pools(zfs)?;
        let found = self.seen.map(super::Seen::importable).unwrap_or_default();
        Ok(read::merged(imported, &found))
    }

    /// Looks for the pools that could be imported, and keeps them; the
    /// last scan's stay when this one fails, as when it is cancelled.
    /// Their hosts are read only for those another system used last.
    fn scanned(&self, zfs: &dyn Zfs) -> Vec<Pool> {
        let found = read::importable(zfs);
        match (self.seen, found) {
            (Some(seen), Some(found)) => {
                seen.keep(found);
                seen.importable()
            }
            (Some(seen), None) => seen.importable(),
            (None, found) => found.unwrap_or_default(),
        }
    }

    /// [`Action::ScanPools`]: every pool, the imported ones and those that
    /// could be imported, which every snapshot lists from now on. When the
    /// imported pools cannot be read, the menu keeps the pools it shows,
    /// and the failure is logged: the menu asks for the scan by itself, and
    /// no dialog may stop an unattended boot for it.
    pub(super) fn scan(&self) -> Vec<Event> {
        let zfs = zbm_zfs::ZfsCli::new(self.context.runner());
        let found = self.scanned(&zfs);
        match read::pools(&zfs) {
            Ok(imported) => vec![Event::PoolsScanned(read::merged(imported, &found))],
            Err(error) => {
                tracing::warn!(%error, "the scan could not read the imported pools");
                Vec::new()
            }
        }
    }

    /// [`Action::LoadDeviceLog`]: the kernel's messages about `device` and
    /// what it stands on, which write nothing on the console.
    pub(super) fn device_log(&self, device: &Untrusted<String>) -> Vec<Event> {
        let log = read::device_log(self.context.files(), self.context.runner(), device);
        vec![Event::DeviceLogLoaded(log)]
    }

    /// The pools of the snapshot, as [`App::pools`] reads them; none when
    /// `zpool list` fails.
    pub(crate) fn known_pools(&self) -> Vec<Pool> {
        let zfs = zbm_zfs::ZfsCli::new(self.context.runner());
        self.pools(&zfs).unwrap_or_default()
    }

    /// Scans the pools again for the snapshots read after an action that
    /// may have changed what a scan finds; nothing, with nothing to keep
    /// the scan in.
    pub(super) fn rescan(&self) {
        if self.seen.is_some() {
            self.scanned(&zbm_zfs::ZfsCli::new(self.context.runner()));
        }
    }
}

#[cfg(test)]
mod tests;
