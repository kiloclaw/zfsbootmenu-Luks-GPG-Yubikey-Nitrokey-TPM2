use zbm_crypt::measure::{Bank, Measured, parse_log, replay};
use zbm_platform::{Installed, KernelLog};
use zbm_testkit::fakes::FakeSystem;
use zbm_types::{KernelArg, KernelCommandLine};

use super::*;

const MNT: &str = "/zfsbootmenu/environments/tank/ROOT/void/mnt";
const BANKS: &str = "selected-pcrs:\n  - sha1: [ 0, 1, 2 ]\n  - sha256: [ 0, 1, 2 ]\n";

/// What PCRs 8 and 9 hold before the boot is measured: PCR 9 the kernel's
/// EFI stub's measurement of ZFSBootMenu's own initramfs.
fn pcrread() -> String {
    let (zero1, zero256) = ("00".repeat(20), "00".repeat(32));
    let (stub1, stub256) = ("AB".repeat(20), "CD".repeat(32));
    format!(
        "  sha1:\n    8 : 0x{zero1}\n    9 : 0x{stub1}\n  sha256:\n    8 : 0x{zero256}\n    \
         9 : 0x{stub256}\n"
    )
}

/// A machine with a TPM whose SHA-1 and SHA-256 banks are active, and a
/// boot environment's kernel and initramfs.
fn machine() -> FakeSystem {
    machine_with(Some("/dev/tpmrm0"), BANKS)
}

/// [`machine`], with the TPM `device`, if any, and its `banks`.
fn machine_with(device: Option<&str>, banks: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    if let Some(device) = device {
        fake.files.device(device);
    }
    fake.runner.respond("tpm2_getcap pcrs", 0, banks);
    fake.runner
        .respond("tpm2_pcrread sha1:8,9+sha256:8,9", 0, &pcrread());
    fake.files
        .file(format!("{MNT}/boot/vmlinuz"), b"KERNEL")
        .file(format!("{MNT}/boot/initramfs.img"), b"INITRD");
    fake
}

/// The kernel log of `fake`, installed.
fn log(fake: &FakeSystem) -> Installed {
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
}

/// Calls `with` with the load of the boot environment's kernel and
/// initramfs, with `quiet`.
fn with_load<T>(with: impl FnOnce(&KexecLoad<'_>) -> T) -> T {
    let root: KernelArg = "root=zfs:tank/ROOT/void".parse().unwrap();
    let args: KernelCommandLine = "quiet".parse().unwrap();
    with(&KexecLoad {
        mnt: MNT,
        kernel: "/boot/vmlinuz",
        initramfs: "/boot/initramfs.img",
        dtb: None,
        root: &root,
        args: &args,
        copies: None,
    })
}

/// The command line of [`with_load`]'s load.
fn command_line() -> String {
    #[allow(
        clippy::redundant_closure_for_method_calls,
        reason = "the method's lifetime is not general enough for `with_load`"
    )]
    with_load(|load| load.command_line())
}

/// The `tpm2_pcrextend` lines `fake` ran, up to the digests.
fn extended(fake: &FakeSystem) -> Vec<String> {
    fake.runner
        .lines()
        .iter()
        .filter_map(|line| line.strip_prefix("tpm2_pcrextend "))
        .map(|rest| rest.split_once(':').map_or(rest, |(pcr, _)| pcr).to_owned())
        .collect()
}

