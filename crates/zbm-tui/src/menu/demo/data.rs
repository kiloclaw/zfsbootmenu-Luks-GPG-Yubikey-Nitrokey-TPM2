//! The demo machine (`docs/dev/menu-design/usability.md`, "Setup"): two
//! pools, four boot environments with their kernels, 400 snapshots on the
//! default one, kept apart from the snapshot as the machine keeps them,
//! three LUKS volumes, a `DEGRADED` pool and, on request, a pool of another
//! host.

use zbm_app::Encryption;
use zbm_types::{DatasetName, SnapshotName, Untrusted};

use super::health;
use super::{DemoOptions, untrusted};
use crate::menu::world::{
    CommandLine, Device, DeviceClass, DeviceErrors, DeviceLog, DeviceState, Environment, Fallback,
    Health, Kernel, Keyboard, Level, Log, LogEntry, Method, Offer, Origin, PcrChange, Pool,
    PoolHost, Snapshot, SnapshotInfo, SnapshotList, Stage, StageState, Timeout, Tpm2Seal, Tries,
    Volume, VolumeState,
};

/// The demo's clock: 2026-10-23 08:00 UTC.
pub const NOW: u64 = 1_792_742_400;

/// A day in seconds.
const DAY: u64 = 86_400;

/// The stages of start-up.
pub const STAGES: [&str; 5] = [
    "Load the ZFS module",
    "Set the host ID",
    "Unlock LUKS volumes",
    "Import pools",
    "Find boot environments",
];

/// The stage that unlocks volumes.
pub const UNLOCK_STAGE: usize = 2;

/// The stage that imports pools.
pub const IMPORT_STAGE: usize = 3;

/// The pool of another host, with `--foreign`.
pub const FOREIGN_POOL: &str = "backup";

/// The machine's host ID.
pub const HOSTID: &str = "00bab10c";

/// The version of the machine's ZFS module.
const ZFS_VERSION: &str = "2.2.6-1";

/// The host ID that last imported the foreign pool.
pub const FOREIGN_HOSTID: &str = "deadbeef";

/// The name of the host that last imported the pool of another host.
const FOREIGN_HOST: &str = "backup-server";

#[allow(
    clippy::unreachable,
    reason = "the names of the demo machine are literals that parse"
)]
fn dataset(name: &str) -> DatasetName {
    name.parse()
        .unwrap_or_else(|_| unreachable!("demo names are valid"))
}

#[allow(
    clippy::unreachable,
    reason = "the names of the demo machine are literals that parse"
)]
fn snapshot_name(name: &str) -> SnapshotName {
    name.parse()
        .unwrap_or_else(|_| unreachable!("demo names are valid"))
}

/// The machine at power-on.
pub fn machine(options: &DemoOptions) -> Snapshot {
    let mut pools = vec![zroot(), tank()];
    if options.foreign {
        let mut foreign = pool(FOREIGN_POOL, Health::Online, 3);
        foreign.imported = false;
        foreign.checkpoint = None;
        foreign.host = Some(PoolHost {
            hostid: FOREIGN_HOSTID.parse().ok(),
            hostname: Some(untrusted(FOREIGN_HOST)),
        });
        foreign.devices = vec![device("sdd", DeviceState::Online, None)];
        pools.push(foreign);
    }
    let fallback = options.failing.then(|| Fallback {
        failed: dataset("zroot/ROOT/void"),
        attempts: 3,
        target: Some(dataset("zroot/ROOT/ubuntu")),
    });
    let mut snapshot = Snapshot {
        next_boot: options.next_boot.then(|| dataset("tank/ROOT/arch")),
        fallback,
        bootfs: Some(dataset("zroot/ROOT/void")),
        startup: STAGES
            .iter()
            .map(|name| Stage {
                name: (*name).to_owned(),
                state: StageState::Pending,
            })
            .collect(),
        timeout: options.timeout.map_or(Timeout::Wait, Timeout::Seconds),
        pools,
        environments: environments(options)
            .into_iter()
            .map(|environment| with_health(environment, options))
            .collect(),
        volumes: volumes(options),
        keyboard: Keyboard {
            layouts: super::keyboard::names(),
            local: true,
            positions: super::keyboard::positions(),
        },
        log: log(),
        now: NOW,
        menu_locked: false,
        // The demo answers every action.
        offers: Offer::ALL.into_iter().collect(),
        loaded: Vec::new(),
        host: HOSTID.parse().ok(),
        zfs_version: Some(untrusted(ZFS_VERSION)),
    };
    super::snapshots::Lists::new(snapshots(options)).show(&mut snapshot);
    snapshot
}

