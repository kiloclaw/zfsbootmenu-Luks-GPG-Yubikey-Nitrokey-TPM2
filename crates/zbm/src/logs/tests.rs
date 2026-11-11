use pretty_assertions::assert_eq;
use zbm_app::Recovery as _;
use zbm_platform::Files as _;
use zbm_startup::shell::Shell;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// What `logs` showed: the pager, and the map it reads keys with.
type Shown = Vec<(Pager, Map)>;

/// Runs `logs` with `args` on `fake`, its lines printed as they are
/// without a terminal: its exit status, standard output and standard
/// error.
fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
    call_with(fake, args, &mut help::printed)
}

/// Runs `logs` with `args` on `fake`, its lines shown with `show`: its
/// exit status, standard output and standard error.
fn call_with(fake: &FakeSystem, args: &[&str], show: Show<'_>) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let status = run_in(
        &fake.system(),
        &args,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
        show,
    );
    (
        status,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

/// Runs `logs` with `args` on `fake` on a terminal, where the pager shows
/// the lines and prints nothing: its exit status, standard output and
/// standard error, and what it showed.
fn on_a_terminal(fake: &FakeSystem, args: &[&str]) -> (i32, String, String, Shown) {
    let mut shown = Vec::new();
    let mut drawn = |pager: &Pager, map: Map, _: &mut dyn std::io::Write| {
        shown.push((pager.clone(), map));
        Ok(())
    };
    let (status, stdout, stderr) = call_with(fake, args, &mut drawn);
    (status, stdout, stderr, shown)
}

/// A machine whose kernel log holds the lines `written`, the first
/// numbered 0, with the state directory `/zfsbootmenu`.
fn machine(written: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.files
        .append(Path::new("/dev/kmsg"), written.as_bytes())
        .unwrap();
    fake
}

/// Logs the lines `written` to the kernel log of `fake`.
fn log(fake: &FakeSystem, written: &str) {
    fake.files
        .append(Path::new("/dev/kmsg"), written.as_bytes())
        .unwrap();
}

/// Marks that errors and warnings were logged on `fake`.
fn mark(fake: &FakeSystem) {
    fake.files
        .file("/zfsbootmenu/have_errors", "")
        .file("/zfsbootmenu/have_warnings", "");
}

#[test]
fn logs_escapes_each_line() {
    let fake = machine(
        "<3>ZFSBootMenu: broken \x1b[31mpool\n\
         <4>ZFSBootMenu: careful\n\
         <6>ZFSBootMenu: \u{202e}info\n",
    );
    assert_eq!(
        call(&fake, &[]),
        (
            0,
            "ZFSBootMenu: broken \\x1b[31mpool\nZFSBootMenu: careful\n".to_owned(),
            String::new()
        )
    );
    assert_eq!(
        call(&fake, &["--level", "info"]).1,
        "ZFSBootMenu: \\u{202e}info\n"
    );
    // Without `/dev/kmsg`, `dmesg`'s lines are escaped too.
    fake.files
        .fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
    fake.runner.respond(
        "dmesg -f user --color=never -l err,warn",
        0,
        "[ 1.0] ZFSBootMenu: \x1b[2Jfrom dmesg\n",
    );
    assert_eq!(call(&fake, &[]).1, "ZFSBootMenu: \\x1b[2Jfrom dmesg\n");
}

#[test]
fn logs_removes_both_markers_and_writes_the_stamp() {
    let fake = machine("<3>ZFSBootMenu: broken\n<4>ZFSBootMenu: careful\n<6>later\n");
    let state = State::new(&fake.files, "/zfsbootmenu");
    mark(&fake);
    assert_eq!(call(&fake, &[]).0, 0);
    assert!(!state.has_errors() && !state.has_warnings());
    assert_eq!(state.errors_since(), Some(2));

    // With nothing to show, all the same.
    let quiet = machine("<6>ZFSBootMenu: fine\n");
    let quiet_state = State::new(&quiet.files, "/zfsbootmenu");
    mark(&quiet);
    assert_eq!(call(&quiet, &[]), (0, String::new(), String::new()));
    assert!(!quiet_state.has_errors() && !quiet_state.has_warnings());
    assert_eq!(quiet_state.errors_since(), Some(0));

    // Without `/dev/kmsg`, `dmesg` shows them, and no stamp is written.
    mark(&fake);
    fake.files
        .fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
    fake.runner.respond(
        "dmesg -f user --color=never -l err,warn",
        0,
        "[ 1.0] ZFSBootMenu: broken\n",
    );
    assert_eq!(call(&fake, &[]).1, "ZFSBootMenu: broken\n");
    assert!(!state.has_errors() && !state.has_warnings());
    assert_eq!(state.errors_since(), Some(2));
}

#[test]
fn logs_removes_only_the_markers_of_the_levels_it_shows() {
    let fake = machine(
        "<3>ZFSBootMenu: broken\n\
         <4>ZFSBootMenu: careful\n\
         <6>ZFSBootMenu: fine\n",
    );
    let state = State::new(&fake.files, "/zfsbootmenu");
    mark(&fake);
    // Neither errors nor warnings: both markers stay, and no stamp.
    assert_eq!(call(&fake, &["--level", "info"]).1, "ZFSBootMenu: fine\n");
    assert!(state.has_errors() && state.has_warnings());
    assert_eq!(state.errors_since(), None);

    // The warnings: `have_warnings` goes, and still no stamp.
    assert_eq!(
        call(&fake, &["--level", "warn"]).1,
        "ZFSBootMenu: careful\n"
    );
    assert!(state.has_errors() && !state.has_warnings());
    assert_eq!(state.errors_since(), None);

    // The errors, with the graver levels: `have_errors` goes, and the
    // stamp is written.
    mark(&fake);
    assert_eq!(call(&fake, &["--level", "err+"]).1, "ZFSBootMenu: broken\n");
    assert!(!state.has_errors() && state.has_warnings());
    assert_eq!(state.errors_since(), Some(2));
}

#[test]
fn an_unknown_level_exits_2() {
    let fake = machine("<3>ZFSBootMenu: broken\n");
    mark(&fake);
    let (status, stdout, stderr) = call(&fake, &["--level", "err,bogus\x1b[2J"]);
    assert_eq!((status, stdout.as_str()), (USAGE, ""));
    assert!(
        stderr.starts_with("logs: err,bogus\\x1b[2J is no list of levels"),
        "{stderr}"
    );
    for args in [&["--level", ""][..], &["--level", "err,"]] {
        assert_eq!(call(&fake, args).0, USAGE, "{args:?}");
    }
    // Nothing was read or forgotten, and `dmesg` did not run.
    assert!(fake.runner.lines().is_empty());
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(state.has_errors() && state.has_warnings());
    assert_eq!(state.errors_since(), None);

    // Names as `dmesg -l` takes them are levels.
    assert_eq!(
        call(&fake, &["--level", "ERR,4"]).1,
        "ZFSBootMenu: broken\n"
    );
    // Wrong options print the usage.
    for args in [
        &["--level"][..],
        &["--trace", "--prompt"],
        &["--level", "err", "--trace"],
        &["--bogus"],
        &["err"],
    ] {
        assert_eq!(
            call(&fake, args),
            (USAGE, String::new(), LOGS.usage.to_owned()),
            "{args:?}"
        );
    }
}

#[test]
fn the_prompt_reads_nothing_without_have_errors() {
    let fake = machine("<3>ZFSBootMenu: broken\n");
    fake.files.file("/zfsbootmenu/have_warnings", "");
    // A read would fail and run `dmesg`.
    fake.files
        .fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
    assert_eq!(
        call(&fake, &["--prompt"]),
        (0, String::new(), String::new())
    );
    assert!(fake.runner.lines().is_empty());
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(state.has_warnings());
    assert!(!fake.files.exists(Path::new("/zfsbootmenu/errors_since")));
}

#[test]
fn the_prompt_shows_only_the_errors_after_the_stamp_and_removes_only_have_errors() {
    let fake = machine(
        "<3>ZFSBootMenu: shown already\n\
         <3>ZFSBootMenu: new \x1b[31merror\n\
         <4>ZFSBootMenu: a warning\n",
    );
    let state = State::new(&fake.files, "/zfsbootmenu");
    state.set_errors_since(0).unwrap();
    mark(&fake);
    assert_eq!(
        call(&fake, &["--prompt"]).1,
        "\nZFSBootMenu: new \\x1b[31merror\n\n"
    );
    assert!(!state.has_errors());
    assert!(state.has_warnings());
    assert_eq!(state.errors_since(), Some(2));

    // Errors marked but none after the stamp: the legacy printer's blank
    // lines.
    fake.files.file("/zfsbootmenu/have_errors", "");
    assert_eq!(call(&fake, &["--prompt"]).1, "\n\n\n");
    assert_eq!(state.errors_since(), Some(2));

    // Without a stamp, every error since the boot.
    fake.files
        .remove_file(Path::new("/zfsbootmenu/errors_since"))
        .unwrap();
    fake.files.file("/zfsbootmenu/have_errors", "");
    assert_eq!(
        call(&fake, &["--prompt"]).1,
        "\nZFSBootMenu: shown already\nZFSBootMenu: new \\x1b[31merror\n\n"
    );

    // Without `/dev/kmsg`, `dmesg` shows every error, and no stamp is
    // written.
    fake.files.file("/zfsbootmenu/have_errors", "");
    fake.files
        .fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
    fake.runner.respond(
        "dmesg -f user --color=never -l err",
        0,
        "[ 1.0] ZFSBootMenu: from dmesg\n",
    );
    assert_eq!(
        call(&fake, &["--prompt"]).1,
        "\nZFSBootMenu: from dmesg\n\n"
    );
    assert!(!state.has_errors());
    assert_eq!(state.errors_since(), Some(2));
}

#[test]
fn the_banner_s_errors_are_not_shown_again_at_the_first_prompt() {
    let fake = machine("<3>ZFSBootMenu: at the boot\n<6>ZFSBootMenu: then\n");
    let state = State::new(&fake.files, "/zfsbootmenu");
    fake.files.file("/zfsbootmenu/have_errors", "");
    Shell::new(fake.system(), &state).emergency_shell("");
    assert!(
        fake.console
            .output_text()
            .contains("ZFSBootMenu: at the boot\n")
    );
    assert_eq!(call(&fake, &["--prompt"]).1, "");

    // An error after the banner is shown alone.
    log(&fake, "<3>ZFSBootMenu: in the shell\n");
    fake.files.file("/zfsbootmenu/have_errors", "");
    assert_eq!(
        call(&fake, &["--prompt"]).1,
        "\nZFSBootMenu: in the shell\n\n"
    );
}

#[test]
fn ztrace_is_logs_trace() {
    let named = |name: &str| {
        let (applet, first) = crate::find(crate::APPLETS, name).unwrap();
        let first: Vec<String> = first.iter().map(|arg| (*arg).to_owned()).collect();
        (applet.name, asked(&first))
    };
    assert_eq!(named("ztrace"), ("logs", Ok(Asked::Trace)));
    let plain = Asked::Levels("err,warn".to_owned(), vec![3, 4]);
    assert_eq!(named("zlogtail"), ("logs", Ok(plain)));

    let fake = machine("<7>ZBM:[1]|f.rs,2|traced\n");
    assert_eq!(
        call(&fake, &["--trace"]).1,
        "[    0.000000]    ZBM:[1]: traced\n[    0.000000]      trace: f.rs#2\n"
    );
}

#[test]
fn logs_on_a_terminal_opens_the_pager() {
    let fake = machine(
        "<3>ZFSBootMenu: broken \x1b[31mpool\n\
         <4>ZFSBootMenu: careful\n\
         <7>ZBM:[1]|f.rs,2|traced\n",
    );
    mark(&fake);
    let (status, stdout, stderr, shown) = on_a_terminal(&fake, &[]);
    assert_eq!((status, stdout.as_str(), stderr.as_str()), (0, "", ""));
    let logs = Pager {
        topic: Topic::Logs,
        lines: vec![
            "ZFSBootMenu: broken \\x1b[31mpool".to_owned(),
            "ZFSBootMenu: careful".to_owned(),
        ],
    };
    assert_eq!(shown, [(logs, Map::default())]);
    // Once shown, they are forgotten, as when printed.
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(!state.has_errors() && !state.has_warnings());
    assert_eq!(state.errors_since(), Some(2));

    // `ztrace` too, its keys read with the map `zbm.menu` sets.
    fake.files
        .file("/zfsbootmenu/zbm.cmdline", "zbm.menu=classic\n");
    let (status, stdout, _, shown) = on_a_terminal(&fake, &["--trace"]);
    assert_eq!((status, stdout.as_str()), (0, ""));
    let trace = Pager {
        topic: Topic::Trace,
        lines: vec![
            "[    0.000000]    ZBM:[1]: traced".to_owned(),
            "[    0.000000]      trace: f.rs#2".to_owned(),
        ],
    };
    assert_eq!(shown, [(trace, Map::Classic)]);

    // Nothing to show opens no pager; without debug lines, it says why.
    let quiet = machine("<6>ZFSBootMenu: fine\n");
    assert_eq!(on_a_terminal(&quiet, &[]).3, []);
    let (status, stdout, _, shown) = on_a_terminal(&quiet, &["--trace"]);
    assert_eq!((status, stdout.as_str(), shown), (0, trace::NONE, vec![]));
}

#[test]
fn a_log_that_cannot_be_shown_exits_1_and_keeps_its_markers() {
    let fake = machine("<3>ZFSBootMenu: broken\n");
    mark(&fake);
    let mut broken = |_: &Pager, _: Map, _: &mut dyn std::io::Write| -> std::io::Result<()> {
        Err(std::io::Error::other("no terminal"))
    };
    let (status, stdout, stderr) = call_with(&fake, &[], &mut broken);
    assert_eq!(
        (status, stdout.as_str(), stderr.as_str()),
        (1, "", "logs: unable to show the kernel log: no terminal\n")
    );
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(state.has_errors() && state.has_warnings());
    assert_eq!(state.errors_since(), None);
}

#[test]
fn the_prompt_never_opens_the_pager() {
    let fake = machine("<3>ZFSBootMenu: broken\n");
    fake.files.file("/zfsbootmenu/have_errors", "");
    let (status, stdout, _, shown) = on_a_terminal(&fake, &["--prompt"]);
    assert_eq!(
        (status, stdout.as_str(), shown),
        (0, "\nZFSBootMenu: broken\n\n", vec![])
    );
}
