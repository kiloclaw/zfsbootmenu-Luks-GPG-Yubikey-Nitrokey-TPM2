#![allow(
    clippy::disallowed_methods,
    reason = "the profile is checked on a program it starts"
)]

use super::*;

#[test]
fn the_keyholder_and_its_programs_gain_no_privileges() {
    assert_eq!(Keyholder.name(), "keyholder");
    assert_eq!(Keyholder.confinement(), Confinement::default());
    let mut command = Command::new("cat");
    command.arg("/proc/self/status");

    let output = command.sandboxed(&Keyholder).unwrap().output().unwrap();

    let status = String::from_utf8_lossy(&output.stdout);
    assert!(status.contains("NoNewPrivs:\t1"), "{status}");
    assert!(status.contains("Seccomp:\t0"), "{status}");
}

#[test]
fn the_console_keeps_two_capabilities_and_reaches_its_places_alone() {
    let console = Console {
        state: "/zfsbootmenu".into(),
        sockets: "/run/zfsbootmenu".into(),
        share: "/usr/share/zfsbootmenu".into(),
        mirror: Some("/nowhere/mirror".into()),
        serial: Some("/dev/ttyS0".into()),
    };

    let confinement = console.confinement();

    assert_eq!(console.name(), "console");
    assert_eq!(confinement.user, None);
    assert_eq!(
        confinement.capabilities,
        Some(vec![Capability::SysTtyConfig, Capability::IpcLock])
    );
    let files = confinement.files.unwrap();
    let access = |path: &str| {
        files
            .iter()
            .find(|(file, _)| file == std::path::Path::new(path))
            .map(|(_, access)| *access)
    };
    assert_eq!(access("/zfsbootmenu"), Some(Access::Full));
    assert_eq!(access("/dev/tty7"), Some(Access::Device));
    assert_eq!(access("/dev/input"), Some(Access::Device));
    // The serial console it serves too.
    assert_eq!(access("/dev/ttyS0"), Some(Access::Device));
    assert_eq!(access("/usr/share/zfsbootmenu"), Some(Access::Read));
    // A mirror not made yet: its directory.
    assert_eq!(access("/nowhere"), Some(Access::Full));
    assert_eq!(access("/etc"), None);
    assert_eq!(access("/dev"), None);
    for call in CONSOLE_CALLS {
        assert!(zbm_sys::sandbox::is_known_call(call), "{call}");
    }
    for refused in ["execve", "kill", "tgkill", "ptrace", "setresuid", "prctl"] {
        assert!(!CONSOLE_CALLS.contains(&refused), "{refused}");
    }
    let described = describe(&Applied {
        user: false,
        landlock: Some(7),
        seccomp: true,
    });
    assert_eq!(described, "no new privileges, seccomp, landlock 7");
    assert_eq!(
        describe(&Applied::default()),
        "no new privileges, no landlock"
    );
}

#[test]
fn the_menu_over_ssh_keeps_no_capability_and_reaches_its_terminal_log_and_sockets() {
    let menu = Menu {
        state: "/zfsbootmenu".into(),
        sockets: "/run/zfsbootmenu".into(),
    };

    let confinement = menu.confinement();

    assert_eq!(menu.name(), "menu");
    assert_eq!(confinement.user, None);
    assert_eq!(confinement.capabilities, Some(Vec::new()));
    let files = confinement.files.unwrap();
    let access = |path: &str| {
        files
            .iter()
            .find(|(file, _)| file == std::path::Path::new(path))
            .map(|(_, access)| *access)
    };
    assert_eq!(access("/dev/tty"), Some(Access::Device));
    assert_eq!(access("/zfsbootmenu"), Some(Access::Full));
    assert_eq!(access("/run/zfsbootmenu"), Some(Access::ReadWrite));
    assert_eq!(access("/dev/kmsg"), Some(Access::ReadWrite));
    for nowhere in ["/dev", "/dev/input", "/dev/dri", "/sys", "/proc", "/etc"] {
        assert_eq!(access(nowhere), None, "{nowhere}");
    }
    for call in MENU_CALLS {
        assert!(zbm_sys::sandbox::is_known_call(call), "{call}");
    }
    // Nothing runs, no thread starts, nothing is signalled or listened on.
    for refused in [
        "execve",
        "execveat",
        "clone",
        "clone3",
        "fork",
        "vfork",
        "kill",
        "tgkill",
        "ptrace",
        "setresuid",
        "prctl",
        "bind",
        "listen",
        "accept4",
        "mount",
    ] {
        assert!(!MENU_CALLS.contains(&refused), "{refused}");
    }
}
