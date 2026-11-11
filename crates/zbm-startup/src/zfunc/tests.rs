//! The functions of the library on a fake machine.

use zbm_platform::console::Screen;
use zbm_platform::exec::{Input, Stream};
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// Runs `function` on `fake` with `args` and the configuration `conf`.
fn outcome(fake: &FakeSystem, conf: &str, function: &str, args: &[&str]) -> Outcome {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    run(
        &fake.system(),
        &Conf::parse(conf),
        function,
        &args,
        &mut Vec::new(),
    )
}

/// The status of a function that ran.
fn call(fake: &FakeSystem, conf: &str, function: &str, args: &[&str]) -> i32 {
    match outcome(fake, conf, function, args) {
        Outcome::Status(status) => status,
        Outcome::Legacy(command) => panic!("legacy: {}", command.display()),
    }
}

/// An image with the legacy library and a state directory.
fn image() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .dir("/zfsbootmenu")
        .file("/usr/share/zfsbootmenu/legacy/lib/zfsbootmenu-core.sh", "");
    fake
}

/// ADR 0017, point 10: a function without Rust code runs as the legacy
/// one, on Bash, with the caller's streams, the configuration where the
/// environment does not set a variable, and the function's status.
#[test]
fn other_functions_run_from_the_legacy_files() {
    let fake = image();
    fake.environment.set("BASE", "/elsewhere");
    let conf = "export BASE=\"/zfsbootmenu\"\nexport loglevel=\"7\"\n";
    let Outcome::Legacy(bash) = outcome(&fake, conf, "import_pool", &["tank", "a b"]) else {
        panic!("not the legacy function");
    };
    assert_eq!(bash.program(), "bash");
    // zfunc replaces itself with Bash: nothing ran here.
    assert!(fake.runner.lines().is_empty());
    let args: Vec<String> = bash
        .get_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args[0], "-c");
    assert!(args[1].contains("builtin source \"${legacy}${1}\""));
    assert_eq!(
        &args[2..],
        [
            "/libexec/zfunc",
            "/usr/share/zfsbootmenu/legacy",
            "import_pool",
            "tank",
            "a b"
        ]
    );
    assert_eq!(bash.get_stdin(), &Input::Inherit);
    assert_eq!(bash.get_stdout(), Stream::Inherit);
    // The caller's BASE stays; the configuration fills in the rest.
    assert_eq!(bash.get_env(), [("loglevel".into(), Some("7".into()))]);
}

#[test]
fn without_the_legacy_files_nothing_runs() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    assert_eq!(call(&fake, "", "import_pool", &[]), 1);
    assert!(fake.runner.lines().is_empty());
}

#[test]
fn a_restricted_pool_is_one_whole_line_of_degraded() {
    let fake = image();
    fake.environment.set("BASE", "/zfsbootmenu");
    // No file: grep's status.
    assert_eq!(call(&fake, "", "is_degraded", &["tank"]), 2);
    fake.files
        .file("/zfsbootmenu/degraded", "tank\nboot pool\n");
    assert_eq!(call(&fake, "", "is_degraded", &["tank"]), 0);
    assert_eq!(call(&fake, "", "is_degraded", &["boot pool"]), 0);
    assert_eq!(call(&fake, "", "is_degraded", &["tan"]), 1);
    assert_eq!(call(&fake, "", "is_degraded", &[]), 1);
    assert!(fake.runner.lines().is_empty(), "no legacy code ran");
}

/// ADR 0017, point 10, and ADR 0010: the recovery shell of a hook goes
/// through the gate: a locked image asks for the PIN first.
#[test]
fn the_emergency_shell_goes_through_the_gate() {
    use zbm_app::auth::{Authority, Lock};

    // This machine's image is not locked: the shell opens.
    let open = image();
    assert_eq!(call(&open, "", "emergency_shell", &[]), 0);
    assert!(open.console.output_text().contains("emergency shell"));

    let fake = image();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let locked = Authority::new(Lock::On(
        "$argon2id$v=19$m=65536,t=3,p=1$c2FsdA$aGFzaA".to_owned(),
    ));
    let shell = Shell::new(fake.system(), &state).with_authority(&locked);
    assert_eq!(open_shell(&shell, &["a hook asked".to_owned()]), 0);
    let lines = fake.runner.lines();
    assert!(
        lines.iter().any(|line| line.contains("lock shell")),
        "{lines:?}"
    );
    assert!(fake.console.output_text().contains("emergency shell"));
    assert!(fake.console.output_text().contains("a hook asked"));
}

/// The configuration of an image that logs debugging messages.
const DEBUG: &str = "export loglevel=\"7\"\n";

