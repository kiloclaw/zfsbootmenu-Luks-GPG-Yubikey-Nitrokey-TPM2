//! The workspace's Pools tab with the pools, the devices and the kernel's
//! messages the input gives.

use zbm_display::keys::{Key, KeyPress};
use zbm_tui::menu::model::Model;
use zbm_tui::menu::world::{
    Action, Device, DeviceClass, DeviceErrors, DeviceLog, DeviceState, Event, Health, Pool,
    PoolHost,
};
use zbm_tui::menu::{Message, update};
use zbm_types::{PoolName, Untrusted};

use super::check_page;

/// A device of the name `name` and the note `note`, with `children`.
fn device(name: Untrusted<String>, note: Untrusted<String>, children: Vec<Device>) -> Device {
    let length = u64::try_from(name.get().len()).unwrap_or(u64::MAX);
    Device {
        class: DeviceClass::Data,
        state: (length % 3 > 0).then_some(DeviceState::Degraded),
        errors: Some(DeviceErrors {
            read: length,
            write: u64::MAX,
            cksum: length.saturating_mul(1 << 20),
        }),
        children,
        name,
        note: Some(note),
    }
}

/// The pools of the input, as a scan sends them: one that the input names,
/// when its name is one, with a vdev of two devices, a log device, a spare
/// in use, shown twice as `zpool` shows it, and what `zpool` says; and one
/// of another host, not imported. The volumes are of no pool.
pub fn hostile_pools(next: &mut impl FnMut() -> Untrusted<String>) -> Vec<Pool> {
    let Ok(name) = next().get().parse().or_else(|_| "tank".parse::<PoolName>()) else {
        return Vec::new();
    };
    let mut pool = Pool::imported(name, Health::Degraded);
    let disks = vec![
        device(next(), next(), Vec::new()),
        device(next(), next(), Vec::new()),
    ];
    let log = Device {
        class: DeviceClass::Log,
        ..device(next(), next(), Vec::new())
    };
    let spare = device(next(), next(), Vec::new());
    let mut spares = Device {
        class: DeviceClass::Spare,
        ..spare.clone()
    };
    spares.state = None;
    pool.devices = vec![device(next(), next(), disks), log, spares];
    pool.devices[0].children.push(spare);
    pool.status = Some(next());
    pool.action = Some(next());
    pool.errors = Some(u64::MAX);
    let mut other = pool.clone();
    other.imported = false;
    other.id = Some(u64::MAX);
    other.host = Some(PoolHost {
        hostid: None,
        hostname: Some(next()),
    });
    vec![pool, other]
}

/// The workspace's Pools tab of `model` after a scan found the pools of
/// the input: each row selected, drawn and said; then `Enter` on the first
/// device of the first vdev, and the kernel's messages the input gives
/// about it.
pub fn pools_tab(model: Model, next: &mut impl FnMut() -> Untrusted<String>, size: (u16, u16)) {
    // The machine changed: its volumes hold devices of no pool.
    let mut changed = model.snapshot.clone();
    for volume in &mut changed.volumes {
        volume.pool = None;
    }
    let event = Event::Changed(Box::new(changed));
    let (model, _) = update(model, Message::Event(Box::new(event)));
    let scanned = Event::PoolsScanned(hostile_pools(next));
    let (model, _) = update(model, Message::Event(Box::new(scanned)));
    let key = |model, key| update(model, Message::Key(KeyPress::new(key))).0;
    let mut model = key(model, Key::Char('3'));
    let rows = model.pools.matches.len();
    for _ in 0..rows {
        check_page(&model, size);
        model = key(model, Key::Down);
    }
    // The first pool, its vdev, then the first device of it.
    let model = [Key::Home, Key::Down, Key::Down]
        .into_iter()
        .fold(model, key);
    let (model, actions) = update(model, Message::Key(KeyPress::new(Key::Enter)));
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::LoadDeviceLog(_))),
        "{actions:?}"
    );
    let log = DeviceLog {
        device: next(),
        names: vec![next(), next()],
        lines: (0..4).map(|_| next()).collect(),
    };
    let loaded = Event::DeviceLogLoaded(log);
    let (model, _) = update(model, Message::Event(Box::new(loaded)));
    check_page(&model, size);
}
