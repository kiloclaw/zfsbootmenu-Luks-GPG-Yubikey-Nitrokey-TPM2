use super::*;
use std::io;
use zbm_testkit::fakes::{FakeFiles, FileOperation};
use zbm_testkit::legacy::{Bash, LOG_STUBS};
use zbm_testkit::{Rng, TempDir, parity};

fn path(text: &str) -> PathBuf {
    PathBuf::from(text)
}

#[test]
fn the_console_s_records() {
    let files = FakeFiles::new();
    let state = State::new(&files, BASE);
    state.create().unwrap();

    assert_eq!(state.console_pid(), None);
    state.set_console(42).unwrap();
    assert_eq!(state.released_console_pid(), None);
    state.set_released_console(43).unwrap();
    assert_eq!(state.released_console_pid(), Some(43));
    state.clear_released_console();
    assert_eq!(state.released_console_pid(), None);
    assert_eq!(files.text("/zfsbootmenu/console").unwrap(), "42\n");
    assert_eq!(state.console_pid(), Some(42));
    state.set_console_layout("de").unwrap();
    assert_eq!(state.console_layout().as_deref(), Some("de"));
    state.set_console_font("ter-v32b.psf 16x32, 80x25").unwrap();
    assert_eq!(
        state.console_font().as_deref(),
        Some("ter-v32b.psf 16x32, 80x25")
    );
    let shot = state.save_screenshot(b"$\n", b"PNG").unwrap();
    assert_eq!(shot, Path::new("/zfsbootmenu/screenshot-1.png"));
    let next = state.save_screenshot(b"$\n", b"PNG").unwrap();
    assert_eq!(next, Path::new("/zfsbootmenu/screenshot-2.png"));
    assert_eq!(
        files
            .read_to_string(Path::new("/zfsbootmenu/screenshot-2.txt"))
            .unwrap(),
        "$\n"
    );
    for (name, is_shot) in [
        ("screenshot-1.png", true),
        ("screenshot-12.txt", true),
        ("screenshot-.png", false),
        ("screenshot-1.jpg", false),
        ("screenshot-x.txt", false),
    ] {
        assert_eq!(is_screenshot(name), is_shot, "{name}");
    }
    for number in 3..=MAX_SCREENSHOTS {
        files.file(format!("/zfsbootmenu/screenshot-{number}.png"), "PNG");
    }
    assert!(state.save_screenshot(b"$\n", b"PNG").is_err());
    let saved = state.save_scrollback(b"$ ls\n").unwrap();
    assert_eq!(saved, Path::new("/zfsbootmenu/console-scrollback.txt"));
    assert_eq!(files.read_to_string(&saved).unwrap(), "$ ls\n");
    state.clear_console();
    assert_eq!(state.console_pid(), None);
    assert_eq!(state.console_layout(), None);
    assert_eq!(state.console_font(), None);
}

#[test]
fn markers_and_locks() {
    let files = FakeFiles::new();
    let state = State::new(&files, BASE);
    state.create().unwrap();
    assert!(!state.is_initialized());
    state.mark_initialized().unwrap();
    assert!(state.is_initialized());

    assert!(!state.is_active());
    assert_eq!(state.active_pid(), None);
    let active = state.take_active(1234).unwrap().unwrap();
    assert!(
        state.take_active(99).unwrap().is_none(),
        "one menu at a time"
    );
    assert_eq!(files.text("/zfsbootmenu/active").unwrap(), "1234\n");
    assert_eq!(state.active_pid(), Some(1234));
    files.file("/zfsbootmenu/active", "  77  \nextra\n");
    assert_eq!(state.active_pid(), Some(77));
    files.file("/zfsbootmenu/active", "garbage\n");
    assert!(state.is_active());
    assert_eq!(state.active_pid(), None);
    drop(active);
    assert!(!state.is_active());
    assert_eq!(files.text("/zfsbootmenu/active"), None);
    // A PID without the lock held is a menu that is gone.
    files.file("/zfsbootmenu/active", "77\n");
    assert_eq!(state.active_pid(), None);
    let mappings = state.lock_mappings().unwrap();
    assert!(state.lock_mappings().is_err(), "the fake does not wait");
    drop(mappings);
    files.dir("/zfsbootmenu/.keys/tank/keys");
    let cache = state.try_lock_key_cache("tank/keys").unwrap();
    assert!(cache.is_some());
    assert!(state.try_lock_key_cache("tank/keys").unwrap().is_none());

    files.file("/zfsbootmenu/have_errors", "");
    assert!(state.has_errors() && !state.has_warnings());
    state.clear_errors();
    assert!(!state.has_errors());
    files.file("/zfsbootmenu/have_warnings", "");
    assert!(state.has_warnings());
    state.clear_warnings();
    assert!(!state.has_warnings());
}