#[test]
fn the_files_and_the_command_line_go_into_the_tpm_and_the_copies_hold_the_log() {
    let fake = machine();
    let state = State::new(&fake.files, "/zfsbootmenu");

    let copies = with_load(|load| measure(&fake.system(), &state, load)).unwrap();

    assert_eq!(extended(&fake), ["9", "9", "8", "9"]);
    let log = fake.files.text("/zfsbootmenu/tpm-measure.json").unwrap();
    let starts = parse_log(&log).unwrap();
    // The log starts with what the PCRs held: replayed, PCR 9 starts from
    // the stub's value, PCR 8 from zeros.
    assert_eq!(starts[0].measured, Measured::Start(8));
    assert_eq!(starts[1].measured, Measured::Start(9));
    assert_eq!(replay(&starts[..2], 9, Bank::Sha256), [0xcd; 32]);
    assert_eq!(replay(&starts[..2], 8, Bank::Sha1), [0; 20]);
    let events = &starts[2..];
    let measured: Vec<&Measured> = events.iter().map(|event| &event.measured).collect();
    let command_line = command_line();
    assert_eq!(
        measured,
        [
            &Measured::Kernel("/boot/vmlinuz".into()),
            &Measured::Initramfs("/boot/initramfs.img".into()),
            &Measured::CommandLine(command_line),
            &Measured::Unsealed,
        ]
    );
    // The log names the banks the TPM has, and the digests it was given.
    assert!(log.contains("\"sha1\"") && !log.contains("sha384"), "{log}");
    let first = fake
        .runner
        .lines()
        .into_iter()
        .find(|line| line.starts_with("tpm2_pcrextend"));
    let kernel_sha256 = hex(events[0].digests.get(Bank::Sha256).unwrap());
    assert!(first.unwrap().contains(&format!("sha256={kernel_sha256}")));
    // The kernel loads the bytes measured; the initramfs has the log after.
    assert_eq!(fake.files.contents(&copies.kernel).unwrap(), b"KERNEL");
    let initramfs = fake.files.contents(&copies.initramfs).unwrap();
    let archive = measure::cpio(measure::LOG_PATH, log.as_bytes());
    assert_eq!(initramfs, [b"INITRD".as_slice(), &archive].concat());
    assert!(copies.kernel.starts_with("/zfsbootmenu/measured"));

    copies.remove(&fake.system());
    assert!(!fake.files.paths().contains(&copies.kernel));
    assert_eq!(fake.files.contents(&copies.kernel), None);
    assert_eq!(fake.files.contents(&copies.initramfs), None);
}

/// The lower-case hexadecimal of `bytes`.
fn hex(bytes: &[u8]) -> String {
    measure::hex(bytes)
}

#[test]
fn without_a_tpm_nothing_is_measured() {
    let fake = machine_with(None, BANKS);
    let _log = log(&fake);
    let state = State::new(&fake.files, "/zfsbootmenu");

    let copies = with_load(|load| measure(&fake.system(), &state, load));

    assert_eq!(copies, None);
    assert!(fake.runner.lines().is_empty());
    assert!(fake.kmsg().contains("no TPM"), "{}", fake.kmsg());
}

#[test]
fn the_tpm_device_without_its_resource_manager_serves_too() {
    let fake = machine_with(Some("/dev/tpm0"), BANKS);
    let state = State::new(&fake.files, "/zfsbootmenu");

    let copies = with_load(|load| measure(&fake.system(), &state, load));

    assert!(copies.is_some());
    let calls = fake.runner.calls();
    assert!(calls[0].get_env().iter().any(|(key, value)| {
        key == "TPM2TOOLS_TCTI" && value.as_deref() == Some("device:/dev/tpm0".as_ref())
    }));
}

#[test]
fn a_tpm_with_no_active_bank_measures_nothing() {
    let fake = machine_with(Some("/dev/tpmrm0"), "selected-pcrs:\n  - sha256: [ ]\n");
    let _log = log(&fake);
    let state = State::new(&fake.files, "/zfsbootmenu");

    let copies = with_load(|load| measure(&fake.system(), &state, load));

    assert_eq!(copies, None);
    assert!(extended(&fake).is_empty());
    assert!(
        fake.kmsg()
            .contains("PCR banks of the TPM /dev/tpmrm0 could not be read: nothing is measured"),
        "{}",
        fake.kmsg()
    );
}

#[test]
fn an_extension_the_tpm_refuses_is_left_out_of_the_log() {
    let fake = machine();
    let _log = log(&fake);
    let command_line = command_line();
    fake.runner.on(move |command| {
        let arg = command.get_args().first()?.to_string_lossy().into_owned();
        (command.program() == "tpm2_pcrextend" && arg.starts_with("8:")).then(|| {
            Ok(zbm_platform::Output::new(
                zbm_platform::Status::exited(1),
                "",
            ))
        })
    });
    let state = State::new(&fake.files, "/zfsbootmenu");

    let copies = with_load(|load| measure(&fake.system(), &state, load));

    // The boot goes on, with the copies.
    assert!(copies.is_some());
    let log = fake.files.text("/zfsbootmenu/tpm-measure.json").unwrap();
    let events = parse_log(&log).unwrap();
    assert_eq!(events.len(), 5);
    assert!(!log.contains(&command_line), "{log}");
    assert!(log.contains("\"recnum\":4"), "{log}");
    assert!(
        fake.kmsg().contains("PCR 8 was not extended"),
        "{}",
        fake.kmsg()
    );
}

