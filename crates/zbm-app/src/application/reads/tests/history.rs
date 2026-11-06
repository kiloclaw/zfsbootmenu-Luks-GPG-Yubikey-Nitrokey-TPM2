//! Tests of the boot history: the ESP read only with `zbm.save_logs`,
//! mounted read-only and unmounted after.

use pretty_assertions::assert_eq;
use zbm_boot::BootSettings;
use zbm_boot::saved_logs::{Destination, READ_MOUNT_POINT};
use zbm_platform::Context;
use zbm_testkit::fakes::FakeSystem;
use zbm_types::{BootRecord, Untrusted};

use super::super::*;
use crate::auth::{self, Authority};
use crate::model::{History, Recorded};
use crate::{Application as _, Runtime, Through};

/// The partition table of a machine with one ESP, as `lsblk` lists it.
const ONE_ESP: &str = "/dev/vda \n/dev/vda1 C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n";

/// The read-only mount of the ESP.
const MOUNT: &str = "mount -t vfat -o ro,nosuid,nodev,noexec /dev/vda1 /run/zbm-saved-logs-read";

/// The unmount after it.
const UNMOUNT: &str = "umount /run/zbm-saved-logs-read";

/// The directory of the saved boot `name` while the ESP is read.
fn saved(name: &str) -> std::path::PathBuf {
    std::path::Path::new(READ_MOUNT_POINT)
        .join("EFI/zfsbootmenu/logs")
        .join(name)
}

/// A record of a boot of `environment` whose command line holds an
/// escape sequence, as a hostile disk could write.
fn record(environment: &str) -> BootRecord {
    BootRecord {
        time: 1_793_246_955,
        environment: Untrusted::new(environment.to_owned()),
        kernel: Untrusted::new("/boot/vmlinuz-6.12.9_1".to_owned()),
        command_line: Untrusted::new("quiet \u{1b}]0;owned\u{7}".to_owned()),
        loaded_by: Untrusted::new("kexec_file_load".to_owned()),
        ..BootRecord::default()
    }
}

/// A machine with one ESP that holds three saved boots: the newest with a
/// record, one saved from the shell without, and the oldest with a broken
/// one.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, ONE_ESP);
    let text = record("tank/ROOT/be").to_json().unwrap();
    fake.files
        .file(saved("2026-10-23T08-00-00Z").join("boot.json"), text)
        .file(saved("2026-10-22T08-00-00Z").join("kernel.log"), "log")
        .file(saved("2026-10-21T08-00-00Z").join("boot.json"), "{\"sch");
    fake
}

/// The runtime of a boot whose logs are saved on the ESP.
fn saving() -> Runtime {
    Runtime::default().with_boot(BootSettings::default().save_logs(Some(Destination::Esp)))
}

/// The history of `events`, which must be one [`Event::HistoryLoaded`].
fn loaded(events: &[Event]) -> &History {
    match events {
        [Event::HistoryLoaded(history)] => history,
        other => panic!("{other:?}"),
    }
}

#[test]
fn without_save_logs_the_history_is_off_and_nothing_is_mounted() {
    let fake = machine();
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);

    let events = app.apply(&Action::LoadHistory, Through::Quietly(fake.system()));

    assert_eq!(loaded(&events), &History::Off);
    assert!(fake.runner.lines().is_empty(), "{:?}", fake.runner.lines());
    assert!(!may_move_pools(&Action::LoadHistory));
}

#[test]
fn the_saved_boots_come_newest_first_from_the_esp_mounted_read_only() {
    let fake = machine();
    let runtime = saving();
    let app = App::new(Context::new(fake.system()), &runtime);

    let events = app.apply(&Action::LoadHistory, Through::Quietly(fake.system()));

    let History::Saved(boots) = loaded(&events) else {
        panic!("{events:?}");
    };
    let names: Vec<&str> = boots.iter().map(|boot| boot.name.get().as_str()).collect();
    assert_eq!(
        names,
        [
            "2026-10-23T08-00-00Z",
            "2026-10-22T08-00-00Z",
            "2026-10-21T08-00-00Z"
        ]
    );
    // The record's text is kept as it was read, untrusted.
    assert_eq!(boots[0].record, Recorded::Read(record("tank/ROOT/be")));
    assert_eq!(boots[1].record, Recorded::Missing);
    assert_eq!(
        boots[1].files,
        [(Untrusted::new("kernel.log".to_owned()), 3)]
    );
    let Recorded::Unreadable(why) = &boots[2].record else {
        panic!("{boots:?}");
    };
    assert!(why.starts_with("the record cannot be read"), "{why}");
    assert!(!why.contains("sch"), "{why}");
    let lines = fake.runner.lines();
    assert!(lines.contains(&MOUNT.to_owned()), "{lines:?}");
    assert_eq!(lines.last().map(String::as_str), Some(UNMOUNT));
    let be = "tank/ROOT/be".parse().unwrap();
    let last = loaded(&events).last_boot_of(&be);
    assert_eq!(last.map(|record| record.time), Some(1_793_246_955));
    assert_eq!(
        loaded(&events).last_boot_of(&"tank/ROOT/other".parse().unwrap()),
        None
    );
}

#[test]
fn a_machine_without_an_esp_has_its_history_unavailable() {
    let fake = FakeSystem::new();
    let runtime = saving();
    let app = App::new(Context::new(fake.system()), &runtime);

    let events = app.apply(&Action::LoadHistory, Through::Quietly(fake.system()));

    assert_eq!(
        loaded(&events),
        &History::Unavailable {
            why: "no EFI system partition found".to_owned(),
            code: 621,
        }
    );
    assert!(
        fake.runner
            .lines()
            .iter()
            .all(|line| !line.starts_with("mount"))
    );
}

#[test]
fn an_esp_that_does_not_mount_has_its_history_unavailable() {
    let fake = machine();
    fake.runner.respond(MOUNT, 32, "");
    let runtime = saving();
    let app = App::new(Context::new(fake.system()), &runtime);

    let events = app.apply(&Action::LoadHistory, Through::Quietly(fake.system()));

    assert_eq!(
        loaded(&events),
        &History::Unavailable {
            why: "unable to mount /dev/vda1".to_owned(),
            code: 624,
        }
    );
    assert!(!fake.runner.lines().contains(&UNMOUNT.to_owned()));
}

#[test]
fn the_locked_menu_lets_the_history_be_read_without_a_pin() {
    let fake = machine();
    fake.files.file("/zfsbootmenu/bootfs", "tank/ROOT/be\n");
    let runtime = saving();
    let authority = Authority::new(auth::Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime).with_authority(&authority);
    assert!(Action::LoadHistory.is_read());

    for through in [
        Through::Quietly(fake.system()),
        Through::Console(fake.system(), &super::super::super::tests::NoShell),
    ] {
        let events = app.apply(&Action::LoadHistory, through);
        assert!(
            matches!(loaded(&events), History::Saved(boots) if boots.len() == 3),
            "{events:?}"
        );
    }
    assert!(!fake.console.output_text().contains(auth::LOCKED_PROMPT));
}
