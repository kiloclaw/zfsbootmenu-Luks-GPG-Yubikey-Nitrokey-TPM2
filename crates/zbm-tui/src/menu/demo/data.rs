//! The demo machine (`docs/dev/menu-design/usability.md`, "Setup"): two
//! pools, four boot environments with their kernels, 400 snapshots on the
//! default one, three LUKS volumes, a `DEGRADED` pool and, on request, a
//! pool of another host.

use zbm_app::Encryption;
use zbm_types::{DatasetName, SnapshotName, Untrusted};

use super::health;
use super::{DemoOptions, untrusted};
use crate::menu::world::{
    CommandLine, Environment, Fallback, Health, Kernel, Keyboard, Level, Log, LogEntry, Method,
    Origin, PcrChange, Pool, Snapshot, SnapshotInfo, Stage, StageState, Target, Timeout, Tpm2Seal,
    Tries, Volume, VolumeState,
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

/// The host ID that last imported the foreign pool.
pub const FOREIGN_HOSTID: &str = "deadbeef";

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
    let mut pools = vec![
        pool("zroot", Health::Online),
        pool("tank", Health::Degraded),
    ];
    if options.foreign {
        let mut foreign = pool(FOREIGN_POOL, Health::Online);
        foreign.imported = false;
        foreign.foreign_hostid = Some(FOREIGN_HOSTID.to_owned());
        pools.push(foreign);
    }
    let fallback = options.failing.then(|| Fallback {
        failed: dataset("zroot/ROOT/void"),
        attempts: 3,
        target: Target::Snapshot(snapshot_name("zroot/ROOT/void@pre-upgrade")),
    });
    Snapshot {
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
    }
}

#[allow(
    clippy::unreachable,
    reason = "the names of the demo machine are literals that parse"
)]
fn pool(name: &str, health: Health) -> Pool {
    Pool {
        name: name
            .parse()
            .unwrap_or_else(|_| unreachable!("demo names are valid")),
        health,
        writable: false,
        imported: true,
        foreign_hostid: None,
        checkpoint: name == "tank",
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

fn environments(options: &DemoOptions) -> Vec<Environment> {
    let environment =
        |name: &str, is_default, encryption, kernels, command_line, snapshots| Environment {
            name: dataset(name),
            is_default,
            encryption,
            kernels,
            command_line,
            snapshots,
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
            many_snapshots("zroot/ROOT/void", options.snapshots),
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
            few_snapshots("zroot/ROOT/ubuntu"),
        ),
        environment(
            "tank/ROOT/arch",
            false,
            Encryption::None,
            kernels(&["6.11.5-arch1-1", "6.10.10-arch1-1"], None),
            CommandLine {
                value: untrusted("rw quiet splash"),
                origin: Origin::Local,
            },
            few_snapshots("tank/ROOT/arch"),
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
            Vec::new(),
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
/// yesterday's upgrade.
fn many_snapshots(dataset: &str, count: usize) -> Vec<SnapshotInfo> {
    let mut snapshots = vec![SnapshotInfo {
        name: snapshot_name(&format!("{dataset}@pre-upgrade")),
        created: NOW - DAY / 2,
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