#[test]
fn bootfs_and_host_id() {
    let files = FakeFiles::new();
    files.dir(BASE);
    let state = State::new(&files, BASE);
    assert_eq!(state.bootfs(), None);
    state.set_bootfs("tank/ROOT/void").unwrap();
    assert_eq!(
        files.text("/zfsbootmenu/bootfs").unwrap(),
        "tank/ROOT/void\n"
    );
    assert_eq!(state.bootfs().as_deref(), Some("tank/ROOT/void"));
    // Replaced atomically through a temporary file.
    assert!(matches!(
        files.operations().last(),
        Some(FileOperation::Rename(_, to)) if *to == path("/zfsbootmenu/bootfs")
    ));
    files.file("/zfsbootmenu/bootfs", "\n");
    assert_eq!(state.bootfs(), None);

    assert_eq!(state.spl_hostid(), None);
    state.set_spl_hostid("00bab10c").unwrap();
    assert_eq!(files.text("/zfsbootmenu/spl_hostid").unwrap(), "00bab10c");
    assert_eq!(state.spl_hostid().as_deref(), Some("00bab10c"));
}

#[test]
fn finished_stages_are_listed_in_order() {
    let files = FakeFiles::new();
    files.dir(BASE);
    let state = State::new(&files, BASE);
    assert!(state.finished_stages().is_empty());
    state.finish_stage("10-kmods").unwrap();
    state.finish_stage("20-hostid").unwrap();
    assert_eq!(state.finished_stages(), ["10-kmods", "20-hostid"]);
    assert_eq!(
        files.text("/zfsbootmenu/.stages").as_deref(),
        Some("10-kmods\n20-hostid\n")
    );
}

#[test]
fn degraded_pools_match_whole_names() {
    let files = FakeFiles::new();
    files.dir(BASE);
    let state = State::new(&files, BASE);
    assert!(!state.is_degraded("tank"));
    assert!(state.add_degraded("data2").unwrap());
    assert!(state.add_degraded("a.b").unwrap());
    assert!(!state.add_degraded("data2").unwrap());
    assert_eq!(state.degraded_pools(), ["data2", "a.b"]);
    assert!(state.is_degraded("data2") && state.is_degraded("a.b"));
    // Not a substring or a pattern: the legacy bug fixed with this port.
    assert!(!state.is_degraded("data") && !state.is_degraded("ata2"));
    assert!(!state.is_degraded("axb") && !state.is_degraded(""));
    // The start-up restricts unhealthy pools without checking.
    state.restrict("data2").unwrap();
    assert_eq!(state.degraded_pools(), ["data2", "a.b", "data2"]);
}

#[test]
fn resume_and_command_line_override() {
    let files = FakeFiles::new();
    files.dir(BASE);
    let state = State::new(&files, BASE);
    assert_eq!(state.noresume(), None);
    state.set_noresume().unwrap();
    let first = state.noresume().unwrap();
    state.set_noresume().unwrap();
    assert!(state.noresume().unwrap() > first);

    assert_eq!(state.cmdline_override(), None);
    state.set_cmdline_override("quiet\nro\n").unwrap();
    assert_eq!(state.cmdline_override().as_deref(), Some("quiet\nro\n"));
    state.clear_cmdline_override();
    assert_eq!(state.cmdline_override(), None);
}

#[test]
fn layout() {
    let files = FakeFiles::new();
    let state = State::new(&files, BASE);
    assert_eq!(state.base(), Path::new("/zfsbootmenu"));
    assert_eq!(state.boot_environments(), path("/zfsbootmenu/bootenvs"));
    assert_eq!(
        state.mount_point("tank/ROOT/void"),
        path("/zfsbootmenu/environments/tank/ROOT/void/mnt")
    );
    assert_eq!(
        state.kernels("tank/ROOT/void"),
        path("/zfsbootmenu/environments/tank/ROOT/void/kernels")
    );
    assert_eq!(
        state.kcl_cache("tank/ROOT/void", "/boot/vmlinuz-6.1:x"),
        Some(path(
            "/zfsbootmenu/environments/tank/ROOT/void/kcl/boot:vmlinuz-6.1::x.kcl"
        ))
    );
    assert_eq!(state.kcl_cache("tank/ROOT/void", ""), None);
    assert_eq!(
        state.key_cache("tank/keys"),
        path("/zfsbootmenu/.keys/tank/keys")
    );
    assert_eq!(state.external_hooks(), path("/zfsbootmenu/.external_hooks"));
    assert_eq!(state.errors_since(), path("/zfsbootmenu/errors_since"));
    assert_eq!(state.luks_mappings(), path("/zfsbootmenu/luks-mappings"));
    assert_eq!(state.luks_skipped(), path("/zfsbootmenu/luks-skipped"));
    assert_eq!(
        state.luks_mappings_lock(),
        path("/zfsbootmenu/luks-mappings.lock")
    );
    assert_eq!(
        state.key_cache_mutex("tank/keys", 42),
        path("/zfsbootmenu/.keys/tank/keys/.cachemutex.42")
    );
    assert_eq!(state.ready(), path("/zfsbootmenu/ready"));

    assert!(!files.exists(&path("/zfsbootmenu/environments/tank")));
    let environment = state.environment("tank/ROOT/void").unwrap();
    assert!(files.is_dir(&environment));
}

