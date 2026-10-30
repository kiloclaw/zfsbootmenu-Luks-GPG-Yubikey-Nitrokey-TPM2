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
