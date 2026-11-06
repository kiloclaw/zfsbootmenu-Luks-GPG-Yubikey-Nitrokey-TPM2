use zbm_platform::Files as _;
use zbm_testkit::fakes::FakeSystem;

use super::*;

#[test]
fn the_console_on_the_display_hands_it_back_before_kexec() {
    let fake = FakeSystem::new();
    let system = fake.system();
    let state = State::new(system.files, "/zfsbootmenu");

    hand_over(&Context::new(system), &state);
    assert!(fake.runner.calls().is_empty());

    state.create().unwrap();
    state.set_console(42).unwrap();
    hand_over(&Context::new(system), &state);
    let calls: Vec<String> = fake
        .runner
        .calls()
        .iter()
        .map(zbm_platform::exec::Command::display)
        .collect();
    assert_eq!(calls, ["/libexec/zbm console release"]);
}

#[test]
fn after_a_failed_kexec_the_console_takes_the_display_again() {
    let fake = FakeSystem::new();
    let system = fake.system();
    let state = State::new(system.files, "/zfsbootmenu");

    take_back(&Context::new(system), &state);
    assert!(fake.runner.calls().is_empty());

    state.create().unwrap();
    state.set_released_console(42).unwrap();
    take_back(&Context::new(system), &state);
    let calls: Vec<String> = fake
        .runner
        .calls()
        .iter()
        .map(zbm_platform::exec::Command::display)
        .collect();
    assert_eq!(calls, ["/libexec/zbm console reattach"]);
}

#[test]
fn the_hand_back_goes_into_the_logs_saved_for_the_boot() {
    let fake = FakeSystem::new();
    // The kernel log as reading `/dev/kmsg` gives it.
    fake.files.file(
        "/dev/kmsg",
        "6,1,42000000,-;ZFSBootMenu: graphics: handed back\n",
    );
    fake.files
        .dir("/run/zbm-saved-logs/EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z");
    let system = fake.system();
    let _log =
        zbm_platform::KernelLog::new(&fake.files, "7", std::path::Path::new("/zfsbootmenu"), 1)
            .install();

    let state = State::new(system.files, "/zfsbootmenu");
    keep_hand_back(&Context::new(system), &state, None);
    assert!(fake.runner.lines().is_empty());

    let saved = Saved {
        device: "/dev/vda1".to_owned(),
        directory: "EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z".to_owned(),
        files: Vec::new(),
    };
    keep_hand_back(&Context::new(system), &state, Some(&saved));
    assert!(
        fake.files
            .text("/run/zbm-saved-logs/EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z/kernel.log")
            .is_some_and(|log| log.contains("handed back"))
    );

    fake.runner.respond(
        "mount -t vfat -o rw,noatime /dev/vda1 /run/zbm-saved-logs",
        32,
        "",
    );
    keep_hand_back(&Context::new(system), &state, Some(&saved));
    assert!(
        fake.kmsg()
            .contains("unable to add the GPUs' hand-back to the saved logs")
    );
}

#[test]
fn the_page_cache_is_dropped_and_live_secrets_named_before_kexec() {
    let _secrets = crate::testing::holding_a_secret();
    let fake = FakeSystem::new();
    fake.files.dir("/proc/sys/vm");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let _log =
        zbm_platform::KernelLog::new(&fake.files, "7", std::path::Path::new("/zfsbootmenu"), 1)
            .install();

    wipe_before_kexec(&Context::new(system), &state);
    assert_eq!(fake.files.text(DROP_CACHES).as_deref(), Some("3\n"));
    assert!(!fake.kmsg().contains("still in memory"), "{}", fake.kmsg());
    let held = zbm_platform::secret::Secret::copy_from(b"pass");
    wipe_before_kexec(&Context::new(system), &state);
    assert!(
        fake.kmsg()
            .contains("secrets are still in memory before kexec")
    );
    drop(held);
}

#[test]
fn the_cached_keys_are_removed_before_kexec_but_not_what_they_link_to() {
    let fake = FakeSystem::new();
    fake.files
        .dir("/proc/sys/vm")
        .file("/zfsbootmenu/.keys/pool/keys/a.key", "key")
        .file("/elsewhere/kept", "kept")
        .symlink("/zfsbootmenu/.keys/pool/link", "/elsewhere");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");

    wipe_before_kexec(&Context::new(system), &state);

    assert!(
        !fake
            .files
            .exists(std::path::Path::new("/zfsbootmenu/.keys"))
    );
    assert_eq!(fake.files.text("/elsewhere/kept").as_deref(), Some("kept"));
}

#[test]
fn nothing_selected_is_an_error_and_boots_nothing() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    let _log =
        zbm_platform::KernelLog::new(&fake.files, "7", std::path::Path::new("/zfsbootmenu"), 1)
            .install();

    let nothing = Kernel::new("", "", "");
    assert!(!names_a_kernel(&nothing));
    assert!(fake.kmsg().contains("fs, kernel, initramfs undefined"));
    let selected = Kernel::new("zroot/ROOT/void", "/boot/vmlinuz", "/boot/initramfs.img");
    assert!(names_a_kernel(&selected));
    assert!(
        fake.kmsg()
            .contains("fs: zroot/ROOT/void, kernel: /boot/vmlinuz, initramfs: /boot/initramfs.img"),
        "{}",
        fake.kmsg()
    );
}

#[test]
fn the_remembering_loader_says_whether_its_call_loaded() {
    use zbm_testkit::fakes::{FakeKernelLoader, Loads};

    let kernel = FakeKernelLoader::default();
    let loader = Remembering {
        loader: &kernel,
        loaded: AtomicBool::new(false),
    };
    let load = || loader.load(Path::new("/k"), Path::new("/i"), "quiet");
    kernel.answer(Loads::Refused(8));
    assert!(load().is_err());
    assert!(!loader.loaded.load(Ordering::Relaxed));
    kernel.answer(Loads::Accepted);
    assert!(load().is_ok());
    assert!(loader.loaded.load(Ordering::Relaxed));
    // A start goes to the loader it wraps.
    let _ = loader.start();
    assert_eq!(kernel.starts(), 1);
}