#[test]
fn write_failures_name_the_file() {
    let files = FakeFiles::new();
    files.dir(BASE);
    files.fail("/zfsbootmenu/initialized", io::ErrorKind::StorageFull);
    let state = State::new(&files, BASE);
    assert_eq!(
        state.mark_initialized().unwrap_err().to_string(),
        "unable to write /zfsbootmenu/initialized: No space left on device"
    );
    files.fail("/zfsbootmenu/environments", io::ErrorKind::PermissionDenied);
    assert!(state.environment("tank/ROOT/void").is_err());
    let missing = State::new(&files, "/missing/base");
    assert!(missing.set_bootfs("x").is_err());
    assert!(missing.add_degraded("x").is_err());
    files.fail("/missing", io::ErrorKind::PermissionDenied);
    assert!(missing.create().is_err());
}

/// The legacy library, loadable outside the image: `zfsbootmenu-core.sh`
/// sources `/lib/zfsbootmenu-kcl.sh`, which the shim finds in `legacy/`.
fn legacy_core() -> Bash {
    Bash::new()
        .code(LOG_STUBS)
        .code(
            r#"source() {
                case "$1" in
                  /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                  *) builtin source "$@" ;;
                esac
              }"#,
        )
        .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
}

#[test]
fn kcl_cache_names_match_the_legacy_function() {
    // Neither implementation touches files here, so the base needs not exist.
    let base = "/zfsbootmenu";
    let files = FakeFiles::new();
    let state = State::new(&files, base);
    let mut parity = parity!("kcl_cache_names_match_the_legacy_function");
    let mut rng = Rng::from_env(4);
    let alphabet = ['a', 'b', '/', ':', '.', '-', '1'];
    let mut kernels = vec![
        "/boot/vmlinuz-6.1".to_owned(),
        "boot/vmlinuz".to_owned(),
        "/a:b/c::d".to_owned(),
        "//x".to_owned(),
    ];
    for _ in 0..40 {
        kernels.push(format!("/{}", rng.string(&alphabet, 12)));
    }
    kernels.push(String::new());
    for kernel in &kernels {
        let (success, stdout): (bool, String) = parity.legacy(format!("{kernel:?}"), || {
            let output = legacy_core().env("BASE", base).call(
                "kernel_kcl_cache",
                &["tank/ROOT/void", kernel],
                b"",
            );
            (output.status.is_success(), output.stdout_text())
        });
        let expected = state
            .kcl_cache("tank/ROOT/void", kernel)
            .map(|path| format!("{}\n", path.display()));
        assert_eq!(success, expected.is_some(), "status for {kernel:?}");
        if success {
            assert_eq!(Some(stdout), expected, "kernel {kernel:?}");
        }
    }
    assert_eq!(state.kcl_cache("tank/ROOT/void", ""), None);
}

#[test]
fn degraded_checks_match_the_legacy_function() {
    let base = "/zfsbootmenu";
    let files = FakeFiles::new();
    let state = State::new(&files, base);
    files.dir(base);
    let recorded = ["data2", "a.b", "rpool", "tank-1"];
    for pool in recorded {
        state.add_degraded(pool).unwrap();
    }
    let degraded = files.contents(state.base().join("degraded")).unwrap();
    let mut pools = vec![
        "data", "data2", "ata2", "a.b", "axb", "rpool", "pool", "tank", "",
    ];
    let mut rng = Rng::from_env(5);
    let generated: Vec<String> = (0..30)
        .map(|_| rng.string(&['a', 'b', '.', '2', 'd', 't'], 6))
        .collect();
    pools.extend(generated.iter().map(String::as_str));
    let mut parity = parity!("degraded_checks_match_the_legacy_function");
    let input = format!("degraded {degraded:?}, pools {pools:?}");
    let legacy: Vec<bool> = parity.legacy(input, || {
        let dir = TempDir::new("zbm-state");
        dir.write("degraded", &degraded);
        let shell = legacy_core().env("BASE", &dir.path().to_string_lossy());
        pools
            .iter()
            .map(|pool| shell.call("is_degraded", &[pool], b"").status.is_success())
            .collect()
    });
    for (pool, expected) in pools.iter().zip(legacy) {
        assert_eq!(state.is_degraded(pool), expected, "pool {pool:?}");
    }
}

#[test]
fn a_boot_is_unattended_when_silent_key_methods_opened_every_volume() {
    let files = FakeFiles::new();
    files.dir("/zfsbootmenu");
    let state = State::new(&files, "/zfsbootmenu");
    // Nothing opened: nobody was needed, nothing to hold to.
    assert_eq!(state.unattended_mappings(), None);
    state.record_unlocked("luks-a", false).unwrap();
    state.record_unlocked("luks-b", false).unwrap();
    assert_eq!(
        state.unattended_mappings(),
        Some(vec!["luks-a".to_owned(), "luks-b".to_owned()])
    );
    // A person opened one: the boot was attended.
    state.record_unlocked("luks-c", true).unwrap();
    assert_eq!(state.unattended_mappings(), None);
    assert!(state.luks_unlocked().ends_with("luks-unlocked"));
}
