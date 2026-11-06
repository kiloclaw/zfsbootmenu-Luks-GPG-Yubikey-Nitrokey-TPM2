//! Tests of what needs attention in the workspace (ADR 0016, point 8).

use pretty_assertions::assert_eq;
use zbm_types::Untrusted;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::theme::Theme;
use crate::menu::world::{Device, LogEntry, PoolHost, Snapshot};

/// A device named `name` in `state`, with the devices `children`.
fn device(name: &str, state: DeviceState, children: Vec<Device>) -> Device {
    Device {
        name: Untrusted::new(name.to_owned()),
        state: Some(state),
        errors: None,
        note: None,
        children,
    }
}

/// The menu of a machine with `pools` and `log`, and nothing else.
fn model(pools: Vec<Pool>, log: Vec<LogEntry>) -> Model {
    let snapshot = Snapshot {
        pools,
        log: Log { entries: log },
        ..Snapshot::default()
    };
    Model::new(snapshot, Theme::new(Capabilities::FULL))
}

/// What needs attention, as it reads, with the tab each leads to.
fn said(model: &Model) -> Vec<(String, Tab)> {
    attention(model)
        .into_iter()
        .map(|item| (item.what, item.tab))
        .collect()
}

#[test]
fn a_pool_lists_its_lost_devices_worst_first_and_its_checkpoint() {
    let mut tank = Pool::imported("tank".parse().unwrap(), Health::Degraded);
    tank.checkpoint = Some(true);
    tank.devices = vec![
        device(
            "mirror-0",
            DeviceState::Degraded,
            vec![
                device("/dev/disk/by-id/ata-A", DeviceState::Removed, Vec::new()),
                device("/dev/sdb", DeviceState::Offline, Vec::new()),
            ],
        ),
        device(
            "mirror-1",
            DeviceState::Degraded,
            vec![
                // ZFS names a device it lost by its identifier, and says
                // which path it had.
                Device {
                    note: Some(Untrusted::new("was /dev/sdc\u{1b}".to_owned())),
                    ..device("9876543210987654321", DeviceState::Unavailable, Vec::new())
                },
                device("/dev/sdd", DeviceState::Faulted, Vec::new()),
            ],
        ),
    ];
    let model = model(vec![tank], Vec::new());

    let pools = |what: &str| (what.to_owned(), Tab::Pools);
    assert_eq!(
        said(&model),
        [
            pools("tank DEGRADED"),
            pools("tank: sdd FAULTED"),
            pools("tank: sdc\\x1b UNAVAIL"),
            pools("tank: ata-A REMOVED"),
            pools("tank: checkpoint"),
        ]
    );
    let roles: Vec<Role> = attention(&model).iter().map(|item| item.role).collect();
    assert_eq!(
        roles,
        [
            Role::Warning,
            Role::Danger,
            Role::Warning,
            Role::Warning,
            Role::Muted
        ]
    );
}

#[test]
fn pools_not_imported_say_who_used_them_last() {
    let not_imported = |name: &str, host: Option<PoolHost>| Pool {
        imported: false,
        checkpoint: None,
        host,
        ..Pool::imported(name.parse().unwrap(), Health::Online)
    };
    let named = PoolHost {
        hostid: None,
        hostname: Some(Untrusted::new("other\u{1b}[2J".to_owned())),
    };
    let known = PoolHost {
        hostid: Some("deadbeef".parse().unwrap()),
        hostname: Some(Untrusted::new("other".to_owned())),
    };
    let model = model(
        vec![
            not_imported("backup", None),
            not_imported("archive", Some(known)),
            not_imported("data", Some(named)),
            not_imported("lost", Some(PoolHost::default())),
        ],
        Vec::new(),
    );

    let what: Vec<String> = said(&model).into_iter().map(|(what, _)| what).collect();
    assert_eq!(
        what,
        [
            "backup: not imported",
            "archive: host deadbeef",
            "data: host other\\x1b[2J",
            "lost: another host",
        ]
    );
}

#[test]
fn the_log_s_errors_and_warnings_are_counted_apart() {
    let entry = |level| LogEntry {
        level,
        text: Untrusted::new("something".to_owned()),
    };
    let model = model(
        Vec::new(),
        vec![
            entry(Level::Warning),
            entry(Level::Error),
            entry(Level::Info),
            entry(Level::Warning),
        ],
    );

    let log = |what: &str| (what.to_owned(), Tab::Log);
    assert_eq!(
        said(&model),
        [log("1 error in the log"), log("2 warnings in the log")]
    );
    assert_eq!(attention_count(&model), 2);
}
