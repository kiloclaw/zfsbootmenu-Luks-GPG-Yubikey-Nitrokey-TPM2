use std::io;
use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use zbm_platform::{Input, Output, Signal, Status, Stream};
use zbm_testkit::FakeRunner;
use zbm_testkit::fakes::FakeFiles;

use super::*;
use crate::programs::{ShellRoot, interactive_shell};

/// Where the tests' filesystem is mounted.
const ROOT: &str = "/zfsbootmenu/environments/tank/ROOT/be/mnt";

fn name() -> FilesystemName {
    "tank/ROOT/be".parse().unwrap()
}

/// The chroot `find` gives on `files`, with `runner`.
fn found(runner: &FakeRunner, files: &FakeFiles) -> io::Result<Option<Chroot>> {
    Chroot::find(runner, files, Path::new(ROOT), &name())
}

/// The shells `runner` tried, each as its command line.
fn tried(runner: &FakeRunner) -> Vec<String> {
    runner.lines()
}

#[test]
fn the_first_shell_that_runs_in_the_chroot_is_taken() {
    let runner = FakeRunner::new();
    let files = FakeFiles::new();
    // Bash is there but not executable, and sh does not run there.
    files.file_with_mode(format!("{ROOT}/bin/bash"), "", 0o644);
    files.file_with_mode(format!("{ROOT}/bin/sh"), "", 0o755);
    files.file_with_mode(format!("{ROOT}/bin/busybox"), "", 0o755);
    runner.respond("/bin/sh -c exit 0", 127, "");

    let chroot = found(&runner, &files).unwrap().unwrap();

    assert_eq!(
        tried(&runner),
        ["/bin/sh -c exit 0", "/bin/busybox sh -c exit 0"]
    );
    assert_eq!(
        chroot,
        Chroot {
            root: ROOT.into(),
            shell: "/bin/busybox".into(),
            args: vec!["sh".to_owned()],
            name: name(),
        }
    );
    for call in runner.calls() {
        assert_eq!(call.get_root(), Some(Path::new(ROOT)));
        assert_eq!(call.get_stdin(), &Input::Null);
        assert_eq!(
            (call.get_stdout(), call.get_stderr()),
            (Stream::Null, Stream::Null)
        );
    }

    // Bash that runs is taken first, and runs without its rc file.
    files.file_with_mode(format!("{ROOT}/bin/bash"), "", 0o755);
    let chroot = found(&runner, &files).unwrap().unwrap();
    assert_eq!(
        (chroot.shell, chroot.args),
        ("/bin/bash".into(), vec!["--norc".to_owned()])
    );
}

#[test]
fn no_shell_that_runs_is_none_and_a_root_that_cannot_be_entered_is_an_error() {
    let runner = FakeRunner::new();
    let files = FakeFiles::new();
    assert_eq!(found(&runner, &files).unwrap(), None);
    assert_eq!(tried(&runner), Vec::<String>::new());

    files.file_with_mode(format!("{ROOT}/bin/bash"), "", 0o755);
    files.file_with_mode(format!("{ROOT}/bin/sh"), "", 0o755);
    runner.on(|_| {
        Some(Err(io::Error::other(format!(
            "cannot change root directory to {ROOT}: Permission denied"
        ))))
    });
    let error = found(&runner, &files).unwrap_err();
    assert!(error.to_string().contains("cannot change root"), "{error}");
    // No other shell is tried in a root that cannot be entered.
    assert_eq!(tried(&runner), ["/bin/bash -c exit 0"]);
}

/// The shell `find` takes on `files`, and the shells it tried, when each
/// runs.
fn taken(files: &FakeFiles) -> (Option<PathBuf>, Vec<String>) {
    let runner = FakeRunner::new();
    let chroot = found(&runner, files).unwrap();
    (chroot.map(|chroot| chroot.shell), tried(&runner))
}

#[test]
fn an_absolute_link_to_the_shell_is_followed_inside_the_root() {
    // The VM's layout: this system has no busybox, the root has.
    let files = FakeFiles::new();
    files
        .symlink(format!("{ROOT}/bin/sh"), "/bin/busybox")
        .file_with_mode(format!("{ROOT}/bin/busybox"), "", 0o755);

    let (shell, tried) = taken(&files);

    // The shell keeps its name, which the chroot resolves.
    assert_eq!(shell, Some("/bin/sh".into()));
    assert_eq!(tried, ["/bin/sh -c exit 0"]);
}

