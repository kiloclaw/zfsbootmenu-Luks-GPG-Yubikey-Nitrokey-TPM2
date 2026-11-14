//! A boot whose filesystem's path in the state is refused (SB19).

use super::*;

/// A kernel named by its files, of a filesystem whose path in the state
/// runs through a link that root planted, to the image's `/etc`, where
/// its kernel waits: nothing is mounted there, no hook runs and no kernel
/// is loaded, and the boot fails as when the filesystem cannot be
/// mounted.
#[test]
fn zkexec_of_a_filesystem_whose_state_path_runs_through_a_link_mounts_nothing() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    fake.files
        .file("/etc/ROOT/be/mnt/boot/vmlinuz-6.1.10", "kernel")
        .file("/etc/ROOT/be/mnt/boot/initramfs-6.1.10.img", "initramfs")
        .file_with_mode("/libexec/hooks/boot-sel.d/10-sel", "#!/bin/sh\n", 0o755)
        .symlink("/zfsbootmenu/environments/tank", "/etc");
    let shells = Shells::default();

    let events = with_boot(&fake, |boot| boot.boot_files(&by_files(), &shells));

    let ran = lines(&fake);
    assert!(
        !ran.iter().any(|line| line.starts_with("mount")
            || line.starts_with("kexec")
            || line.contains("hooks")),
        "{ran:#?}"
    );
    assert!(fake.kernel.loaded().is_empty());
    assert!(
        matches!(events.last(), Some(Event::Finished(Outcome::Failed { .. }))),
        "{events:#?}"
    );
    let shells = shells.0.lock().unwrap();
    assert!(
        shells
            .iter()
            .any(|message| message.contains("unable to mount")),
        "{shells:#?}"
    );
}
