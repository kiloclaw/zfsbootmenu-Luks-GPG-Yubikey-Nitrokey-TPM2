use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// Runs `mount_esp` with `args` on `fake`: its exit status, standard
/// output and standard error.
fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let status = run_in(
        &fake.system(),
        &args,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    (
        status,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

/// A machine whose `/dev/vdb` is a block device, with nothing mounted.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .file("/etc/zfsbootmenu.conf", "export loglevel=\"4\"\n")
        .file("/proc/self/mounts", "proc /proc proc rw 0 0\n")
        .dir("/dev")
        .block_device("/dev/vdb");
    fake
}

#[test]
fn the_mount_point_is_printed_and_a_refusal_exits_1() {
    let fake = machine();
    assert_eq!(
        call(&fake, &["/dev/vdb"]),
        (0, "/mnt/vdb\n".to_owned(), String::new())
    );
    assert_eq!(fake.runner.lines(), ["mount /dev/vdb /mnt/vdb"]);

    // What the person typed is shown escaped, here and in the kernel log.
    let refused = "mount_esp: /dev/vdb\\x1b[2J does not exist or is not a block device";
    assert_eq!(
        call(&fake, &["/dev/vdb\x1b[2J"]),
        (1, String::new(), format!("{refused}\n"))
    );
    let log = fake.files.text("/dev/kmsg").unwrap_or_default();
    assert!(
        log.contains(&format!("<3>ZFSBootMenu: {refused}")),
        "{log:?}"
    );
    // A name that is no path is no block device either.
    assert_eq!(
        call(&fake, &["vdb"]).2,
        "mount_esp: vdb does not exist or is not a block device\n"
    );
    // Without a device, or with an empty one, the ESP; this machine has
    // none.
    for args in [&[][..], &[""]] {
        assert_eq!(
            call(&fake, args),
            (
                1,
                String::new(),
                "mount_esp: no EFI system partition found\n".to_owned()
            ),
            "{args:?}"
        );
    }
}

#[test]
fn a_refusal_says_its_cause_too() {
    let fake = machine();
    fake.files
        .fail("/mnt/vdb", std::io::ErrorKind::ReadOnlyFilesystem);

    let refused = "mount_esp: unable to make the mount point /mnt/vdb: \
                   Read-only file system (os error 30)";
    assert_eq!(
        call(&fake, &["/dev/vdb"]),
        (1, String::new(), format!("{refused}\n"))
    );
    let log = fake.files.text("/dev/kmsg").unwrap_or_default();
    assert!(
        log.contains(&format!("<3>ZFSBootMenu: {refused}")),
        "{log:?}"
    );
    assert_eq!(fake.runner.lines(), Vec::<String>::new());
}

#[test]
fn two_arguments_print_the_usage_and_exit_2() {
    let fake = machine();
    assert_eq!(
        call(&fake, &["/dev/vdb", "/dev/vdc"]),
        (USAGE, String::new(), MOUNT_ESP.usage.to_owned())
    );
    assert_eq!(fake.runner.lines(), Vec::<String>::new());

    // On this machine too, before anything is looked at.
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let args = ["a".to_owned(), "b".to_owned()];
    let status = run(
        &args,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    assert_eq!(status, USAGE);
    assert_eq!(String::from_utf8(stderr).unwrap(), MOUNT_ESP.usage);
}