/// The snapshots of each environment at power-on, newest first: those the
/// machine sends when a column opens.
pub fn snapshots(options: &DemoOptions) -> Vec<SnapshotList> {
    let list = |name: &str, snapshots| SnapshotList {
        environment: dataset(name),
        snapshots,
    };
    vec![
        list(
            "zroot/ROOT/void",
            many_snapshots("zroot/ROOT/void", options.snapshots, options.workspace),
        ),
        list("zroot/ROOT/ubuntu", few_snapshots("zroot/ROOT/ubuntu")),
        list("tank/ROOT/arch", few_snapshots("tank/ROOT/arch")),
    ]
}

#[allow(
    clippy::unreachable,
    reason = "the names of the demo machine are literals that parse"
)]
fn pool(name: &str, health: Health, id: u64) -> Pool {
    let name = name
        .parse()
        .unwrap_or_else(|_| unreachable!("demo names are valid"));
    Pool {
        id: Some(id),
        devices: vec![device("/dev/nvme0n1p2", DeviceState::Online, None)],
        errors: Some(0),
        ..Pool::imported(name, health)
    }
}

/// The pools start-up left alone with `options.exported`, which the scan
/// finds: `archive`, which another host used last, and a `tank` that is
/// not the one imported.
pub(super) fn exported(options: &DemoOptions) -> Vec<Pool> {
    if !options.exported {
        return Vec::new();
    }
    let not_imported = |name, id, disk| Pool {
        imported: false,
        checkpoint: None,
        errors: None,
        devices: vec![Device {
            errors: None,
            ..device(disk, DeviceState::Online, None)
        }],
        ..pool(name, Health::Online, id)
    };
    let mut archive = not_imported("archive", 4, "sde");
    archive.host = Some(PoolHost {
        hostid: FOREIGN_HOSTID.parse().ok(),
        hostname: Some(untrusted(FOREIGN_HOST)),
    });
    vec![archive, not_imported("tank", 5, "sdf")]
}

/// A device without children; a note says why it cannot be used.
fn device(name: &str, state: DeviceState, note: Option<&str>) -> Device {
    Device {
        name: untrusted(name),
        class: DeviceClass::Data,
        state: Some(state),
        errors: Some(DeviceErrors::default()),
        note: note.map(untrusted),
        children: Vec::new(),
    }
}

/// `zroot`: a mirror of two disks and, as its cache, a third.
fn zroot() -> Pool {
    let mirror = Device {
        name: untrusted("mirror-0"),
        class: DeviceClass::Data,
        state: Some(DeviceState::Online),
        errors: Some(DeviceErrors::default()),
        note: None,
        children: vec![
            device("/dev/nvme0n1p2", DeviceState::Online, None),
            device("/dev/nvme1n1p2", DeviceState::Online, None),
        ],
    };
    Pool {
        devices: vec![
            mirror,
            Device {
                class: DeviceClass::Cache,
                ..device("/dev/nvme2n1", DeviceState::Online, None)
            },
        ],
        ..pool("zroot", Health::Online, 1)
    }
}

/// `tank`: a RAID-Z of three disks, one of them missing and another with
/// checksum errors, with a checkpoint.
fn tank() -> Pool {
    let raidz = Device {
        name: untrusted("raidz1-0"),
        class: DeviceClass::Data,
        state: Some(DeviceState::Degraded),
        errors: Some(DeviceErrors::default()),
        note: None,
        children: vec![
            device("/dev/sda2", DeviceState::Online, None),
            device("/dev/sdb2", DeviceState::Unavailable, Some("cannot open")),
            Device {
                errors: Some(DeviceErrors {
                    cksum: 14,
                    ..DeviceErrors::default()
                }),
                ..device("/dev/sdc2", DeviceState::Degraded, Some("too many errors"))
            },
        ],
    };
    Pool {
        checkpoint: Some(true),
        devices: vec![raidz],
        status: Some(untrusted(
            "One or more devices could not be opened. Sufficient replicas exist for the pool to \
             continue functioning in a degraded state.",
        )),
        action: Some(untrusted(
            "Attach the missing device and online it using 'zpool online'.",
        )),
        ..pool("tank", Health::Degraded, 2)
    }
}

