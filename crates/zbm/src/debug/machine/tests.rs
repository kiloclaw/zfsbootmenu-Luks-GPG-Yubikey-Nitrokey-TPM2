use pretty_assertions::assert_eq;
use zbm_app::model::{
    CommandLine, Environment, Health, Method, Pool, Snapshot, Volume, VolumeState,
};
use zbm_testkit::fakes::FakeSystem;
use zbm_types::Untrusted;

use super::*;

/// A machine with a pool, two boot environments and two volumes.
fn snapshot() -> Snapshot {
    let environment = |name: &str, is_default, encryption, line: &str, origin| Environment {
        name: name.parse().unwrap(),
        is_default,
        encryption,
        kernels: Vec::new(),
        command_line: CommandLine {
            value: Untrusted::new(line.to_owned()),
            origin,
        },
        snapshots: Vec::new(),
        checks: Vec::new(),
        tries: None,
    };
    Snapshot {
        pools: vec![Pool {
            name: "zroot".parse().unwrap(),
            health: Health::Degraded,
            writable: false,
            imported: true,
            foreign_hostid: None,
            checkpoint: true,
        }],
        environments: vec![
            environment(
                "zroot/ROOT/void",
                true,
                Encryption::Unlocked,
                "quiet",
                Origin::Inherited("zroot/ROOT".parse().unwrap()),
            ),
            environment(
                "zroot/ROOT/old",
                false,
                Encryption::None,
                "",
                Origin::Default,
            ),
        ],
        volumes: vec![
            Volume {
                name: Untrusted::new("zroot-a".to_owned()),
                device: Untrusted::new("/dev/sda2".to_owned()),
                state: VolumeState::Open,
                methods: vec![Method::Tpm2Pin, Method::Passphrase],
                pool: Some("zroot".parse().unwrap()),
                tpm2: None,
            },
            Volume {
                name: Untrusted::new("/dev/sdb2".to_owned()),
                device: Untrusted::new("/dev/sdb2".to_owned()),
                state: VolumeState::Failed("wrong".into()),
                methods: vec![Method::Passphrase],
                pool: None,
                tpm2: None,
            },
        ],
        ..Snapshot::default()
    }
}

#[test]
fn volumes_show_their_mapping_state_methods_and_pool() {
    assert_eq!(
        luks_lines(&snapshot()),
        [
            "/dev/sda2\tzroot-a\topen\tmethods: TPM2 PIN, passphrase\tpool: zroot",
            "/dev/sdb2\t/dev/sdb2\tfailed: wrong\tmethods: passphrase",
        ]
    );
    assert_eq!(luks_lines(&Snapshot::default()), ["no LUKS volume"]);
}

#[test]
fn pools_and_boot_environments_show_their_state() {
    assert_eq!(
        pool_lines(&snapshot()),
        [
            "zroot\tDEGRADED\tread-only, checkpoint",
            "  zroot/ROOT/void (default)\tunlocked\tcommand line: quiet (from zroot/ROOT)",
            "  zroot/ROOT/old\tnot encrypted\tcommand line: - (default)",
        ]
    );
    assert_eq!(pool_lines(&Snapshot::default()), ["no pool is imported"]);
    assert_eq!(origin(&Origin::Local), "set here");
}

#[test]
fn the_reports_read_the_machine_through_the_application() {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    let settings = Settings::new("/zfsbootmenu");
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };

    assert_eq!(run_in(&fake.system(), &settings, "pools", &mut io), 0);
    assert_eq!(run_in(&fake.system(), &settings, "luks", &mut io), 0);
    let broken = FakeSystem::new();
    broken.runner.respond_program("zpool", 1, "");
    assert_eq!(run_in(&broken.system(), &settings, "pools", &mut io), 1);

    assert_eq!(
        String::from_utf8(stdout).unwrap(),
        "no pool is imported\nno LUKS volume\n"
    );
    assert!(
        String::from_utf8(stderr)
            .unwrap()
            .starts_with("zbm debug: ")
    );
}
