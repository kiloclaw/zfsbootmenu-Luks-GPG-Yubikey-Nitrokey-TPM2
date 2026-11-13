//! The commands of the programs: hooks, key scripts, shells, applets and
//! the legacy functions, and the legacy functions' shell code on Bash.

use super::*;
use pretty_assertions::assert_eq;
use std::ffi::OsString;
use zbm_platform::{Secret, Status};
use zbm_testkit::{FakeRunner, TempDir};

/// The command line `hook_command` gives for a hook starting with
/// `start`, with brush chosen or no shell.
fn line(start: Option<&[u8]>, shell: Option<&str>) -> String {
    let command = hook_command(Path::new("/hooks/h"), start, shell.map(Path::new));
    std::iter::once(command.program())
        .chain(command.get_args().iter().map(OsString::as_os_str))
        .map(|word| word.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn hooks_run_as_the_kernel_would_run_them() {
    let brush = Some("/usr/bin/brush");
    // Fix 21: the options of a shell's `#!` line are kept.
    assert_eq!(
        line(Some(b"#!/bin/sh -e\n"), brush),
        "/usr/bin/brush -e /hooks/h"
    );
    assert_eq!(
        line(Some(b"#!/bin/bash\n"), brush),
        "/usr/bin/brush /hooks/h"
    );
    // One brush lacks runs as the kernel runs it, on its interpreter.
    assert_eq!(line(Some(b"#!/bin/bash -o pipefail\n"), brush), "/hooks/h");
    assert_eq!(line(Some(b"#!/bin/sh -p\n"), brush), "/hooks/h");
    assert_eq!(
        line(Some(b"#!/bin/bash -o pipefail\n"), Some("/bin/bash")),
        "/bin/bash -o pipefail /hooks/h"
    );
    // Fix 22: an ELF hook, and other interpreters, run as they are.
    assert_eq!(line(Some(b"\x7fELF\x02\x01"), brush), "/hooks/h");
    assert_eq!(line(Some(b"#!/usr/bin/python3\n"), brush), "/hooks/h");
    assert_eq!(line(Some(b"#!/usr/bin/env bash\n"), brush), "/hooks/h");
    // A text without `#!` runs on the shell, never on `/bin/sh`.
    assert_eq!(line(Some(b"echo hi\n"), brush), "/usr/bin/brush /hooks/h");
    assert_eq!(line(Some(b"echo hi\n"), None), "/bin/bash /hooks/h");
    // Without a chosen shell a `#!` line is the kernel's; unread, it
    // runs as it is.
    assert_eq!(line(Some(b"#!/bin/sh -e\n"), None), "/hooks/h");
    assert_eq!(line(None, brush), "/hooks/h");
}

#[test]
fn a_legacy_function_runs_on_bash_from_the_legacy_files() {
    let args = ["tank".to_owned()];
    let call = legacy_function(
        Path::new(LEGACY_LIBRARY),
        "export_pool",
        &args,
        &[("loglevel", "7")],
    );
    assert_eq!(call.program(), "bash");
    for gated in ["emergency_shell", "zfs_chroot"] {
        let line = format!("\n{gated}() {{ /libexec/zbm zfunc {gated} \"$@\"; }}\n");
        assert!(LEGACY_FUNCTION.contains(&line), "{gated}");
    }
    let args: Vec<String> = call
        .get_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args[1], LEGACY_FUNCTION);
    assert_eq!(
        &args[2..],
        ["/libexec/zfunc", LEGACY_LIBRARY, "export_pool", "tank"]
    );
    assert_eq!(call.get_env(), [("loglevel".into(), Some("7".into()))]);
    assert_eq!(call.get_stdout(), Stream::Inherit);
}

#[test]
fn the_legacy_runner_sources_no_ui_library() {
    // The `fzf` menu's library is not in Rust images (ADR 0018, point
    // 21): sourcing it would fail every legacy function.
    assert!(!LEGACY_FUNCTION.contains("zfsbootmenu-ui.sh"));
    let sourced = "for src in /lib/profiling-lib.sh /lib/kmsg-log-lib.sh \
                   /lib/zfsbootmenu-core.sh; do";
    assert!(LEGACY_FUNCTION.contains(sourced), "{LEGACY_FUNCTION}");
}

#[test]
fn a_quiet_run_has_no_streams_and_ignores_failures() {
    let runner = FakeRunner::new();
    runner.respond("brltty -q", 1, "");
    run_quietly(&runner, "brltty", &["-q"]);
    let calls = runner.calls();
    let [call] = calls.as_slice() else {
        panic!("one call: {calls:?}");
    };
    assert_eq!(call.display(), "brltty -q");
    assert!(matches!(call.get_stdin(), Input::Null));
    assert_eq!(
        (call.get_stdout(), call.get_stderr()),
        (Stream::Null, Stream::Null)
    );
}

#[test]
fn brush_supports_the_options_of_a_script() {
    for argument in ["-e", "-eu", "-x", "-efnuvx"] {
        assert!(brush_supports(argument), "{argument}");
    }
    for argument in ["", "-", "-o pipefail", "-i", "e", "--posix", "-e -u"] {
        assert!(!brush_supports(argument), "{argument}");
    }
}

/// A hook whose `#!` option brush lacks runs as its line says, and the
/// log says why.
#[test]
fn an_option_brush_lacks_is_logged() {
    let files = zbm_testkit::fakes::FakeFiles::new();
    files.dir("/dev");
    let log = zbm_platform::KernelLog::new(&files, "7", Path::new("/zfsbootmenu"), 1).install();
    let brush = Some("/usr/bin/brush");
    assert_eq!(line(Some(b"#!/bin/bash -o pipefail\n"), brush), "/hooks/h");
    drop(log);
    let kmsg = zbm_platform::Files::read_to_string(&files, Path::new("/dev/kmsg")).unwrap();
    assert!(
        kmsg.contains("/hooks/h: brush lacks -o pipefail, so its #! line runs it"),
        "{kmsg}"
    );
}

#[test]
fn a_hook_runs_on_the_console_with_its_environment() {
    let runner = FakeRunner::new();
    runner.on(|command| {
        (command.program() == "/hooks/missing")
            .then(|| Err(io::Error::from(io::ErrorKind::NotFound)))
    });

    run_hook(
        &runner,
        Path::new("/hooks/plain"),
        Some(b"echo hi\n"),
        &[("ZBM_SELECTED_BE", "tank")],
    );
    run_hook(&runner, Path::new("/hooks/missing"), None, &[]);

    assert_eq!(runner.lines(), ["/bin/bash /hooks/plain", "/hooks/missing"]);
    let calls = runner.calls();
    assert_eq!(
        calls[0].get_env(),
        [("ZBM_SELECTED_BE".into(), Some("tank".into()))]
    );
    assert_eq!(calls[0].get_stdout(), Stream::Inherit);
    // Without a chosen shell, as in these tests.
    assert_eq!(shell::chosen(), None);
}

#[test]
fn key_scripts_shells_and_applets() {
    let runner = FakeRunner::new();
    runner.on(|command| {
        (command.program() == "/etc/keyscript").then(|| {
            Ok(Output::with_secret(
                Status::exited(0),
                Secret::copy_from(b"key"),
            ))
        })
    });

    let output = key_script(
        &runner,
        "/etc/keyscript",
        "none",
        &[("CRYPTTAB_NAME", "root")],
    )
    .unwrap();
    interactive_shell(&runner, ShellRoot::Image).unwrap();
    let chroot = Chroot {
        root: "/zfsbootmenu/environments/tank/ROOT/be@old/mnt".into(),
        shell: "/bin/sh".into(),
        args: Vec::new(),
        name: "tank/ROOT/be@old".parse().unwrap(),
    };
    interactive_shell(&runner, ShellRoot::Chroot(&chroot)).unwrap();
    applet(&runner, "zluks", &["unlock"], &[("LUKS_RETRY", "1")]).unwrap();

    copy_file(&runner, Path::new("/a"), Path::new("/b")).unwrap();
    copy_preserving(&runner, Path::new("/s/-a"), Path::new("/b")).unwrap();
    edit(&runner, "vi", Path::new("/tmp/kcl")).unwrap();
    merged(
        &runner,
        &[("A".to_owned(), "1".to_owned())],
        &["dracut".to_owned(), "-f".to_owned()],
    )
    .unwrap();
    assert!(merged(&runner, &[], &[]).is_err());
    assert_eq!(output.secret.unwrap().expose(), b"key");
    assert_eq!(
        runner.lines(),
        [
            "/etc/keyscript none",
            "/bin/bash -i",
            "/bin/sh",
            "zluks unlock",
            "cp /a /b",
            "cp -a -- /s/-a /b",
            "vi /tmp/kcl",
            "dracut -f"
        ]
    );
    let calls = runner.calls();
    assert_eq!(calls[0].get_stdout(), Stream::CaptureSecret);
    assert_eq!(calls[1].get_default_signals(), [Signal::Interrupt]);
    assert_eq!(calls[2].get_default_signals(), [Signal::Interrupt]);
    assert_eq!(calls[2].get_stdin(), &Input::Inherit);
    assert_eq!(
        calls[3].get_env(),
        [("LUKS_RETRY".into(), Some("1".into()))]
    );
}

/// The program of [`LEGACY_FUNCTION`] on this machine, with `zbm` at
/// `zbm`: the image's `/lib/profiling-lib.sh`, which this machine lacks,
/// is not sourced.
fn legacy_program(zbm: &Path) -> String {
    let profiling = "/lib/profiling-lib.sh ";
    assert!(LEGACY_FUNCTION.contains(profiling));
    LEGACY_FUNCTION
        .replace(profiling, "")
        .replace(ZBM, &zbm.to_string_lossy())
}

/// Runs `function` with `arg` as [`legacy_function`] runs it, with a stub
/// library below a directory, where a legacy caller takes the mount point
/// of a filesystem from the legacy `be_location`, as `mount_zfs` does, and
/// the stub `zbm` of `zbm_script`; what it printed, and its status.
fn legacy_run(zbm_script: &str, function: &str, arg: &str) -> (String, Option<i32>) {
    let dir = TempDir::new("zbm-legacy-function");
    dir.write("lib/kmsg-log-lib.sh", "zdebug() { :; }\nzerror() { :; }\n");
    dir.write(
        "lib/zfsbootmenu-core.sh",
        r#"be_location() {
  [ -n "${1}" ] || return 1
  echo "${BASE}/environments/${1}"
}
mount_point() { echo "$( be_location "${1}" )/mnt"; }
"#,
    );
    let zbm = dir.write_executable("zbm", zbm_script);
    let command = Command::new("bash")
        .args(["-c", &legacy_program(&zbm), "/libexec/zfunc"])
        .arg(dir.path())
        .args([function, arg])
        .stdin(Input::Null)
        .stderr(Stream::Capture)
        .env("BASE", "/zfsbootmenu");
    let output = zbm_platform::exec::SystemRunner.run(&command).unwrap();
    (output.stdout_text(), output.status.code())
}

