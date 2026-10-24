use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeFiles;

use super::*;

/// A journal of two programs: an import, a LUKS unlock with a redacted
/// field, and the commands they ran.
const JOURNAL_TEXT: &str = concat!(
    r#"{"pid":7,"seq":0,"level":"INFO","target":"zbm","#,
    r#""message":"running init stage 10-kmods"}"#,
    "\n",
    r#"{"pid":7,"seq":1,"level":"INFO","target":"zbm_platform::traced","#,
    r#""message":"command ended","fields":{"status":0,"duration_ms":12},"#,
    r#""spans":[{"span":"command","program":"zpool","#,
    r#""line":"zpool import -N tank","stdin":"null"}]}"#,
    "\n",
    "garbage\n",
    r#"{"pid":9,"seq":0,"level":"WARN","target":"zbm_luks","#,
    r#""message":"wrong passphrase","fields":{"passphrase":"[redacted]"},"#,
    r#""spans":[{"span":"unlock","device":"/dev/sda2"},{"span":"key_method"}]}"#,
    "\n",
    r#"{"pid":9,"seq":1,"level":"WARN","target":"zbm_platform::traced","#,
    r#""message":"command could not start","#,
    r#""fields":{"error":"not found","duration_ms":0},"#,
    r#""spans":[{"span":"command","program":"tpm2_unseal","#,
    r#""line":"tpm2_unseal -c /x","stdin":"secret"}]}"#,
    "\n",
    r#"{"pid":9,"seq":2,"level":"ERROR","target":"zbm_luks","#,
    r#""message":"no key method opened /dev/sda2"}"#,
    "\n",
);

/// The state directory with the journal.
fn files() -> FakeFiles {
    let files = FakeFiles::new();
    files.file("/zfsbootmenu/journal.jsonl", JOURNAL_TEXT);
    files
}

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

#[test]
fn each_record_takes_one_line_with_its_fields_and_spans() {
    let records = records(&files(), Path::new("/zfsbootmenu"));

    assert_eq!(records.len(), 5);
    assert_eq!(
        line(&records[2]),
        "     9 WARN  zbm_luks: wrong passphrase passphrase=[redacted] \
         [unlock{device=/dev/sda2} > key_method]"
    );
    assert_eq!(
        line(&records[0]),
        "     7 INFO  zbm: running init stage 10-kmods"
    );
}

#[test]
fn the_journal_is_shown_from_a_level_up() {
    let files = files();
    let base = Path::new("/zfsbootmenu");

    let (code, all, _) = output(|io| journal(&files, base, &[], io).unwrap());
    let (_, warnings, _) = output(|io| journal(&files, base, &["--level", "warn"], io).unwrap());

    assert_eq!(code, 0);
    assert_eq!(all.lines().count(), 5);
    assert_eq!(warnings.lines().count(), 3);
    assert!(
        warnings.lines().all(|line| !line.contains(" INFO ")),
        "{warnings}"
    );
    assert!(
        journal(
            &files,
            base,
            &["--level", "loud"],
            &mut Io {
                stdout: &mut Vec::new(),
                stderr: &mut Vec::new(),
            }
        )
        .is_none()
    );
    let empty = FakeFiles::new();
    let (code, _, stderr) = output(|io| journal(&empty, base, &[], io).unwrap());
    assert_eq!(
        (code, stderr.as_str()),
        (1, "zbm debug: no journal in /zfsbootmenu\n")
    );
}

#[test]
fn the_commands_say_how_they_ended() {
    let ran = commands(&records(&files(), Path::new("/zfsbootmenu")));

    assert_eq!(
        ran,
        [
            Ran {
                line: "zpool import -N tank".into(),
                status: Some(0),
                duration_ms: 12,
            },
            Ran {
                line: "tpm2_unseal -c /x".into(),
                status: None,
                duration_ms: 0,
            },
        ]
    );
    let (code, text, _) = output(|io| print_commands(&files(), Path::new("/zfsbootmenu"), io));
    assert_eq!(code, 0);
    assert_eq!(
        text,
        "   0      12 ms  zpool import -N tank\n   \
         -       0 ms  tpm2_unseal -c /x\n\
         2 commands, 1 failed or did not start, 12 ms in all\n"
    );
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "a command of any adapter would do; this one is the start-up's import"
)]
fn the_commands_of_a_traced_runner_are_read_back_from_the_journal() {
    let fake = zbm_testkit::fakes::FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner.respond_program("zpool", 0, "");
    let base = Path::new("/zfsbootmenu");
    {
        let _log = zbm_platform::KernelLog::new(&fake.files, "4", base, 1).install();
        let system = fake.system();
        let traced = zbm_platform::TracedRunner::new(system.runner, system.clock);
        let system = system.with_runner(&traced);
        let command = zbm_platform::Command::new("zpool").args(["import", "-N", "tank"]);
        let _ = system.runner.run(&command);
    }

    let ran = commands(&records(&fake.files, base));

    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(ran[0].line, "zpool import -N tank");
    assert_eq!(ran[0].status, Some(0));
}
