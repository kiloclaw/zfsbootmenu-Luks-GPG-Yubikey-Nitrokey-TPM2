use zbm_platform::Files as _;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::saved_logs::{Destination, MOUNT_POINT};

/// What must never reach a report: a passphrase, the sealed TPM2 key and
/// the Clevis JWE of the tokens, and a key slot's salt.
const SECRETS: &[&str] = &[
    "correct horse battery staple",
    "TPM2-BLOB-SECRET",
    "JWE-SECRET",
    "SALT-SECRET",
];

/// The LUKS2 metadata of a volume with a TPM2 and a Clevis token.
const METADATA: &str = r#"{
  "keyslots": {"0": {"type": "luks2", "kdf": {"salt": "SALT-SECRET"}}},
  "tokens": {
    "0": {"type": "systemd-tpm2", "keyslots": ["1"], "tpm2-blob": "TPM2-BLOB-SECRET"},
    "1": {"type": "clevis", "keyslots": ["2"], "jwe": {"ciphertext": "JWE-SECRET"}}
  }
}"#;

/// A machine with a pool, a LUKS volume with tokens, a TPM, and a journal
/// whose secret field was redacted when it was written.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner.respond(
        "lsblk -rnpo NAME,PARTTYPE",
        0,
        "/dev/vda1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n",
    );
    fake.runner
        .respond("zpool status -P", 0, "  pool: tank\n state: ONLINE\n");
    fake.runner.respond(
        "blkid -c /dev/null -o device -t TYPE=crypto_LUKS",
        0,
        "/dev/vda2\n",
    );
    fake.runner
        .respond("cryptsetup luksUUID /dev/vda2", 0, "2a6f-luks\n");
    fake.runner.respond(
        "cryptsetup luksDump --dump-json-metadata /dev/vda2",
        0,
        METADATA,
    );
    // The kernel log as reading `/dev/kmsg` gives it.
    fake.files.file(
        "/dev/kmsg",
        "6,1,100000,-;mem auto-init: stack:all(zero), heap alloc:on, heap free:on\n\
         6,2,1000000,-;zfsbootmenu: started\n",
    );
    fake.files
        .file(
            "/etc/zbm-release",
            "NAME=\"ZFSBootMenu\"\nVERSION=\"3.1.0\"\nBUILD_ID=\"2026-10-24\"\nHOME_URL=x\n",
        )
        .file("/proc/sys/kernel/osrelease", "7.3.0-9-generic\n")
        .file("/sys/module/zfs/version", "2.4.4-1\n")
        .file("/dev/tpmrm0", "")
        .file(
            "/zfsbootmenu/journal.jsonl",
            "{\"message\":\"unlocking\",\"passphrase\":\"[redacted]\"}\n",
        )
        .file(
            "/zfsbootmenu/crash-1767225600-77.txt",
            "ZFSBootMenu crash report\n",
        )
        .file("/zfsbootmenu/console-scrollback.txt", "$ zpool status\n")
        .file("/zfsbootmenu/screenshot-1.txt", "$\n")
        .file("/zfsbootmenu/screenshot-1.png", "PNG")
        .file("/zfsbootmenu/screenshot-1.jpg", "not a screenshot");
    fake
}

#[test]
fn the_summary_says_what_the_machine_is_and_how_it_booted() {
    let fake = machine();

    let text = summary(&fake.system(), Path::new("/zfsbootmenu"), 1_767_225_600);

    assert!(
        text.starts_with("ZFSBootMenu report, 2026-01-01T00-00-00Z\n"),
        "{text}"
    );
    for expected in [
        "VERSION=\"3.1.0\"\n",
        "kernel: 7.3.0-9-generic\nzfs: 2.4.4-1\n",
        "  pool: tank\n state: ONLINE\n",
        "/dev/vda2: UUID 2a6f-luks; tokens: 0 systemd-tpm2 (key slots 1); 1 clevis (key slots 2)\n",
        "/dev/tpmrm0: present, not in lockout\n",
        "== memory\nfreed memory is zeroed (init_on_free)\n",
        "== crash reports\ncrash-1767225600-77.txt\n",
    ] {
        assert!(text.contains(expected), "{expected:?} in:\n{text}");
    }
    assert!(!text.contains("HOME_URL"), "{text}");
}

#[test]
fn a_bare_machine_has_little_to_report() {
    let fake = FakeSystem::new();

    let text = summary(&fake.system(), Path::new("/zfsbootmenu"), 0);

    for expected in [
        "kernel: unknown\nzfs: unknown\n",
        "== pools\nno pool is imported\n",
        "== LUKS volumes\nnone\n",
        "== TPM\nnone\n",
        "== memory\nunknown whether freed memory is zeroed\n",
        "== crash reports\nnone\n",
    ] {
        assert!(text.contains(expected), "{expected:?} in:\n{text}");
    }
}

#[test]
fn a_report_holds_no_key_material() {
    let fake = machine();
    let system = fake.system();

    let dir = write(&system, Path::new("/zfsbootmenu"), Path::new("/media/usb")).unwrap();

    assert_eq!(dir, Path::new("/media/usb/zbm-report-2026-01-01T00-00-00Z"));
    let names: Vec<String> = fake
        .files
        .read_dir(&dir)
        .unwrap()
        .into_iter()
        .map(|name| name.into_string().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "console-scrollback.txt",
            "crash-1767225600-77.txt",
            "journal.jsonl",
            "kernel.log",
            "report.txt",
            "screenshot-1.png",
            "screenshot-1.txt",
        ]
    );
    for name in names {
        let text = fake.files.text(dir.join(&name)).unwrap();
        for secret in SECRETS {
            assert!(!text.contains(secret), "{secret} in {name}:\n{text}");
        }
    }
}

#[test]
fn a_report_goes_to_the_esp_or_says_why_not() {
    let fake = machine();
    let system = fake.system();

    let (device, dir) = write_on(&system, Path::new("/zfsbootmenu"), &Destination::Esp).unwrap();

    assert_eq!(device, "/dev/vda1");
    assert_eq!(
        dir,
        "EFI/zfsbootmenu/reports/zbm-report-2026-01-01T00-00-00Z"
    );
    let report = Path::new(MOUNT_POINT).join(&dir).join("report.txt");
    assert!(fake.files.exists(&report));
    fake.files
        .fail("/media/full", std::io::ErrorKind::PermissionDenied);
    let error = write(&system, Path::new("/zfsbootmenu"), Path::new("/media/full")).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("unable to write /media/full/"),
        "{error}"
    );
}

#[test]
fn zeroing_of_freed_memory_is_read_from_the_kernel_log_or_command_line() {
    let off = "[0.0] mem auto-init: stack:off, heap alloc:on, heap free:off\n";
    assert_eq!(zeroes_freed_memory(off, "init_on_free=1"), Some(false));
    assert_eq!(zeroes_freed_memory("", "quiet init_on_free=1"), Some(true));
    assert_eq!(zeroes_freed_memory("", "init_on_free=0"), Some(false));
    assert_eq!(zeroes_freed_memory("", "quiet"), None);
    // The kernel takes the last one; quotes do not hide it.
    assert_eq!(
        zeroes_freed_memory("", "init_on_free=0 \"init_on_free=1\""),
        Some(true)
    );
    let fake = FakeSystem::new();
    fake.files.file("/proc/cmdline", "ro init_on_free=0\n");
    let text = summary(&fake.system(), Path::new("/zfsbootmenu"), 0);
    assert!(text.contains("not zeroed: add init_on_free=1"), "{text}");
}