/// SB19: the legacy functions that hooks and the person run keep a
/// filesystem's state where the Rust runtime keeps it.
#[test]
fn the_legacy_functions_find_the_state_through_zbm_s_be_location() {
    let zbm = "#!/bin/sh\n[ \"$*\" = 'zfunc be_location tank/a/mnt' ] || exit 2\n\
               echo /zfsbootmenu/environments/tank/a/+mnt\n";
    assert_eq!(
        legacy_run(zbm, "mount_point", "tank/a/mnt"),
        (
            "/zfsbootmenu/environments/tank/a/+mnt/mnt\n".to_owned(),
            Some(0)
        )
    );
}

/// Where `zbm` prints nothing, or fails, the legacy `be_location` answers,
/// so that the legacy `mount_zfs` never mounts on the image's `/mnt`.
#[test]
fn the_legacy_be_location_answers_where_zbm_prints_nothing() {
    for zbm in ["#!/bin/sh\nexit 0\n", "#!/bin/sh\necho /x\nexit 1\n"] {
        assert_eq!(
            legacy_run(zbm, "mount_point", "tank/ROOT/be"),
            (
                "/zfsbootmenu/environments/tank/ROOT/be/mnt\n".to_owned(),
                Some(0)
            ),
            "{zbm}"
        );
    }
    // Without a name, `zbm` is not asked: the legacy function fails.
    let asked = "#!/bin/sh\necho asked\n";
    assert_eq!(
        legacy_run(asked, "be_location", ""),
        (String::new(), Some(1))
    );
}