/// The arguments of `command`, as text.
fn args_of(command: &Command) -> Vec<String> {
    command
        .get_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// ADR 0018, point 8: the chroot runs `zfs-chroot` on the console, the
/// name one argument even with a space, which the legacy function split;
/// its status is the function's.
#[test]
fn the_chroot_goes_through_the_applet_as_one_argument() {
    let fake = image();
    fake.runner.respond_program("/libexec/zbm", 3, "");

    assert_eq!(call(&fake, DEBUG, "zfs_chroot", &["tank/ROOT/my be"]), 3);

    let calls = fake.runner.calls();
    let [applet] = calls.as_slice() else {
        panic!("{:?}", fake.runner.lines());
    };
    assert_eq!(applet.program(), "/libexec/zbm");
    assert_eq!(args_of(applet), ["zfs-chroot", "tank/ROOT/my be"]);
    assert_eq!(applet.get_stdin(), &Input::Inherit);
    assert_eq!(
        fake.console.screens(),
        [Screen::Clear, Screen::ShowCursor],
        "tput clear; tput cnorm"
    );
    let kmsg = fake.kmsg();
    assert!(
        kmsg.contains("chroot environment: tank/ROOT/my be"),
        "{kmsg}"
    );
}

#[test]
fn an_empty_filesystem_runs_nothing() {
    for args in [&[][..], &[""][..]] {
        let fake = image();

        assert_eq!(call(&fake, DEBUG, "zfs_chroot", args), 1);

        assert!(fake.runner.lines().is_empty());
        assert!(fake.console.screens().is_empty());
        assert!(fake.kmsg().contains("fs is undefined"), "{}", fake.kmsg());
    }
}

#[test]
fn a_chroot_that_cannot_start_is_127_as_in_a_shell() {
    let fake = image();
    fake.runner.on(|command| {
        (command.program() == "/libexec/zbm").then(|| Err(std::io::Error::other("gone")))
    });

    assert_eq!(call(&fake, "", "zfs_chroot", &["tank/ROOT/be"]), 127);
}

/// ADR 0010 and P17: the chroot of a hook is no way past the lock. The
/// native asks nothing and opens nothing itself, locked or not: it runs
/// the applet, which asks for the PIN in a locked image as `zbm lock
/// shell` does (`zbm`'s `a_locked_image_asks_for_the_pin_before_the_chroot`).
#[test]
fn the_hook_s_chroot_leaves_the_pin_to_the_applet() {
    let run = |hash: Option<&str>| {
        let fake = image();
        if let Some(hash) = hash {
            fake.files.file(zbm_app::auth::LOCK_FILE, hash);
        }
        call(&fake, "", "zfs_chroot", &["tank/ROOT/be"]);
        assert!(!fake.console.output_text().contains("PIN"));
        fake.runner.lines()
    };

    let locked = run(Some("$argon2id$v=19$m=65536,t=3,p=1$c2FsdA$aGFzaA"));

    assert_eq!(locked, ["/libexec/zbm zfs-chroot tank/ROOT/be"]);
    assert_eq!(locked, run(None), "the same, locked or not");
}

/// ADR 0017, point 10: `zfs_chroot` logs, clears the screen, shows the
/// cursor and passes back the status of `zfs-chroot` as the legacy
/// function did, with `tput` and `zfs-chroot` stubbed. The legacy function
/// split a name with a space into words (`bash -c "zfs-chroot ${fs}"`);
/// the native passes it as one argument, which the stub does not compare.
#[test]
fn zfs_chroot_matches_the_legacy_function() {
    use zbm_platform::KernelLog;
    use zbm_testkit::TempDir;
    use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines, path, stub_lines};

    let mut parity = zbm_testkit::parity!("zfs_chroot_matches_the_legacy_function");
    for (fs, status) in [
        ("", 0),
        ("tank/ROOT/be", 0),
        ("tank/ROOT/be@old", 1),
        ("rpool/ROOT/my be", 3),
    ] {
        let input = format!("fs {fs:?}, zfs-chroot exits {status}");
        let legacy: (i32, String, Vec<String>) = parity.legacy(input, || {
            let library =
                std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh")).unwrap();
            let start = library.find("\nzfs_chroot() {\n").unwrap() + 1;
            let end = start + library[start..].find("\n}\n").unwrap() + 3;
            let dir = TempDir::new("zbm-zfs-chroot");
            dir.write_executable("zfs-chroot", "#!/bin/sh\nexit \"${CHROOT_STATUS}\"\n");
            let search = std::env::var("PATH").unwrap_or_default();
            let output = Bash::new()
                .code(LOG_STUBS)
                .code("tput() { echo \"$*\"; }")
                .code(&library[start..end])
                .env("PATH", &format!("{}:{search}", dir.path().display()))
                .env("CHROOT_STATUS", &status.to_string())
                .call("zfs_chroot", &[fs], b"");
            let screens = output.stdout_text().lines().map(str::to_owned).collect();
            let code = output.status.code().unwrap_or(-1);
            (code, log_lines(&output.stderr_text()), screens)
        });

        let fake = image();
        let state = State::new(&fake.files, "/zfsbootmenu");
        let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
        fake.runner
            .respond_program("/libexec/zbm", u8::try_from(status).unwrap(), "");
        let code = zfs_chroot(&fake.system(), &state, &[fs.to_owned()], &mut Vec::new());
        let screens: Vec<String> = fake
            .console
            .screens()
            .into_iter()
            .map(|screen| match screen {
                Screen::Clear => "clear".to_owned(),
                Screen::ShowCursor => "cnorm".to_owned(),
                other => format!("{other:?}"),
            })
            .collect();
        let log = stub_lines(&fake.kmsg());
        assert_eq!((code, log, screens), legacy, "{fs:?}");
    }
}
