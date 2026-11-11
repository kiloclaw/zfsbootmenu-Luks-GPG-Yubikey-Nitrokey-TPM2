//! The shell's `zbm help`, `zbm --help` and `help`, through a pipe and
//! in the pager.

use pretty_assertions::assert_eq;

use super::*;

/// The shell's answers to [`help`]: `zbm help` prints `commands`, and
/// `zbm --help` prints `usage`; then the shell's `help`, which is
/// `zbm help`, prints the commands through a pipe and shows them in the
/// pager, which `q` leaves.
fn help_script(commands: &'static str, usage: &'static str) -> Vec<Step> {
    let mut script = vec![
        Step::Hear("{ zbm help; echo HELP-STATUS=$?; } | cat; echo RC=$?"),
        Step::Say(commands),
        Step::Hear("zbm --help; echo RC=$?"),
        Step::Say(usage),
    ];
    script.extend(alias_script(ALIAS_COMMANDS, PAGER));
    script
}

/// The shell's answers to its `help`: through a pipe it prints `piped`,
/// and on the terminal `paged`, then, after `q`, its exit status.
fn alias_script(piped: &'static str, paged: &'static str) -> Vec<Step> {
    vec![
        Step::Hear("{ help; echo ALIAS-STATUS=$?; } | cat; echo RC=$?"),
        Step::Say(piped),
        Step::Hear("help; echo PAGER-RC=$?"),
        Step::Say(paged),
        Step::Keys("q"),
        Step::Say("\x1b[?1049lPAGER-RC=0\r\n"),
    ]
}

/// What the shell's `help` prints of the chroot through a pipe.
const ALIAS_COMMANDS: &str = "zfs-chroot\r\n\r\n  Open a shell in a chroot of a filesystem.\r\n\
    ALIAS-STATUS=0\r\nRC=0\r\n";

/// The pager on the commands, as the shell's `help` draws it.
const PAGER: &str = "\x1b[?1049h\x1b[H\x1b[2J\u{250c} COMMANDS \u{2500}\u{2500} 1-22 of 160 \
    \u{2510}\r\n\u{2502} zluks \u{2502}\r\n";

/// Plays `script` against [`help`]; the checks it recorded.
fn help_checks(name: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    help(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

/// What `zbm help` prints of the chroot, among the other commands.
const HELP_COMMANDS: &str = "zfs-chroot\r\n\r\n  Open a shell in a chroot of a filesystem.\r\n\r\n\
    \x20   Usage: zfs-chroot FILESYSTEM\r\nHELP-STATUS=0\r\nRC=0\r\n";

#[test]
fn the_shell_s_zbm_help_lists_zfs_chroot_and_zbm_help_prints_zbm_s_usage() {
    let found = help_checks(
        "runtime-recovery-help",
        help_script(
            HELP_COMMANDS,
            "Usage: zbm <command> [arguments]\r\n       <command> [arguments]\r\nRC=0\r\n",
        ),
    );

    assert_eq!(
        found,
        [
            "ok   - zbm help exits 0 through a pipe",
            "ok   - zbm help lists zfs-chroot",
            "ok   - zbm --help prints zbm's usage, not the menu's (0)",
            "ok   - the shell's help is zbm help: through a pipe it lists zfs-chroot",
            "ok   - the shell's help opens the pager on the commands",
            "ok   - q leaves the pager, back at the prompt, help ending with 0",
        ]
    );
}

#[test]
fn a_zbm_help_that_opens_the_menu_fails() {
    let found = help_checks(
        "runtime-recovery-help-menu",
        help_script(
            "zfsbootmenu: unknown option help\r\nHELP-STATUS=2\r\nRC=0\r\n",
            "Usage: zfsbootmenu\r\nRC=0\r\n",
        ),
    );

    assert_eq!(
        found,
        [
            "FAIL - zbm help exits 0 through a pipe",
            "FAIL - zbm help lists zfs-chroot",
            "FAIL - zbm --help prints zbm's usage, not the menu's (0)",
            "ok   - the shell's help is zbm help: through a pipe it lists zfs-chroot",
            "ok   - the shell's help opens the pager on the commands",
            "ok   - q leaves the pager, back at the prompt, help ending with 0",
        ]
    );
}

/// Plays `script` against [`pager`]; what it returned, and the checks it
/// recorded.
fn pager_checks(name: &str, script: Vec<Step>) -> (Result<()>, Vec<String>) {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    let done = pager(&test, &mut vm);

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    (done, found)
}

#[test]
fn a_help_that_prints_on_the_terminal_fails() {
    let (done, found) = pager_checks(
        "runtime-recovery-pager-printed",
        vec![
            Step::Hear("help; echo PAGER-RC=$?"),
            Step::Say("zluks\r\n\r\n  Unlock, close and list LUKS volumes.\r\nPAGER-RC=0\r\n"),
        ],
    );

    assert!(done.is_err());
    assert_eq!(
        found,
        ["FAIL - the shell's help opens the pager on the commands"]
    );
}

#[test]
fn a_pager_that_q_does_not_leave_fails() {
    let (done, found) = pager_checks(
        "runtime-recovery-pager-stays",
        vec![
            Step::Hear("help; echo PAGER-RC=$?"),
            Step::Say(PAGER),
            Step::Keys("q"),
        ],
    );

    assert!(done.is_ok());
    assert_eq!(
        found,
        [
            "ok   - the shell's help opens the pager on the commands",
            "FAIL - q leaves the pager, back at the prompt, help ending with 0",
        ]
    );
}

#[test]
fn a_help_that_runs_the_legacy_viewer_fails() {
    let settings = settings("runtime-recovery-alias-legacy");
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let mut script = help_script(
        HELP_COMMANDS,
        "Usage: zbm <command> [arguments]\r\nRC=0\r\n",
    );
    script.truncate(4);
    script.extend(alias_script(
        "bash: /libexec/zfsbootmenu-help: No such file or directory\r\n\
         ALIAS-STATUS=127\r\nRC=0\r\n",
        PAGER,
    ));
    let (mut vm, guest) = scripted(&test, script);

    help(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(
        checks(&test)[3],
        "FAIL - the shell's help is zbm help: through a pipe it lists zfs-chroot"
    );
    cleanup(&settings);
}
