use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// A machine with two pools: `tank`, imported read/write, with a boot
/// environment, another filesystem and two snapshots, and `rpool`,
/// imported read-only, with a boot environment that cannot be mounted;
/// and two EFI system partitions, of which `/dev/vda1` is mounted. `zfs`
/// prints text.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner.respond("zfs version -j", 1, "");
    fake.runner.respond(
        "zfs list -H -p -t filesystem -o name",
        0,
        "tank\ntank/ROOT\ntank/ROOT/be\ntank/home\nrpool\nrpool/ROOT/off\n",
    );
    fake.runner.respond(
        "zfs list -H -p -t snapshot -o name,creation -r tank",
        0,
        "tank/ROOT/be@old\t100\ntank/home@new\t200\n",
    );
    fake.runner.respond(
        "zfs list -H -p -t snapshot -o name,creation -r rpool",
        0,
        "",
    );
    fake.runner.respond(
        "zfs list -H -p -t filesystem -o name,mountpoint,canmount,keystatus",
        0,
        "tank\t/tank\ton\t-\ntank/ROOT\tnone\toff\t-\ntank/ROOT/be\t/\tnoauto\t-\n\
         tank/home\t/home\ton\t-\nrpool\tnone\toff\t-\nrpool/ROOT/off\t/\toff\t-\n",
    );
    fake.runner
        .respond("zpool list -H -o name", 0, "tank\nrpool\n");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.runner
        .respond("zpool get -H -o value readonly rpool", 0, "on\n");
    fake.runner.respond(
        "lsblk -rnpo NAME,PARTTYPE",
        0,
        "/dev/vda \n/dev/vda1 C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n/dev/vda2 6a85cf4d\n\
         /dev/vdb1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n",
    );
    fake.files.file(
        "/proc/self/mounts",
        "proc /proc proc rw 0 0\n/dev/vda1 /mnt/vda1 vfat rw 0 0\n",
    );
    fake
}

/// What Tab completes for `kind` after `prefix` on the machine.
fn listed(fake: &FakeSystem, kind: Completion, prefix: &str) -> Vec<String> {
    list(&fake.system(), kind, prefix)
}

#[test]
fn each_kind_lists_what_the_legacy_completion_listed() {
    let fake = machine();
    // Snapshots, then filesystems, as `zfs-chroot`'s and `mount_zfs`'s.
    assert_eq!(
        listed(&fake, Completion::FilesystemsAndSnapshots, ""),
        [
            "tank/ROOT/be@old",
            "tank/home@new",
            "tank",
            "tank/ROOT",
            "tank/ROOT/be",
            "tank/home",
            "rpool",
            "rpool/ROOT/off",
        ]
    );
    // Filesystems, as `zkexec`'s.
    assert_eq!(
        listed(&fake, Completion::Filesystems, ""),
        [
            "tank",
            "tank/ROOT",
            "tank/ROOT/be",
            "tank/home",
            "rpool",
            "rpool/ROOT/off",
        ]
    );
    // The pools `set_rw_pool` imports read/write, and `set_ro_pool`
    // read-only.
    assert_eq!(listed(&fake, Completion::ReadOnlyPools, ""), ["rpool"]);
    assert_eq!(listed(&fake, Completion::WritablePools, ""), ["tank"]);
    // The EFI system partitions that are not mounted.
    assert_eq!(listed(&fake, Completion::EspPartitions, ""), ["/dev/vdb1"]);
    assert_eq!(listed(&fake, Completion::EfivarfsModes, ""), ["ro", "rw"]);
    // Nothing is mounted or imported to list them: they are only read.
    let reads = [
        "zfs version",
        "zfs list ",
        "zpool list ",
        "zpool get ",
        "lsblk ",
    ];
    let ran = fake.runner.lines();
    assert!(
        ran.iter()
            .all(|line| reads.iter().any(|read| line.starts_with(read))),
        "{ran:?}"
    );
}

#[test]
fn zsnapshots_completes_the_boot_environments_alone() {
    // The legacy completion listed every filesystem, of which `zsnapshots`
    // refuses all but boot environments.
    let fake = machine();
    assert_eq!(
        listed(&fake, Completion::BootEnvironments, ""),
        ["tank/ROOT/be"]
    );
}

#[test]
fn completions_are_filtered_by_their_prefix() {
    let fake = machine();
    assert_eq!(
        listed(&fake, Completion::FilesystemsAndSnapshots, "tank/ROOT/be"),
        ["tank/ROOT/be@old", "tank/ROOT/be"]
    );
    assert_eq!(
        listed(&fake, Completion::FilesystemsAndSnapshots, "tank/home@"),
        ["tank/home@new"]
    );
    assert_eq!(
        listed(&fake, Completion::BootEnvironments, "r"),
        Vec::<String>::new()
    );
    assert_eq!(listed(&fake, Completion::EfivarfsModes, "r"), ["ro", "rw"]);
    assert_eq!(listed(&fake, Completion::EfivarfsModes, "rw"), ["rw"]);
    assert_eq!(
        listed(&fake, Completion::EspPartitions, "/dev/vda"),
        Vec::<String>::new()
    );
}

#[test]
fn a_machine_that_cannot_be_read_completes_nothing() {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zfs", 1, "");
    fake.runner.respond_program("zpool", 1, "");
    fake.runner.respond_program("lsblk", 1, "");
    for kind in Completion::ALL {
        let names = listed(&fake, kind, "");
        let expected: &[&str] = if kind == Completion::EfivarfsModes {
            &["ro", "rw"]
        } else {
            &[]
        };
        assert_eq!(names, expected, "{kind:?}");
    }
}

#[test]
fn a_name_with_a_control_character_is_left_out() {
    let fake = FakeSystem::new();
    fake.runner.respond(
        "lsblk -rnpo NAME,PARTTYPE",
        0,
        "/dev/a\x1bb c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n\
         /dev/vdc1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n\
         vdd1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n",
    );
    // A name that is no device's path is left out too.
    assert_eq!(listed(&fake, Completion::EspPartitions, ""), ["/dev/vdc1"]);
}

#[test]
fn a_name_with_a_space_is_one_shell_word() {
    let fake = FakeSystem::new();
    fake.runner.respond("zfs version -j", 1, "");
    fake.runner.respond(
        "zfs list -H -p -t filesystem -o name",
        0,
        "rpool/my be\nrpool/my bf\n",
    );
    let escaped = ["rpool/my\\ be", "rpool/my\\ bf"];
    // As typed: without the space, or with it behind a backslash.
    assert_eq!(listed(&fake, Completion::Filesystems, "rpool/my"), escaped);
    assert_eq!(
        listed(&fake, Completion::Filesystems, "rpool/my\\ b"),
        escaped
    );
    assert_eq!(
        listed(&fake, Completion::Filesystems, "rpool/my\\ be"),
        ["rpool/my\\ be"]
    );
}
