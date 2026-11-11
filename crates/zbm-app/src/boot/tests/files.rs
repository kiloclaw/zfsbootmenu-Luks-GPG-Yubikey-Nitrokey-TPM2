//! The boot of a kernel named by its files, as `zkexec` names one.

use super::*;

/// A kernel named by its files boots only once the key of its boot
/// environment is loaded, as the menu loads it: without the key, nothing
/// is mounted or loaded, and the boot says why.
#[test]
fn a_kernel_by_its_files_loads_the_key_first() {
    let locked = locked();
    let shells = Shells::default();

    let events = with_boot(&locked, |boot| boot.boot_files(&by_files(), &shells));

    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { what, why, .. }))
            if what == "unable to load key for tank/ROOT/be" && !why.is_empty()
    ));
    let ran = lines(&locked);
    assert!(
        ran.iter()
            .any(|line| line == "zfs get -H -o value keystatus tank/ROOT"),
        "{ran:#?}"
    );
    assert!(
        !ran.iter()
            .any(|line| line.starts_with("mount") || line.starts_with("kexec")),
        "{ran:#?}"
    );
    assert!(shells.0.lock().unwrap().is_empty());
}

/// The `teardown.d` hooks of a kernel booted by its files learn what
/// loaded it, as those of a boot from the menu do: `zkexec` loads the
/// kernel itself, where the legacy one ran the stage through
/// `zbm run-hooks`, which could not say.
#[test]
fn zkexec_sets_loaded_by() {
    let fake = machine();
    fake.files
        .file_with_mode("/libexec/hooks/teardown.d/10-look", "#!/bin/sh\n", 0o755);
    let files = fake.files.clone();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let record = std::sync::Arc::clone(&seen);
    fake.runner.on(move |command| {
        if command.program() == "/libexec/hooks/teardown.d/10-look" {
            *record.lock().unwrap() = files.text("/run/zbm/hooks/teardown.d.json");
        }
        None
    });

    let events = with_boot(&fake, |boot| {
        boot.boot_files(&by_files(), &Shells::default())
    });

    let context = seen.lock().unwrap().clone();
    let context = context.unwrap_or_else(|| panic!("the hook ran: {:#?}", lines(&fake)));
    assert!(
        context.contains(r#""loaded_by":"kexec""#)
            || context.contains(r#""loaded_by":"kexec_file_load""#),
        "{context}"
    );
    assert!(
        context.contains(r#""kernel":"/boot/vmlinuz-6.1.10""#),
        "{context}"
    );
    // The fake's kexec -e returns: the boot failed.
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { what, .. })) if what == "unable to boot tank/ROOT/be"
    ));
}

/// A kernel named by its files boots with the command line a boot from
/// the menu would give it: the boot environment is searched again first,
/// which drops a command line cached by an earlier boot and takes the
/// kernel's own `.kcl` file.
#[test]
fn a_kernel_by_its_files_boots_with_the_command_line_of_a_menu_boot() {
    let fake = machine();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let cached = state.kcl_cache(BE, "default").unwrap();
    fake.files.file(cached, "stale=1\n");
    let mnt = PathBuf::from("/zfsbootmenu/environments")
        .join(BE)
        .join("mnt/boot");
    fake.files.file(mnt.join("vmlinuz-6.1.10.kcl"), "fresh=1\n");

    let _ = with_boot(&fake, |boot| {
        boot.boot_files(&by_files(), &Shells::default())
    });

    let loaded = lines(&fake);
    let load = loaded
        .iter()
        .find(|line| line.starts_with("kexec -a -l"))
        .unwrap_or_else(|| panic!("{loaded:#?}"));
    assert!(load.contains("fresh=1"), "{load}");
    assert!(!load.contains("stale=1"), "{load}");

    // A boot environment without kernels of its own still boots the files
    // named.
    let bare = FakeSystem::new();
    bare.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let _ = with_boot(&bare, |boot| {
        boot.boot_files(&by_files(), &Shells::default())
    });
    assert!(
        lines(&bare)
            .iter()
            .any(|line| line.starts_with("kexec -a -l")),
        "{:#?}",
        lines(&bare)
    );
}

/// A kernel of a snapshot does not boot: the key, the mount and the hooks
/// would not agree on what boots.
#[test]
fn a_kernel_of_a_snapshot_does_not_boot() {
    let fake = machine();
    let snapshot = Kernel::new("tank/ROOT/be@snap", "/boot/vmlinuz", "/boot/initramfs.img");

    let events = with_boot(&fake, |boot| boot.boot_files(&snapshot, &Shells::default()));

    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { what, why, .. }))
            if what == "unable to boot tank/ROOT/be@snap"
                && why == &["it is not a boot environment"]
    ));
    assert!(lines(&fake).is_empty(), "{:#?}", lines(&fake));
}
