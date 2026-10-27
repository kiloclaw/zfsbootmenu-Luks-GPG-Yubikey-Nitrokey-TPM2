//! Tests of the fake machine: its programs answer from its disks, and its
//! builders set up what they say.

use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_platform::exec::{Command, Input, Output, Runner};
use zbm_platform::files::Files as _;
use zbm_platform::secret::Secret;

use super::*;
use crate::{assert_no_secret_leaked, assert_ran};

/// Runs `command` on `machine`.
fn run(machine: &FakeMachine, command: &Command) -> Output {
    machine.runner.run(command).unwrap()
}

/// `cryptsetup ARGS`.
fn cryptsetup(args: &[&str]) -> Command {
    Command::new("cryptsetup").args(args)
}

/// `cryptsetup open` with `key` on standard input.
fn open(device: &str, name: &str, key: &[u8]) -> Command {
    cryptsetup(&["open", "--type", "luks", "--key-file=-", device, name])
        .stdin(Input::secret(Secret::copy_from(key)))
}

#[test]
fn the_right_passphrase_opens_a_volume_and_a_wrong_one_does_not() {
    let machine = FakeMachine::new().with_luks_volume("/dev/sda2", "pw");

    let wrong = run(&machine, &open("/dev/sda2", "root", b"nope"));
    let right = run(&machine, &open("/dev/sda2", "root", b"pw"));

    assert_eq!(wrong.status.code(), Some(2));
    assert_eq!(right.status.code(), Some(0));
    assert!(machine.is_open("root"));
    assert_eq!(
        machine.files.text("/fake/holders/sda2").as_deref(),
        Some("root\n")
    );
    assert_ran!(
        machine,
        "cryptsetup open --type luks --key-file=- /dev/sda2 root"
    );
    assert_no_secret_leaked!(machine, b"pw", b"nope");
}

#[test]
fn a_key_file_of_the_machine_opens_a_volume() {
    let machine = FakeMachine::new()
        .with_luks_volume("/dev/sda2", "from file")
        .with_file("/etc/root.key", "from file");

    let output = run(
        &machine,
        &cryptsetup(&["open", "--key-file=/etc/root.key", "/dev/sda2", "v"]),
    );

    assert!(output.status.is_success());
    assert!(machine.is_open("v"));
}

#[test]
fn closing_follows_the_exit_codes_of_cryptsetup() {
    let machine = FakeMachine::new()
        .with_luks_volume("/dev/sda2", "pw")
        .with_luks_volume("/dev/sdb2", "pw");
    run(&machine, &open("/dev/sda2", "a", b"pw"));
    machine.open_elsewhere("/dev/sdb2", "b");
    machine.hold_open("b");

    let codes: Vec<Option<i32>> = ["a", "a", "b"]
        .iter()
        .map(|name| run(&machine, &cryptsetup(&["close", name])).status.code())
        .collect();

    assert_eq!(codes, [Some(0), Some(4), Some(5)]);
    assert!(!machine.is_open("a") && machine.is_open("b"));
    assert!(!machine.files.exists(Path::new("/fake/holders/sda2")));
    let reopened = run(&machine, &open("/dev/sdb2", "b", b"pw"));
    assert_eq!(reopened.status.code(), Some(5));
}

