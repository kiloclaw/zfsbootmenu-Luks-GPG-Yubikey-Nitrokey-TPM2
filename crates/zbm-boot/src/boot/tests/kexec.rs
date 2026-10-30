//! The boot through the `KernelLoader` port (ROADMAP.md, SB4).

use zbm_testkit::fakes::{FakeSystem, Loads};

use super::*;

#[test]
fn the_kernel_loads_the_boot_environment_s_kernel_and_kexec_starts_it_when_it_cannot() {
    let _secrets = crate::testing::booting();
    let fake = FakeSystem::new();
    let _log = fake_log(&fake);
    fake.files.dir("/zfsbootmenu");
    fake.kernel.answer(Loads::Accepted);
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "-\n");
    fake.runner.respond_program("mount", 0, "");
    fake.runner.respond_program("umount", 0, "");
    fake.runner
        .respond("kexec -e -i", 1, "kexec: no kernel loaded\n");
    let state = State::new(&fake.files, "/zfsbootmenu");
    let selected = Kernel {
        dataset: FS.to_owned(),
        kernel: "/boot/vmlinuz-6.1.10".to_owned(),
        initramfs: "/boot/initramfs-6.1.10.img".to_owned(),
    };
    let shells = Shells::default();

    kexec_kernel(
        &fake.system(),
        &state,
        &BootSettings::default(),
        &selected,
        &shells,
    );

    let loaded = fake.kernel.loaded();
    assert_eq!(loaded.len(), 1, "{:?}", fake.runner.lines());
    let mnt = loaded[0]
        .kernel
        .to_string_lossy()
        .replace("/boot/vmlinuz-6.1.10", "");
    assert_eq!(
        loaded[0].initrd,
        std::path::PathBuf::from(format!("{mnt}/boot/initramfs-6.1.10.img"))
    );
    assert!(
        loaded[0]
            .command_line
            .starts_with(&format!("root=zfs:{FS}")),
        "{loaded:?}"
    );
    let lines = fake.runner.lines();
    assert!(
        !lines.iter().any(|line| line.starts_with("kexec -a -l")),
        "{lines:?}"
    );
    // Its start failed in the fake: the program tried, and the boot failed.
    assert_eq!(fake.kernel.starts(), 1);
    assert_eq!(lines.last().map(String::as_str), Some("kexec -e -i"));
    assert!(fake.console.output_text().contains("failed!"));
}

#[test]
fn under_lockdown_a_kernel_the_call_refuses_is_not_loaded_by_kexec() {
    let _secrets = crate::testing::booting();
    let fake = FakeSystem::new();
    let _log = fake_log(&fake);
    fake.files.dir("/zfsbootmenu");
    fake.files.file(
        "/sys/kernel/security/lockdown",
        "none [integrity] confidentiality\n",
    );
    // EKEYREJECTED: an unsigned kernel.
    fake.kernel.answer(Loads::Refused(129));
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "-\n");
    fake.runner.respond_program("mount", 0, "");
    fake.runner.respond_program("umount", 0, "");
    let state = State::new(&fake.files, "/zfsbootmenu");
    let selected = Kernel {
        dataset: FS.to_owned(),
        kernel: "/boot/vmlinuz-6.1.10".to_owned(),
        initramfs: "/boot/initramfs-6.1.10.img".to_owned(),
    };
    let shells = Shells::default();

    kexec_kernel(
        &fake.system(),
        &state,
        &BootSettings::default(),
        &selected,
        &shells,
    );

    let lines = fake.runner.lines();
    // The kernel's lockdown is read from securityfs, which nothing mounted.
    let securityfs =
        "mount -t securityfs securityfs /sys/kernel/security -o ro,nosuid,nodev,noexec";
    assert!(lines.iter().any(|line| line == securityfs), "{lines:?}");
    let lines = fake.runner.lines();
    assert!(
        !lines.iter().any(|line| line.starts_with("kexec")),
        "{lines:?}"
    );
    assert_eq!(fake.kernel.starts(), 0);
    assert!(
        fake.console
            .output_text()
            .contains("Unable to load kernel or initramfs into memory")
    );
    assert!(
        fake.kmsg().contains("lockdown is on: the kernel refused"),
        "{}",
        fake.kmsg()
    );
}

#[test]
fn a_measured_boot_loads_the_copies_and_removes_them_after() {
    let _secrets = crate::testing::booting();
    let fake = FakeSystem::new();
    let _log = fake_log(&fake);
    fake.files.device("/dev/tpmrm0");
    fake.kernel.answer(Loads::Accepted);
    fake.runner
        .respond("tpm2_getcap pcrs", 0, "selected-pcrs:\n  - sha256: [ 0 ]\n");
    fake.runner.respond_program("tpm2_pcrextend", 0, "");
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "-\n");
    fake.runner.respond_program("mount", 0, "");
    fake.runner.respond_program("umount", 0, "");
    fake.runner.respond("kexec -e -i", 1, "");
    let state = State::new(&fake.files, "/zfsbootmenu");
    // The boot environment's files, wherever it is mounted.
    let kernel = "/boot/vmlinuz-6.1.10";
    let initramfs = "/boot/initramfs-6.1.10.img";
    let mnt = crate::boot::mounts::be_location(&state, FS).join("mnt");
    fake.files
        .file(mnt.join(&kernel[1..]), b"KERNEL")
        .file(mnt.join(&initramfs[1..]), b"INITRD");
    let selected = Kernel {
        dataset: FS.to_owned(),
        kernel: kernel.to_owned(),
        initramfs: initramfs.to_owned(),
    };

    kexec_kernel(
        &fake.system(),
        &state,
        &BootSettings::default(),
        &selected,
        &Shells::default(),
    );

    let loaded = fake.kernel.loaded();
    assert_eq!(loaded.len(), 1, "{:?}", fake.runner.lines());
    let copies = std::path::Path::new("/zfsbootmenu/measured");
    assert_eq!(loaded[0].kernel, copies.join("kernel"));
    assert_eq!(loaded[0].initrd, copies.join("initramfs"));
    // Loaded, the copies go.
    assert!(
        !fake
            .files
            .paths()
            .iter()
            .any(|path| path.starts_with(copies) && path != copies)
    );
    let log = fake
        .files
        .text("/zfsbootmenu/tpm-measure.json")
        .unwrap_or_default();
    assert!(log.contains("zbm-unsealed"), "{log}");
}