#[test]
fn a_shell_that_only_the_image_has_is_not_tried() {
    let files = FakeFiles::new();
    files
        .symlink(format!("{ROOT}/bin/bash"), "/usr/bin/bash")
        .file_with_mode("/usr/bin/bash", "", 0o755);

    assert_eq!(taken(&files), (None, Vec::new()));
}

#[test]
fn an_absolute_usr_merge_link_finds_the_environment_s_bash() {
    let files = FakeFiles::new();
    files
        .symlink(format!("{ROOT}/bin"), "/usr/bin")
        .file_with_mode(format!("{ROOT}/usr/bin/bash"), "", 0o755);

    let (shell, tried) = taken(&files);

    assert_eq!(shell, Some("/bin/bash".into()));
    assert_eq!(tried, ["/bin/bash -c exit 0"]);
}

#[test]
fn a_shell_link_that_climbs_out_stays_inside_the_root() {
    // From the root's `bin`, eight `..` climb out of the root in this
    // system, where busybox is not executable; in the chroot they stop at
    // its root, whose busybox is.
    let files = FakeFiles::new();
    files
        .symlink(
            format!("{ROOT}/bin/sh"),
            "../../../../../../../../bin/busybox",
        )
        .file_with_mode(format!("{ROOT}/bin/busybox"), "", 0o755)
        .file_with_mode("/bin/busybox", "", 0o644);

    let (shell, tried) = taken(&files);

    assert_eq!(shell, Some("/bin/sh".into()));
    assert_eq!(tried, ["/bin/sh -c exit 0"]);
}

#[test]
fn a_shell_reached_through_dev_or_tmp_is_not_tried() {
    // Both are this system's in the chroot, whatever the root has there.
    let files = FakeFiles::new();
    files
        .symlink(format!("{ROOT}/bin/bash"), "/dev/shm/bash")
        .symlink(format!("{ROOT}/bin/sh"), "../tmp/sh")
        .file_with_mode(format!("{ROOT}/dev/shm/bash"), "", 0o755)
        .file_with_mode(format!("{ROOT}/tmp/sh"), "", 0o755)
        .file_with_mode("/dev/shm/bash", "", 0o755)
        .file_with_mode("/tmp/sh", "", 0o755);

    assert_eq!(taken(&files), (None, Vec::new()));
}

#[test]
fn a_shell_whose_links_loop_is_not_tried() {
    // In this system, the loop's first link leads to a shell.
    let files = FakeFiles::new();
    files
        .symlink(format!("{ROOT}/bin/sh"), "/bin/sh2")
        .symlink(format!("{ROOT}/bin/sh2"), "./sh")
        .file_with_mode("/bin/sh2", "", 0o755)
        .file_with_mode(format!("{ROOT}/bin/busybox"), "", 0o755);

    let (shell, tried) = taken(&files);

    assert_eq!(shell, Some("/bin/busybox".into()));
    assert_eq!(tried, ["/bin/busybox sh -c exit 0"]);
}

#[test]
fn the_chroot_s_shell_has_its_root_its_ps1_and_no_prompt_command() {
    let runner = FakeRunner::new();
    runner.on(|_| Some(Ok(Output::new(Status::exited(3), ""))));
    let chroot = Chroot {
        root: ROOT.into(),
        shell: "/bin/bash".into(),
        args: vec!["--norc".to_owned()],
        name: "tank/ROOT/be@old".parse().unwrap(),
    };

    let ran = interactive_shell(&runner, ShellRoot::Chroot(&chroot)).unwrap();

    assert_eq!(ran.status.shell_code(), 3);
    let calls = runner.calls();
    let [call] = calls.as_slice() else {
        panic!("one call: {calls:?}");
    };
    assert_eq!(call.display(), "/bin/bash --norc");
    assert_eq!(call.get_root(), Some(Path::new(ROOT)));
    assert_eq!(
        call.get_env(),
        [
            (
                "PS1".into(),
                Some("\\[\\033[0;33m\\]tank/ROOT/be@old\\[\\033[0m\\] \\w > ".into())
            ),
            ("PROMPT_COMMAND".into(), None),
        ]
    );
    assert_eq!(call.get_default_signals(), [Signal::Interrupt]);
    assert_eq!(call.get_stdin(), &Input::Inherit);
    assert_eq!(call.get_stdout(), Stream::Inherit);
}