fn kernels(versions: &[&str], default: Option<&str>) -> Vec<Kernel> {
    versions
        .iter()
        .map(|version| Kernel {
            version: untrusted(version),
            is_default: default == Some(*version),
        })
        .collect()
}

fn inherited(value: &str, from: &str) -> CommandLine {
    CommandLine {
        value: untrusted(value),
        origin: Origin::Inherited(dataset(from)),
    }
}

/// `environment` with its checks, and boot counting on the default one.
fn with_health(mut environment: Environment, options: &DemoOptions) -> Environment {
    let kernel = environment
        .boot_kernel()
        .map(|k| k.version.clone())
        .unwrap_or_default();
    environment.checks = health::checks(environment.name.as_str(), &kernel);
    if environment.is_default && environment.name.pool().as_str() == "zroot" {
        let left = if options.failing { 0 } else { 3 };
        environment.tries = Some(Tries { left, total: 3 });
    }
    environment
}

/// The command line of `tank/ROOT/arch` with
/// [`DemoOptions::long_command_line`].
const LONG_COMMAND_LINE: &str = "rw quiet splash loglevel=3 rd.udev.log_level=3 \
    systemd.show_status=auto zfs.zfs_arc_max=8589934592 init_on_alloc=1 \
    nvidia-drm.modeset=1 resume=/dev/mapper/luks-swap";

fn environments(options: &DemoOptions) -> Vec<Environment> {
    let environment = |name: &str, is_default, encryption, kernels, command_line| Environment {
        name: dataset(name),
        is_default,
        encryption,
        kernels,
        command_line,
        snapshot_count: 0,
        newest_snapshot: None,
        checks: Vec::new(),
        tries: None,
    };
    vec![
        environment(
            "zroot/ROOT/void",
            true,
            Encryption::Unlocked,
            kernels(&["6.12.9_1", "6.12.8_1", "6.8.0_1"], None),
            inherited("quiet loglevel=4", "zroot/ROOT"),
        ),
        environment(
            "zroot/ROOT/ubuntu",
            false,
            if options.locked {
                Encryption::Locked
            } else {
                Encryption::Unlocked
            },
            kernels(
                &["6.8.0-48-generic", "6.8.0-47-generic"],
                Some("6.8.0-47-generic"),
            ),
            inherited("quiet loglevel=4", "zroot/ROOT"),
        ),
        environment(
            "tank/ROOT/arch",
            false,
            Encryption::None,
            kernels(&["6.11.5-arch1-1", "6.10.10-arch1-1"], None),
            CommandLine {
                value: untrusted(if options.long_command_line {
                    LONG_COMMAND_LINE
                } else {
                    "rw quiet splash"
                }),
                origin: Origin::Local,
            },
        ),
        environment(
            "tank/ROOT/debian",
            false,
            Encryption::None,
            kernels(&["6.1.0-26-amd64"], None),
            CommandLine {
                value: Untrusted::default(),
                origin: Origin::Default,
            },
        ),
    ]
}

/// A snapshot taken `days` days before the demo's clock.
fn taken(name: &str, days: u64, used: u64) -> SnapshotInfo {
    SnapshotInfo {
        name: snapshot_name(name),
        created: NOW - days * DAY - 3600,
        used,
    }
}

/// A daily snapshot for each of `count` days, and the one taken before
/// yesterday's upgrade; with `today`, as in the workspace, before this
/// morning's, so that its timeline shows a group of each kind: today, the
/// days of this week, the weeks of this month and the months.
fn many_snapshots(dataset: &str, count: usize, today: bool) -> Vec<SnapshotInfo> {
    let mut snapshots = vec![SnapshotInfo {
        name: snapshot_name(&format!("{dataset}@pre-upgrade")),
        created: if today { NOW - 3600 } else { NOW - DAY / 2 },
        used: 1_288_490_188,
    }];
    for day in 1..=u64::try_from(count.saturating_sub(1)).unwrap_or_default() {
        let date = crate::menu::format::date(NOW - day * DAY);
        let used = 4_194_304 + (day % 7) * 13_631_488;
        snapshots.push(taken(
            &format!("{dataset}@autosnap_{date}_daily"),
            day,
            used,
        ));
    }
    snapshots
}

