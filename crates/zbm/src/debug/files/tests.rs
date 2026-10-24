use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeFiles;

use super::*;

/// What `run` printed, and its exit status.
fn output(run: impl FnOnce(&mut Io) -> i32) -> (i32, String, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = run(&mut Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    });
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    (code, text(stdout), text(stderr))
}

/// The line of `name` in `lines`.
fn line_of<'l>(lines: &'l [String], name: &str) -> &'l str {
    lines
        .iter()
        .find(|line| line.starts_with(&format!("{name} ")))
        .map_or("", String::as_str)
}

#[test]
fn each_option_says_its_value_and_where_it_came_from() {
    let lines =
        config_lines("zbm.timeout=5\ntimeout=7\nzbm.prefer=tank!\nzbm.import_policy=bogus\n");

    let words = |name: &str| -> Vec<String> {
        line_of(&lines, name)
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(words("zbm.timeout")[1..], ["5", "command", "line"]);
    assert_eq!(words("zbm.prefer")[1], "tank!");
    let policy = line_of(&lines, "zbm.import_policy");
    assert!(
        policy.contains("default: zbm.import_policy=bogus is not valid"),
        "{policy}"
    );
    assert!(
        line_of(&lines, "zbm.hookroot").ends_with("default"),
        "{lines:?}"
    );
    let aliased = config_lines("spl_hostid=0x1234\n");
    assert!(
        line_of(&aliased, "spl.spl_hostid").ends_with("command line, as spl_hostid"),
        "{aliased:?}"
    );
}

#[test]
fn config_prints_the_saved_command_line_first() {
    let files = FakeFiles::new();
    let base = Path::new("/zfsbootmenu");

    let (code, _, stderr) = output(|io| config(&files, base, io));
    assert_eq!(
        (code, stderr.as_str()),
        (1, "zbm debug: no saved command line in /zfsbootmenu\n")
    );

    files.file("/zfsbootmenu/zbm.cmdline", "zbm.timeout=5\nquiet\n");
    let (code, stdout, _) = output(|io| config(&files, base, io));
    assert_eq!(code, 0);
    assert!(
        stdout.starts_with("command line: zbm.timeout=5 quiet\n"),
        "{stdout}"
    );
    assert!(stdout.contains("zbm.timeout"), "{stdout}");
}

#[test]
fn state_shows_small_files_and_leaves_out_the_journal_crashes_and_keys() {
    let files = FakeFiles::new();
    let base = Path::new("/zfsbootmenu");
    files
        .file("/zfsbootmenu/bootfs", "tank/ROOT/be\n")
        .file("/zfsbootmenu/luks-mappings", "a1\t/dev/sda2\t0")
        .file("/zfsbootmenu/journal.jsonl", "{}\n")
        .file("/zfsbootmenu/crash-1-2.txt", "report\n")
        .file("/zfsbootmenu/.keys/tank/key", "SECRET")
        .file("/zfsbootmenu/big", "x".repeat(5000))
        .file("/zfsbootmenu/pools/tank", "");

    let (code, stdout, _) = output(|io| state(&files, base, io));

    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "== big (5000 bytes)\n\
         == bootfs (13 bytes)\ntank/ROOT/be\n\
         == luks-mappings (14 bytes)\na1\t/dev/sda2\t0\n\
         == pools/tank (0 bytes)\n"
    );
    assert!(!stdout.contains("SECRET") && !stdout.contains("report"));
    let empty = FakeFiles::new();
    assert_eq!(output(|io| state(&empty, base, io)).0, 1);
}
