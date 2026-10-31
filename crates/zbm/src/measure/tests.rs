use pretty_assertions::assert_eq;
use zbm_boot::boot::measured::{Bank, Digests, Event, measure};
use zbm_testkit::fakes::FakeSystem;

use super::*;

const LINE: &str = "root=zfs:tank/ROOT/void           quiet";

fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let code = run_in(&fake.system(), &args, &mut io);
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    (code, text(stdout), text(stderr))
}

/// The events of a boot of the kernel `KERNEL` and the initramfs `INITRD`.
fn boot() -> Vec<Event> {
    let file = |measured, bytes: &[u8]| Event {
        measured,
        digests: Digests::of_bytes(bytes),
    };
    vec![
        file(Measured::Kernel("/boot/vmlinuz-6.1.10".into()), b"KERNEL"),
        file(
            Measured::Initramfs("/boot/initramfs-6.1.10.img".into()),
            b"INITRD",
        ),
        Event::command_line(LINE),
        Event::unsealed(),
    ]
}

/// The log of `events`, with SHA-1 and SHA-256 digests.
fn log_of(events: &[Event]) -> String {
    let banks = [Bank::Sha1, Bank::Sha256];
    (0..)
        .zip(events)
        .map(|(recnum, event)| measure::record(recnum, event, &banks))
        .collect()
}

/// A booted system with this boot's log, and the boot environment
/// `tank/ROOT/void` mounted at `/` with two kernels.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .file(LOG, log_of(&boot()))
        .file(
            MOUNTS,
            "tank/ROOT/void / zfs rw,xattr 0 0\nproc /proc proc rw 0 0\n",
        )
        .file("/boot/vmlinuz-6.1.9", b"OLD")
        .file("/boot/initramfs-6.1.9.img", b"OLDRD")
        .file("/boot/vmlinuz-6.1.10", b"KERNEL")
        .file("/boot/initramfs-6.1.10.img", b"INITRD");
    fake.runner.respond_program("zfs", 0, "-\n");
    fake
}

/// The value lines of PCRs 8 and 9 after `events` in `bank`.
fn expected(events: &[Event], bank: Bank) -> String {
    format!(
        "8:{name}={}\n9:{name}={}\n",
        hex(&replay(events, 8, bank)),
        hex(&replay(events, 9, bank)),
        name = bank.name()
    )
}

#[test]
fn the_log_lists_its_events_and_replays_them_in_each_bank() {
    let fake = machine();

    let (code, out, err) = call(&fake, &["log"]);

    assert_eq!((code, err.as_str()), (0, ""));
    let events = boot();
    let listing = [
        "  0  PCR 9  kernel        /boot/vmlinuz-6.1.10\n".to_owned(),
        "  1  PCR 9  initramfs     /boot/initramfs-6.1.10.img\n".to_owned(),
        format!("  2  PCR 8  command-line  {LINE}\n"),
        "  3  PCR 9  separator     zbm-unsealed\n\n".to_owned(),
        expected(&events, Bank::Sha1),
        expected(&events, Bank::Sha256),
    ]
    .concat();
    assert_eq!(out, listing);
    // Another file, given.
    fake.files.file("/tmp/other.json", log_of(&events[2..3]));
    let (code, out, _) = call(&fake, &["log", "/tmp/other.json"]);
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("9:sha256={}", hex(&[0; 32]))),
        "{out}"
    );
}

#[test]
fn a_log_that_is_missing_or_not_ours_is_said_so() {
    let fake = FakeSystem::new();
    let (code, _, err) = call(&fake, &["log"]);
    assert_eq!(code, 1);
    assert!(
        err.starts_with("zbm measure: unable to read /run/log/"),
        "{err}"
    );
    fake.files
        .file(LOG, "\u{1e}{\"content_type\":\"systemd\"}\n");
    let (code, _, err) = call(&fake, &["log"]);
    assert_eq!(code, 1);
    assert!(err.contains("record 0 of"), "{err}");
}

#[test]
fn a_prediction_reads_the_newest_kernel_and_this_boot_s_command_line() {
    let fake = machine();

    let (code, out, err) = call(&fake, &["predict", "tank/ROOT/void"]);

    assert_eq!((code, err.as_str()), (0, ""));
    let head = format!(
        "kernel        /boot/vmlinuz-6.1.10\ninitramfs     /boot/initramfs-6.1.10.img\n\
         command line  {LINE}\nstart         zeros (no start in this boot's log)\n\n"
    );
    assert_eq!(out, format!("{head}{}", expected(&boot(), Bank::Sha256)));
    let lines = fake.runner.lines();
    assert_eq!(
        lines,
        ["zfs get -H -o value org.zfsbootmenu:kernel tank/ROOT/void"]
    );
}

#[test]
fn a_prediction_takes_the_kernel_the_root_the_command_line_and_the_bank_given() {
    let fake = machine();
    fake.files
        .file("/mnt/be/boot/vmlinuz-6.2", b"NEW")
        .file("/mnt/be/boot/initramfs-6.2.img", b"NEWRD");

    let args = [
        "predict",
        "--root",
        "/mnt/be/",
        "--command-line",
        "root=zfs:tank/ROOT/new",
        "--bank",
        "sha1",
        "tank/ROOT/new",
        "/boot/vmlinuz-6.2",
    ];
    let (code, out, err) = call(&fake, &args);

    assert_eq!((code, err.as_str()), (0, ""));
    let kernel = Kernel {
        dataset: "tank/ROOT/new".to_owned(),
        kernel: "/boot/vmlinuz-6.2".to_owned(),
        initramfs: "/boot/initramfs-6.2.img".to_owned(),
    };
    let line = Event::command_line("root=zfs:tank/ROOT/new");
    let events =
        zbm_boot::boot::measured::predicted(&fake.files, Path::new("/mnt/be"), &kernel, line)
            .unwrap();
    assert!(out.ends_with(&expected(&events, Bank::Sha1)), "{out}");
    // The kernel was given: no property is read.
    assert!(fake.runner.lines().is_empty());
}