#[test]
fn headers_answer_by_device() {
    let machine = FakeMachine::new()
        .with_luks(
            LuksVolume::new("/dev/sda2", "pw")
                .uuid("11111111-2222-3333-4444-555555555555")
                .metadata(r#"{"tokens":{}}"#),
        )
        .with_luks_volume("/dev/sdb2", "pw");
    let text = |args: &[&str]| run(&machine, &cryptsetup(args)).stdout_text();
    let code = |args: &[&str]| run(&machine, &cryptsetup(args)).status.code();

    assert_eq!(code(&["isLuks", "/dev/sda2"]), Some(0));
    assert_eq!(code(&["isLuks", "/dev/sdc2"]), Some(1));
    assert_eq!(
        text(&["luksUUID", "/dev/sda2"]),
        "11111111-2222-3333-4444-555555555555\n"
    );
    assert_eq!(
        text(&["luksDump", "--dump-json-metadata", "/dev/sda2"]),
        r#"{"tokens":{}}"#
    );
    assert_eq!(
        code(&["luksDump", "--dump-json-metadata", "/dev/sdb2"]),
        Some(1)
    );
    assert_eq!(code(&["luksUUID", "/dev/sdc2"]), Some(1));
}

#[test]
fn uuids_are_predictable_and_differ_by_device() {
    let machine = FakeMachine::new()
        .with_luks_volume("/dev/sda2", "pw")
        .with_luks_volume("/dev/sdb2", "pw");
    let a = machine.volume("/dev/sda2").uuid;
    let b = machine.volume("/dev/sdb2").uuid;

    assert_eq!(a, LuksVolume::new("/dev/sda2", "").uuid);
    assert_ne!(a, b);
    assert_eq!(a.len(), 36);
    assert_eq!(
        machine
            .files
            .canonicalize(Path::new(&format!("/dev/disk/by-uuid/{a}")))
            .unwrap(),
        Path::new("/dev/sda2")
    );
}

#[test]
fn blkid_lists_volumes_and_what_open_ones_hold() {
    let machine = FakeMachine::new()
        .with_luks_volume("/dev/sda2", "pw")
        .with_luks(LuksVolume::new("/dev/sdb2", "pw").content("ext4"));
    run(&machine, &open("/dev/sda2", "a", b"pw"));
    run(&machine, &open("/dev/sdb2", "b", b"pw"));
    let blkid = |args: &[&str]| run(&machine, &Command::new("blkid").args(args)).stdout_text();

    assert_eq!(
        blkid(&["-o", "device", "-t", "TYPE=crypto_LUKS"]),
        "/dev/sda2\n/dev/sdb2\n"
    );
    assert_eq!(
        blkid(&["-p", "-s", "TYPE", "-o", "value", "/dev/mapper/a"]),
        "zfs_member\n"
    );
    assert_eq!(
        blkid(&["-p", "-s", "TYPE", "-o", "value", "/dev/mapper/b"]),
        "ext4\n"
    );
}

#[test]
fn pools_are_listed_with_their_vdevs() {
    let machine = FakeMachine::new()
        .with_pool("tank", "/dev/mapper/a")
        .with_pool("data", "/dev/sdc1");
    let zpool = |args: &[&str]| run(&machine, &Command::new("zpool").args(args)).stdout_text();

    assert_eq!(zpool(&["list", "-H", "-o", "name"]), "tank\ndata\n");
    assert!(zpool(&["status", "-P", "tank"]).contains("/dev/mapper/a  ONLINE"));
}

#[test]
fn builders_set_up_files_the_console_and_programs() {
    let machine = FakeMachine::new()
        .with_kernel_cmdline("root=zfs:AUTO quiet")
        .typing("pw");

    assert_eq!(
        machine.files.text("/proc/cmdline").as_deref(),
        Some("root=zfs:AUTO quiet\n")
    );
    assert_eq!(machine.console.remaining().len(), 1);
    assert_eq!(
        machine.system().environment.non_empty("PATH").as_deref(),
        Some(BIN)
    );
    assert!(machine.files.exists(Path::new("/sbin/cryptsetup")));
    assert!(machine.files.exists(Path::new("/zfsbootmenu")));
    assert_eq!(run(&machine, &Command::new("lsblk")).stdout_text(), "1G\n");
    assert!(std::ptr::eq(machine.runner(), &raw const machine.runner));
}

#[test]
#[should_panic(expected = "the fake machine has no LUKS volume on /dev/sdz")]
fn asking_for_a_volume_the_machine_lacks_is_a_mistake_in_the_test() {
    let _ = FakeMachine::default().volume("/dev/sdz");
}
