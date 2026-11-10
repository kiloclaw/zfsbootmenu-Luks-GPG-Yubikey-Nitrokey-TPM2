//! The hostile machine: the demo's, with the names the input gives.

use std::sync::OnceLock;

use zbm_tui::menu::demo::DemoOptions;
use zbm_tui::menu::demo::data::{machine, snapshots};
use zbm_tui::menu::world::{
    DeviceState, Environment, Event, Health, Kernel, Level, LogEntry, Pool, PoolHost, Snapshot,
    SnapshotInfo, SnapshotList,
};
use zbm_types::{DatasetName, PoolName, Untrusted};

/// The demo's machine, the snapshots of every environment read: made
/// once.
fn demo() -> &'static Snapshot {
    static DEMO: OnceLock<Snapshot> = OnceLock::new();
    DEMO.get_or_init(|| {
        let options = DemoOptions::default();
        let mut snapshot = machine(&options);
        snapshot.loaded = snapshots(&options);
        snapshot
    })
}

/// The demo's machine with the input's names: each environment's kernel
/// versions and command line, the LUKS volumes' names and devices, the
/// names and notes of the pools' devices, a copy of the first pool not
/// imported, last used by a host the input names, and the log.
pub fn hostile(next: &mut impl FnMut() -> Untrusted<String>) -> Snapshot {
    let mut snapshot = demo().clone();
    for environment in &mut snapshot.environments {
        environment.kernels = vec![
            Kernel {
                version: next(),
                is_default: true,
            },
            Kernel {
                version: next(),
                is_default: false,
            },
        ];
        // The menu shows an inherited line's expansion, any other its
        // value: both carry the input.
        environment.command_line.value = next();
        environment.command_line.expanded = environment.command_line.value.clone();
    }
    for volume in &mut snapshot.volumes {
        volume.name = next();
        volume.device = next();
    }
    for pool in &mut snapshot.pools {
        for device in &mut pool.devices {
            device.name = next();
            device.note = Some(next());
            device.state = Some(DeviceState::Unavailable);
            for child in &mut device.children {
                child.name = next();
                child.state = Some(DeviceState::Faulted);
            }
        }
    }
    if let Some(mut other) = snapshot.pools.first().cloned() {
        other.imported = false;
        other.id = Some(9);
        other.host = Some(PoolHost {
            hostid: None,
            hostname: Some(next()),
        });
        snapshot.pools.push(other);
    }
    snapshot.log.entries = (0..3)
        .map(|_| LogEntry {
            level: Level::Warning,
            text: next(),
        })
        .collect();
    snapshot
}

/// Adds two environments the input names to the first one's pool, and a
/// pool it names with an environment of its own. A name that is none is
/// not added: a dataset's name is hostile by its length, its spaces and
/// its depth alone.
pub fn added(snapshot: &mut Snapshot, next: &mut impl FnMut() -> Untrusted<String>) {
    let Some(first) = snapshot.environments.first().cloned() else {
        return;
    };
    let copy = |name: DatasetName| Environment {
        name,
        is_default: false,
        newest_snapshot: None,
        ..first.clone()
    };
    let pool = first.name.pool();
    for _ in 0..2 {
        if let Ok(name) = format!("{pool}/ROOT/{}", next().get()).parse() {
            snapshot.environments.push(copy(name));
        }
    }
    let Ok(named) = next().get().parse::<PoolName>() else {
        return;
    };
    if let Ok(name) = format!("{named}/{}", next().get()).parse() {
        snapshot.environments.push(copy(name));
    }
    snapshot.pools.push(Pool::imported(named, Health::Faulted));
}

/// The snapshots of `environment` whose names the input gives, as the
/// machine sends them: taken at times that cross the boundaries of the
/// timeline's groups, around `now`, after it and at the epoch.
pub fn taken(
    environment: &DatasetName,
    now: u64,
    next: &mut impl FnMut() -> Untrusted<String>,
) -> Event {
    let mut taken = Vec::new();
    for hours in 0..12_u64 {
        let part = next();
        let Ok(snapshot) = format!("{environment}@{}", part.get()).parse() else {
            continue;
        };
        let length = u64::try_from(part.get().len()).unwrap_or(u64::MAX);
        let created = match length % 4 {
            0 => 0,
            1 => now.saturating_add(hours * 3600),
            2 => now.saturating_sub(length.saturating_mul(hours).saturating_mul(86_400)),
            _ => u64::MAX,
        };
        taken.push(SnapshotInfo {
            name: snapshot,
            created,
            used: length.saturating_mul(1 << 40),
        });
    }
    Event::SnapshotsLoaded(SnapshotList {
        environment: environment.clone(),
        snapshots: taken,
    })
}