#[test]
fn a_prediction_without_its_files_or_a_command_line_is_refused() {
    let fake = machine();
    let refused = |args: &[&str]| {
        let (code, _, err) = call(&fake, args);
        assert_eq!(code, 1, "{args:?}");
        err
    };
    assert!(refused(&["predict", "tank/ROOT/other"]).contains("is not mounted"));
    assert!(refused(&["predict", "tank/ROOT/void", "/boot/vmlinuz-5"]).contains("is no kernel"));
    assert!(refused(&["predict", "--root", "/empty", "tank/ROOT/x"]).contains("has no kernel"));
    assert!(refused(&["predict", "tank//void"]).contains("is no dataset"));
    fake.files.file(LOG, "");
    assert!(refused(&["predict", "tank/ROOT/void"]).contains("give --command-line"));
}

#[test]
fn wrong_arguments_print_the_usage() {
    let fake = machine();
    for args in [
        &[][..],
        &["predict"],
        &["predict", "a", "b", "c"],
        &["predict", "--bank", "md5", "a"],
        &["predict", "--root"],
        &["predict", "--bogus", "a"],
        &["log", "a", "b"],
        &["bogus"],
    ] {
        let (code, out, err) = call(&fake, args);
        assert_eq!((code, out.as_str()), (USAGE, ""), "{args:?}");
        assert!(err.starts_with("Usage: zbm measure"), "{args:?}");
    }
    let (code, out, _) = call(&fake, &["--help"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("Usage: zbm measure"));
}

#[test]
fn a_prediction_gives_what_a_measured_boot_extends() {
    let fake = machine();
    fake.files.device("/dev/tpmrm0");
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("tpm2_getcap pcrs", 0, "selected-pcrs:\n  - sha256: [ 0 ]\n");
    // PCR 9 held the EFI stub's measurement of ZFSBootMenu's initramfs.
    let (zeros, stub) = ("00".repeat(32), "5a".repeat(32));
    fake.runner.respond(
        "tpm2_pcrread sha256:8,9",
        0,
        &format!("  sha256:\n    8 : 0x{zeros}\n    9 : 0x{stub}\n"),
    );
    let state = zbm_state::State::new(&fake.files, "/zfsbootmenu");
    let root: zbm_types::KernelArg = "root=zfs:tank/ROOT/void".parse().unwrap();
    let args: zbm_types::KernelCommandLine = "quiet".parse().unwrap();
    let load = zbm_boot::commandline::KexecLoad {
        mnt: "",
        kernel: "/boot/vmlinuz-6.1.10",
        initramfs: "/boot/initramfs-6.1.10.img",
        dtb: None,
        root: &root,
        args: &args,
        copies: None,
    };
    let _ = measure(&fake.system(), &state, &load);
    let measured = fake.files.text("/zfsbootmenu/tpm-measure.json").unwrap();
    fake.files.file(LOG, &measured);

    let (_, replayed, _) = call(&fake, &["log"]);
    let (_, predicted, _) = call(&fake, &["predict", "tank/ROOT/void"]);

    let values = |text: &str| -> Vec<String> {
        let lines = text.lines().filter(|line| line.starts_with("9:"));
        lines.map(str::to_owned).collect()
    };
    assert_eq!(values(&predicted), values(&replayed));
    assert!(predicted.contains(&load.command_line()));
    assert!(predicted.contains("start         the values in this boot's log, and its hook root"));
    assert!(replayed.contains("  0  PCR 8  start"), "{replayed}");
}

#[test]
fn the_unseal_value_is_pcr_9_after_the_start_and_the_hooks() {
    let fake = machine();
    let start = Event::start(9, Digests::new(vec![(Bank::Sha256, vec![3; 32])]));
    let hook = Event::hook("early-setup.d/10-net", b"net");
    let mut events = vec![start.clone(), hook.clone()];
    events.extend(boot());
    // SHA-256 alone, as the start has no other digest.
    let records: String = (0..)
        .zip(&events)
        .map(|(recnum, event)| measure::record(recnum, event, &[Bank::Sha256]))
        .collect();
    fake.files.file(LOG, &records);

    let (code, out, err) = call(&fake, &["unseal"]);

    assert_eq!((code, err.as_str()), (0, ""));
    let expected = replay(&[start.clone(), hook.clone()], 9, Bank::Sha256);
    assert_eq!(out, format!("9:sha256={}\n", hex(&expected)));
    // A prediction builds on the start and the hooks too.
    let (_, predicted, _) = call(&fake, &["predict", "tank/ROOT/void"]);
    let mut whole = vec![start, hook];
    whole.extend(boot());
    let nine = format!("9:sha256={}", hex(&replay(&whole, 9, Bank::Sha256)));
    assert!(predicted.contains(&nine), "{predicted}");
    // Another file, and one that is missing.
    fake.files.file("/tmp/other.json", &records);
    assert_eq!(call(&fake, &["unseal", "/tmp/other.json"]).1, out);
    assert_eq!(call(&fake, &["unseal", "/tmp/missing.json"]).0, 1);
}
