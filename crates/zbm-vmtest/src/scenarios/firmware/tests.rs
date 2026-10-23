use std::io::Read;

use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks as recorded, cleanup, scripted, settings};

const UUID: &str = "5d0c3a1e-0000-4000-8000-000000000001";

const STATUS: &str = "ZBMTEST-STATUS-BEGIN\r\n\
    ZS|   pool: zroot\r\nZS|  state: ONLINE\r\nZL| luks-x open zroot\r\nZBMTEST-STATUS-END\r\n\
    ZBMTEST-TEARDOWN dm=[] pools=[] state=[]\r\n";

const BE: &str = "BE-INIT-START\r\nBE-KERNEL: 7.3.1-zbm\r\n\
    BE-ZFS-VERSION: zfs-2.4.4-1 zfs-kmod-2.4.4-1\r\nBE-ROOT: zroot/ROOT/be / zfs rw\r\n\
    BE|   pool: zroot\r\nBE|  state: ONLINE\r\nBE-WRITE: ok\r\n\
    BE-SCRUB: zroot pool 'zroot' is healthy\r\n\
    BE-CMDLINE: root=zfs:zroot/ROOT/be luks.crypttab=no \
    luks.uuid=5d0c3a1e-0000-4000-8000-000000000001\r\n\
    BE-DM| luks-x: 0 8 crypt aes-xts-plain64 :64:logon 0 259:2 32768 4 allow_discards \
    no_read_workqueue no_write_workqueue sector_size:4096\r\nBE-INIT-DONE\r\n";

#[test]
fn the_efi_command_line_names_the_volume_and_the_key_file_on_a_stick() {
    let plain = efi_cmdline(UUID, false);
    let usb = efi_cmdline(UUID, true);

    assert_eq!(
        plain,
        "console=ttyS0 loglevel=4 zbm.timeout=3 zbm.luks=UUID=5d0c3a1e-0000-4000-8000-000000000001"
    );
    assert_eq!(
        usb,
        format!("{plain} zbm.luks_key=/keys/zroot.key:LABEL=ZBMKEY")
    );
}

#[test]
fn the_efi_system_partition_fills_its_128_mib() {
    let size = esp_size();

    assert_eq!(size, "128M");
    assert_eq!(ESP_START * SECTOR, 1 << 20);
}

#[test]
fn a_file_is_copied_into_another_at_an_offset() {
    let settings = settings("firmware-copy");
    fs::create_dir_all(&settings.work).unwrap();
    let (from, to) = (settings.work.join("from"), settings.work.join("to"));
    fs::write(&from, b"abc").unwrap();
    fs::write(&to, [0_u8; 8]).unwrap();

    copy_at(&from, &to, 4).unwrap();

    assert_eq!(fs::read(&to).unwrap(), b"\0\0\0\0abc\0");
    cleanup(&settings);
}

#[test]
fn the_nvme_disk_has_the_esp_and_the_luks_partition() {
    let settings = settings("firmware-nvme");
    let files = Files::new(&settings);

    make_nvme(&files).unwrap();

    let size = fs::metadata(files.nvme()).unwrap().len();
    let mut disk = vec![0_u8; 3 * 4096];
    File::open(files.nvme())
        .unwrap()
        .read_exact(&mut disk)
        .unwrap();
    assert_eq!(size, NVME_SIZE);
    assert_eq!(&disk[4096..4104], b"EFI PART");
    let second = &disk[8192 + 128..8192 + 256];
    assert_eq!(second[32..40], (ESP_START + ESP_SECTORS).to_le_bytes());
    assert_eq!(&second[56..60], &[b'z', 0, b'r', 0]);
    cleanup(&settings);
}

#[test]
fn a_scenario_needs_the_volume_of_the_setup() {
    let settings = settings("firmware-nosetup");
    let test = Test::new(&settings, "firmware-boot").unwrap();

    run(&test, "boot").unwrap();
    fs::create_dir_all(settings.disks("firmware")).unwrap();
    fs::write(Files::new(&settings).uuid(), format!("{UUID}\n")).unwrap();
    run(&test, "other").unwrap();

    assert_eq!(
        recorded(&test),
        [
            "FAIL - run firmware-setup first",
            "FAIL - unknown scenario other"
        ]
    );
    cleanup(&settings);
}

#[test]
fn without_a_stick_zfsbootmenu_waits_then_asks_for_the_passphrase() {
    let settings = settings("firmware-nostick");
    let test = Test::new(&settings, "firmware-nostick").unwrap();
    let script = vec![
        Step::Say("Insert the key device labelled ZBMKEY\r\n"),
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(PW),
        Step::Say(STATUS),
        Step::Say(BE),
    ];
    let (mut vm, guest) = scripted(&test, script);

    let boot = nostick(&test, &mut vm).unwrap();
    checks(&test, &boot, UUID);

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(
        recorded(&test),
        [
            "ok   - after the wait, one passphrase prompt (['luks'])",
            "ok   - ZFSBootMenu imported zroot: ONLINE",
            "ok   - teardown exported pools and closed volumes (dm=[] pools=[] state=[])",
            "ok   - kexec into the ZFS 2.4.4 boot environment",
            "ok   - boot environment kernel 7.3.1-zbm",
            "ok   - boot environment runs ZFS 2.4.4 (zfs-2.4.4-1 zfs-kmod-2.4.4-1)",
            "ok   - root file system zroot/ROOT/be / zfs rw",
            "ok   - boot environment imported zroot: ONLINE (expected ONLINE)",
            "ok   - root dataset is writable in the boot environment",
            "ok   - scrub of zroot in the boot environment: pool 'zroot' is healthy",
            "ok   - boot environment command line has the luks.* parameters and root=zfs:",
            "ok   - boot environment: 4096-byte encryption sectors",
            "ok   - boot environment: TRIM and no-workqueue flags active",
            "ok   - the boot environment did not ask for a passphrase",
        ]
    );
    cleanup(&settings);
}

#[test]
fn a_boot_that_stopped_early_fails_the_common_checks() {
    let settings = settings("firmware-early");
    let test = Test::new(&settings, "firmware-boot").unwrap();
    let boot = Boot {
        prompts: vec!["luks".into(), "be-luks-prompt".into()],
        ..Boot::default()
    };

    checks(&test, &boot, UUID);

    assert_eq!(
        recorded(&test),
        [
            "FAIL - ZFSBootMenu imported zroot: None",
            "FAIL - teardown report missing",
            "FAIL - kexec into the ZFS 2.4.4 boot environment",
            "FAIL - boot environment command line has the luks.* parameters and root=zfs:",
            "FAIL - boot environment: 4096-byte encryption sectors",
            "FAIL - boot environment: TRIM and no-workqueue flags active",
            "FAIL - the boot environment did not ask for a passphrase",
        ]
    );
    cleanup(&settings);
}
