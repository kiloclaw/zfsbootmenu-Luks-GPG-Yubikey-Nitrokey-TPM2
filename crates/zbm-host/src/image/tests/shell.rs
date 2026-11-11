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
    // The Rust runtime's differs in that line and in `help` alone;
    // brush's hand-over follows it.
    let logs = "\nexport PROMPT_COMMAND=\"/libexec/zbm logs --prompt\"\n";
    let viewer = "\nalias help=\"/libexec/zfsbootmenu-help -L recovery-shell\"\n";
    let help = "\nalias help=\"zbm help\"\n";
    let expected = legacy.replace(printer, logs).replace(viewer, help);
    assert!(rust.starts_with(&expected), "{rust}");
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
    let rust = [
        (Builder::Dracut, RUST_ALL),
        (Builder::Dracut, RUST_RELEASE),
        (Builder::Mkinitcpio, RUST_ALL),
    ];
    for (builder, settings) in rust {
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