fn few_snapshots(dataset: &str) -> Vec<SnapshotInfo> {
    vec![
        taken(&format!("{dataset}@2026-10-01"), 22, 52_428_800),
        taken(&format!("{dataset}@2026-09-01"), 52, 734_003_200),
    ]
}

/// The TPM2 key of `luks-zroot-a`, sealed to PCRs 0 and 7; with
/// `changed`, the firmware was updated since.
fn tpm2_seal(changed: bool) -> Tpm2Seal {
    let update = PcrChange {
        index: 0,
        detail: Some("the firmware was updated to 1.21".to_owned()),
    };
    Tpm2Seal {
        pcrs: vec![0, 7],
        changed: if changed { vec![update] } else { Vec::new() },
    }
}

fn volumes(options: &DemoOptions) -> Vec<Volume> {
    let state = if options.locked {
        VolumeState::Locked
    } else {
        VolumeState::Open
    };
    let volume = |name: &str, device: &str, methods: Vec<Method>, pool: &str| Volume {
        name: untrusted(name),
        device: untrusted(device),
        state: state.clone(),
        methods,
        pool: pool.parse().ok(),
        tpm2: None,
    };
    let mut first = volume(
        "luks-zroot-a",
        "/dev/nvme0n1p2",
        vec![Method::Tpm2Pin, Method::Passphrase],
        "zroot",
    );
    first.tpm2 = Some(tpm2_seal(options.tpm_changed));
    if options.tpm_changed {
        first.state = VolumeState::Locked;
    }
    vec![
        first,
        volume(
            "luks-zroot-b",
            "/dev/nvme1n1p2",
            vec![Method::Fido2, Method::Passphrase],
            "zroot",
        ),
        volume("luks-tank", "/dev/sda2", vec![Method::Passphrase], "tank"),
    ]
}

fn log() -> Log {
    let entry = |level, text: &str| LogEntry {
        level,
        text: untrusted(text),
    };
    Log {
        entries: vec![
            entry(Level::Info, "ZFS 2.4.4 loaded, host ID 00bab10c"),
            entry(Level::Info, "imported zroot read-only"),
            entry(
                Level::Warning,
                "tank is DEGRADED: /dev/sdb2 is missing, 1 of 3 devices",
            ),
            entry(Level::Info, "imported tank read-only"),
        ],
    }
}

/// The kernel's messages about `device` on the demo machine: the disk
/// `tank` lost, `/dev/sdb2`, failed before it went (the lines of its
/// port, `ata3.00`, name no disk and are not among them), and one of its
/// messages, as a USB serial may, holds an escape sequence; the others
/// are quiet. Another path is no block device.
pub(super) fn device_log(device: &Untrusted<String>) -> DeviceLog {
    let path = device.get().as_str();
    let name = path.strip_prefix("/dev/").unwrap_or(path);
    let quiet = [
        "nvme0n1p2",
        "nvme1n1p2",
        "nvme2n1",
        "sda2",
        "sdc2",
        "sdd",
        "sde",
        "sdf",
    ];
    let (names, lines): (Vec<&str>, &[&str]) = match name {
        "sdb2" => (
            vec!["sdb2", "sdb"],
            &[
                "[    2.104511] sd 1:0:0:0: [sdb] 976773168 512-byte logical blocks",
                "[    2.118203]  sdb: sdb1 sdb2",
                "[   41.620355] I/O error, dev sdb, sector 2048 op 0x0:(READ)",
                "[   41.903846] sd 1:0:0:0: [sdb] Product: \u{1b}[2JDisk\u{7}",
                "[   42.511027] sd 1:0:0:0: [sdb] Synchronizing SCSI cache",
            ],
        ),
        _ if quiet.contains(&name) => (vec![name], &[]),
        _ => (Vec::new(), &[]),
    };
    DeviceLog {
        device: device.clone(),
        names: names.iter().map(|name| untrusted(name)).collect(),
        lines: lines.iter().map(|line| untrusted(line)).collect(),
    }
}
