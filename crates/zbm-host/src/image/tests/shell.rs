//! The shell's files in the plans: `/etc/profile`, `/bin/zbm`,
//! `/root/.bashrc` and the help it offers.

use std::path::Path;

use super::*;

/// `/root/.bashrc` of the dracut image of `settings`.
fn bashrc(settings: &str) -> String {
    let (files, runner) = (host_files(), host_runner());
    let plan = plan_of(Builder::Dracut, settings, &files, &runner).unwrap();
    let text = plan.written(Path::new("/root/.bashrc")).unwrap();
    String::from_utf8_lossy(text).into_owned()
}

/// The `TERM` that `/etc/profile` of the plan of `settings` leaves, run by
/// `sh` with `TERM` set to `term`.
fn term_after_profile(settings: &str, term: &str) -> String {
    let (files, runner) = (host_files(), host_runner());
    let plan = plan_of(Builder::Dracut, settings, &files, &runner).unwrap();
    let profile = plan.written(Path::new("/etc/profile")).unwrap();
    let script = format!("{}\nprintf %s \"$TERM\"", String::from_utf8_lossy(profile));
    let output = std::process::Command::new("sh")
        .args(["-c", &script])
        .env("TERM", term)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

/// What `/bin/zbm` of a Rust plan runs for `args`: the arguments it gives
/// `/libexec/zbm`, run by `sh`.
fn bin_zbm_runs(args: &[&str]) -> String {
    let (files, runner) = (host_files(), host_runner());
    let plan = plan_of(
        Builder::Dracut,
        "zfsbootmenu_runtime\trust\n",
        &files,
        &runner,
    )
    .unwrap();
    let script = String::from_utf8_lossy(plan.written(Path::new("/bin/zbm")).unwrap())
        .replace("exec /libexec/zbm", "echo");
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&script)
        .arg("zbm")
        .args(args)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn bin_zbm_gives_zbm_its_own_options_and_the_menu_the_others() {
    for option in ["-h", "--help", "--list", "-V", "--version"] {
        assert_eq!(bin_zbm_runs(&[option]), format!("{option}\n"));
    }
    assert_eq!(bin_zbm_runs(&[]), "zfsbootmenu\n");
    assert_eq!(bin_zbm_runs(&["-x"]), "zfsbootmenu -x\n");
    assert_eq!(bin_zbm_runs(&["help", "keys"]), "help keys\n");
}

/// The directories of the `PATH` that `/etc/profile` of the plan of
/// `builder` with `settings` sets, in order.
fn path_of_profile(builder: Builder, settings: &str) -> Vec<String> {
    let (files, runner) = (host_files(), host_runner());
    let plan = plan_of(builder, settings, &files, &runner).unwrap();
    let profile = String::from_utf8_lossy(plan.written(Path::new("/etc/profile")).unwrap());
    let line = profile
        .lines()
        .find_map(|line| line.strip_prefix("export PATH="))
        .unwrap();
    line.split(':').map(str::to_owned).collect()
}

#[test]
fn the_rust_profile_finds_zbm_s_links_before_the_builder_s_programs() {
    // dracut's shutdown module installs the host's `reboot` and `poweroff`
    // in `/usr/sbin` or `/sbin`, where they are not one directory with
    // `/bin`: `/bin`, where the image links `zbm`'s commands, must come
    // before each of them.
    assert!(zbm_types::LINKED.iter().any(|linked| linked.dir == "/bin"));
    for (builder, settings) in RUST {
        let path = path_of_profile(builder, settings);
        let place = |dir: &str| path.iter().position(|entry| entry == dir);
        let bin = place("/bin").unwrap();
        for other in ["/usr/sbin", "/usr/bin", "/sbin"] {
            assert!(
                place(other).is_some_and(|at| bin < at),
                "{builder:?}: /bin after {other} in {path:?}"
            );
        }
    }
    // The legacy runtime keeps its own.
    assert_eq!(
        path_of_profile(Builder::Dracut, ""),
        ["/zbm/bin", "/usr/sbin", "/usr/bin", "/sbin", "/bin"]
    );
}

#[test]
fn the_rust_profile_keeps_a_dumb_terminal_for_the_line_view() {
    let rust = "zfsbootmenu_runtime\trust\n";
    assert_eq!(term_after_profile(rust, "dumb"), "dumb");
    assert_eq!(term_after_profile(rust, "vt100"), "linux");
    // The legacy runtime's menu needs the Linux console's description.
    assert_eq!(term_after_profile("", "dumb"), "linux");
}

#[test]
fn the_rust_bashrc_runs_logs_prompt_and_the_legacy_one_is_unchanged() {
    let legacy = bashrc("");
    let rust = bashrc("zfsbootmenu_runtime\trust\n");
    let printer = "\nexport PROMPT_COMMAND=\"/libexec/recovery-error-printer\"\n";
    assert!(legacy.contains(printer), "{legacy}");
    assert!(!rust.contains("recovery-error-printer"), "{rust}");
    // The Rust runtime's differs in that line, in `help` and in the
    // completions alone; brush's hand-over follows it.
    let logs = "\nexport PROMPT_COMMAND=\"/libexec/zbm logs --prompt\"\n";
    let viewer = "\nalias help=\"/libexec/zfsbootmenu-help -L recovery-shell\"\n";
    let help = "\nalias help=\"zbm help\"\n";
    let completions =
        "[ -f /lib/zfsbootmenu-completions.sh ] && source /lib/zfsbootmenu-completions.sh\n";
    assert!(legacy.contains(completions), "{legacy}");
    let expected = legacy
        .replace(printer, logs)
        .replace(viewer, help)
        .replace(completions, &super::super::shell::completions_rust());
    assert!(rust.starts_with(&expected), "{rust}");
}

#[test]
fn every_completed_name_is_in_the_rust_bashrc() {
    let rust = bashrc("zfsbootmenu_runtime\trust\n");
    let completed = zbm_types::LINKED
        .iter()
        .filter_map(|linked| Some((linked.name, linked.complete?.0)))
        .chain(zbm_types::Completion::FUNCTIONS.iter().copied());
    let mut count = 0;
    for (name, kind) in completed {
        let line = format!("\ncomplete -F _zbm_complete {name}\n");
        assert!(rust.contains(&line), "{name}: {rust}");
        let arm = rust
            .lines()
            .find(|arm| arm.contains(&format!("\"{name} ")))
            .unwrap_or_default();
        assert!(
            arm.ends_with(&format!(") kind={} ;;", kind.name())),
            "{name}: {arm}"
        );
        count += 1;
    }
    assert_eq!(rust.matches("complete -F ").count(), count, "{rust}");
    // The legacy runtime keeps its file.
    let legacy = bashrc("");
    assert!(!legacy.contains("_zbm_complete"), "{legacy}");
}

/// What the Rust `.bashrc`'s completion offers for the words `words`, the
/// last being completed, run by `shell`: `zbm complete` stands in for
/// `/libexec/zbm` and answers with its kind, its prefix and a name with a
/// space, behind a backslash as `zbm complete` prints it.
fn completed_by(shell: &str, words: &[&str]) -> String {
    let rust = bashrc("zfsbootmenu_runtime\trust\n");
    let start = rust.find("\n# Tab completes").unwrap();
    let end = rust.find("\nexport PROMPT_COMMAND").unwrap();
    let function = rust[start..end].replace("/libexec/zbm complete", "zbm_complete");
    let quoted: Vec<String> = words.iter().map(|word| format!("'{word}'")).collect();
    let (last, before) = (words.len() - 1, words.len().saturating_sub(2));
    let script = format!(
        "zbm_complete() {{ printf '%s\\n' \"$1:$2\" 'a\\ name'; }}\n{function}\n\
         COMP_WORDS=({words})\nCOMP_CWORD={last}\n\
         _zbm_complete {command} {current} {previous}\n\
         printf '[%s]' \"${{COMPREPLY[@]}}\"\n",
        words = quoted.join(" "),
        command = quoted[0],
        current = quoted[last],
        previous = quoted[before],
    );
    let output = std::process::Command::new(shell)
        .args(["--norc", "-c", &script])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// The workspace's `brush`, the image's shell, which the tests' build
/// puts beside them: `target/debug/brush` for `target/debug/deps/TEST`.
fn workspace_brush() -> std::path::PathBuf {
    let test = std::env::current_exe().unwrap();
    let deps = test.parent().unwrap();
    deps.parent().unwrap_or(deps).join("brush")
}

#[test]
fn the_rust_bashrc_completes_the_argument_a_command_takes_in_bash() {
    if zbm_testkit::can_run(zbm_testkit::have_program("bash"), "bash") {
        completes_the_argument_a_command_takes("bash");
    }
}

#[test]
fn the_rust_bashrc_completes_the_argument_a_command_takes_in_brush() {
    let brush = workspace_brush();
    let what = "the workspace's brush (cargo build -p zbm-brush)";
    if zbm_testkit::can_run(brush.is_file(), what) {
        completes_the_argument_a_command_takes(brush.to_str().unwrap());
    }
}

/// The checks of the tests that the Rust `.bashrc` completes the argument
/// a command takes, in `shell`.
fn completes_the_argument_a_command_takes(shell: &str) {
    assert_eq!(
        completed_by(shell, &["zfs-chroot", "tank/R"]),
        "[filesystems-and-snapshots:tank/R][a\\ name]"
    );
    assert_eq!(
        completed_by(shell, &["/bin/zsnapshots", ""]),
        "[boot-environments:][a\\ name]"
    );
    assert_eq!(
        completed_by(shell, &["zfsbootmenu", "--snapshots", "t"]),
        "[boot-environments:t][a\\ name]"
    );
    assert_eq!(
        completed_by(shell, &["zkexec", "tank/h"]),
        "[filesystems:tank/h][a\\ name]"
    );
    assert_eq!(
        completed_by(shell, &["set_rw_pool", "r"]),
        "[read-only-pools:r][a\\ name]"
    );
    // Not the second argument, nor the first of the menu, which takes an
    // option there.
    assert_eq!(completed_by(shell, &["zfs-chroot", "tank", "x"]), "[]");
    assert_eq!(completed_by(shell, &["zfsbootmenu", "--"]), "[]");
    assert_eq!(completed_by(shell, &["zkexec", "tank/ROOT/be", "v"]), "[]");
}

#[test]
fn the_rust_help_alias_runs_zbm_help() {
    let bashrc = bashrc("zfsbootmenu_runtime\trust\n");
    let alias = bashrc
        .lines()
        .find(|line| line.starts_with("alias help="))
        .unwrap();
    // Bash runs the alias, not its builtin `help` nor `/bin/help`: `zbm`
    // stands in for the image's `/bin/zbm`, which gets the topic.
    let script = format!(
        "shopt -s expand_aliases\n{alias}\nzbm() {{ printf '%s\\n' \"$*\"; }}\nhelp keys\n"
    );
    let output = std::process::Command::new("bash")
        .args(["--norc", "-c", &script])
        .output()
        .unwrap();
    let ran = String::from_utf8(output.stdout).unwrap();
    assert_eq!(ran, "help keys\n");
    // `/bin/zbm` gives `help` and its topic to `zbm`'s applet.
    let words: Vec<&str> = ran.split_whitespace().collect();
    assert_eq!(bin_zbm_runs(&words), "help keys\n");
}

#[test]
fn no_rust_plan_installs_a_help_page() {
    let files = host_files();
    let runner = host_runner();
    for (builder, settings) in RUST {
        let plan = plan_of(builder, settings, &files, &runner)
            .unwrap()
            .golden();
        for page in ["/usr/share/docs/", ".ansi", "zfsbootmenu-help"] {
            assert!(!plan.contains(page), "{builder:?}: {page}");
        }
    }
    // The legacy runtime's images keep its help pages and their viewer.
    let legacy = plan_of(Builder::Dracut, "", &files, &runner)
        .unwrap()
        .golden();
    assert!(legacy.contains("/usr/share/docs/help-files/92/recovery-shell.ansi"));
    assert!(legacy.contains("\t/libexec/zfsbootmenu-help\t"));
}