#[test]
fn files_that_cannot_be_read_leave_the_separator_alone() {
    let fake = machine();
    fake.files.fail(
        format!("{MNT}/boot/initramfs.img"),
        std::io::ErrorKind::PermissionDenied,
    );
    let state = State::new(&fake.files, "/zfsbootmenu");

    let copies = with_load(|load| measure(&fake.system(), &state, load));

    assert_eq!(copies, None);
    assert_eq!(extended(&fake), ["9"]);
    let log = fake.files.text("/zfsbootmenu/tpm-measure.json").unwrap();
    assert_eq!(parse_log(&log).unwrap()[2].measured, Measured::Unsealed);
}

#[test]
fn a_second_boot_continues_the_log() {
    let fake = machine();
    let state = State::new(&fake.files, "/zfsbootmenu");

    for _ in 0..2 {
        let _ = with_load(|load| measure(&fake.system(), &state, load));
    }

    // The PCRs' start is read once, before the first extension.
    let log = fake.files.text("/zfsbootmenu/tpm-measure.json").unwrap();
    assert_eq!(parse_log(&log).unwrap().len(), 10);
    assert!(log.contains("\"recnum\":9"), "{log}");
    let reads = fake.runner.lines();
    let reads = reads.iter().filter(|line| line.starts_with("tpm2_pcrread"));
    assert_eq!(reads.count(), 1);
}

#[test]
fn copies_that_cannot_be_written_load_the_boot_environment_s_files() {
    let fake = machine();
    let _log = log(&fake);
    fake.files.fail(
        "/zfsbootmenu/measured/initramfs",
        std::io::ErrorKind::StorageFull,
    );
    let state = State::new(&fake.files, "/zfsbootmenu");

    let copies = with_load(|load| measure(&fake.system(), &state, load));

    assert_eq!(copies, None);
    assert_eq!(extended(&fake).len(), 4);
    assert_eq!(fake.files.contents("/zfsbootmenu/measured/kernel"), None);
    assert!(fake.kmsg().contains("measured copies"), "{}", fake.kmsg());
}

#[test]
fn pcrs_that_cannot_be_read_leave_the_log_without_a_start() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files.dir("/zfsbootmenu").device("/dev/tpmrm0");
    fake.files
        .file(format!("{MNT}/boot/vmlinuz"), b"KERNEL")
        .file(format!("{MNT}/boot/initramfs.img"), b"INITRD");
    fake.runner.respond("tpm2_getcap pcrs", 0, BANKS);
    fake.runner.respond_program("tpm2_pcrread", 1, "");
    let state = State::new(&fake.files, "/zfsbootmenu");

    let copies = with_load(|load| measure(&fake.system(), &state, load));

    assert!(copies.is_some());
    let log = fake.files.text("/zfsbootmenu/tpm-measure.json").unwrap();
    assert_eq!(parse_log(&log).unwrap().len(), 4);
    assert!(
        fake.kmsg().contains("replays from zeros"),
        "{}",
        fake.kmsg()
    );
}

#[test]
fn events_are_extended_after_the_start_and_continue_the_log() {
    let fake = machine();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let hook = Event::hook("early-setup.d/10-hello", b"echo hello\n");

    assert_eq!(
        extend_events(&fake.system(), &state, &[hook]),
        Extended::Extended
    );
    let _ = with_load(|load| measure(&fake.system(), &state, load));

    let log = fake.files.text("/zfsbootmenu/tpm-measure.json").unwrap();
    let kinds: Vec<&str> = parse_log(&log)
        .unwrap()
        .iter()
        .map(|event| event.measured.kind())
        .collect();
    assert_eq!(
        kinds,
        [
            "start",
            "start",
            "hook",
            "kernel",
            "initramfs",
            "command-line",
            "separator"
        ]
    );
    assert_eq!(extended(&fake), ["9", "9", "9", "8", "9"]);
    let reads = fake.runner.lines();
    assert_eq!(
        reads
            .iter()
            .filter(|line| line.starts_with("tpm2_pcrread"))
            .count(),
        1
    );
    let none = machine_with(None, BANKS);
    let state = State::new(&none.files, "/zfsbootmenu");
    assert_eq!(extend_events(&none.system(), &state, &[]), Extended::NoTpm);
}
