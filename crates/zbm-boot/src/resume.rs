//! The resume check: `has_resume_device` and `resume_prompt`.
//!
//! Importing a pool read-write and then resuming from hibernation, whose
//! image still holds the pool's old state, can destroy the pool. Before a
//! read-write import, ZFSBootMenu looks for a suspend partition and, if it
//! finds one, asks the user whether to go on and whether to add `noresume`
//! to the boot environment's command line.

use zbm_core::exec::{Command, Stream};
use zbm_core::shell::trim_ifs;
use zbm_state::State;
use zbm_system::System;

use crate::prompt::{colorize, tput};

/// The partition types of a hibernation image, from dracut's `95resume`
/// module.
pub const SUSPEND_TYPES: [&str; 3] = ["suspend", "swsuspend", "swsupend"];

/// Whether a block device holds a hibernation image (`has_resume_device`).
pub fn has_resume_device(system: &System<'_>) -> bool {
    SUSPEND_TYPES.iter().any(|kind| {
        let blkid = Command::new("blkid")
            .args(["-t", &format!("TYPE={kind}")])
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        let found = system
            .runner
            .run(&blkid)
            .is_ok_and(|output| output.status.success());
        if found {
            zbm_trace::debug!("resume device: {kind}");
        }
        found
    })
}

/// The warning `resume_prompt` shows for `pool`.
fn warning(pool: &str) -> String {
    format!(
        "WARNING!!!

This system appears to have an active suspend partition.

The action you are requesting requires the ZFS pool

    {pool}

be imported read-write. Importing read-write and then resuming
from an active suspend partition may DESTROY YOUR POOL.

If you choose to proceed, ZFSBootMenu can attempt to remove any
\"resume=\" arguments from your kernel command line and append a
\"noresume\" argument to prevent your system from attempting to
restore from the active suspend partition.

Type {noresume} to proceed with the import, allowing ZFSBootMenu
to add a \"noresume\" argument to your kernel command line.

Type {dangerous} to proceed with the import without allowing
ZFSBootMenu to modify your kernel command line. Make sure to
add the \"noresume\" argument yourself if necessary.

Type any other text, or just press enter, to abort.

Proceed {no} ?
",
        pool = colorize("red", pool),
        noresume = colorize("green", "NORESUME"),
        dangerous = colorize("red", "DANGEROUS"),
        no = colorize("red", "[No]"),
    )
}

/// Whether `pool` may be imported read-write as far as hibernation goes, as
/// `resume_prompt` decides: yes without a suspend partition, or when the user
/// already chose `noresume`; otherwise the user decides. `NORESUME` records
/// the choice in the state directory, so the boot environment's command line
/// gets `noresume`; `DANGEROUS` goes on without it.
pub fn resume_prompt(system: &System<'_>, state: &State<'_>, pool: &str) -> bool {
    if pool.is_empty() {
        zbm_trace::error!("pool is undefined");
        return false;
    }
    zbm_trace::debug!("pool set to {pool}");
    if !has_resume_device(system) {
        return true;
    }
    // A noresume already given is never taken back.
    if state.noresume().is_some() {
        return true;
    }

    tput(system, &["clear"]);
    tput(system, &["cnorm"]);
    tput(system, &["cup", "0", "0"]);
    let _ = system.console.write(warning(pool).as_bytes());

    // zfsbootmenu-input: `read -r -e -p "> "`, then `echo`
    let _ = system.console.write(b"> ");
    let line = system
        .console
        .read_line()
        .ok()
        .flatten()
        .unwrap_or_default();
    match trim_ifs(&line) {
        "DANGEROUS" => true,
        "NORESUME" => {
            if let Err(error) = state.set_noresume() {
                zbm_trace::error!("{}", error.to_string());
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use zbm_core::exec::{Output, Status};
    use zbm_system::KernelLog;
    use zbm_system::files::Files;
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines};
    use zbm_testkit::{TempDir, parity};

    fn legacy_core() -> Bash {
        Bash::new()
            .code(LOG_STUBS)
            .code(
                r#"source() {
                    case "$1" in
                      /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                      *) builtin source "$@" ;;
                    esac
                  }"#,
            )
            .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
    }

    /// A machine whose `blkid` finds a partition of type `suspend` (or none).
    fn machine(suspend: Option<&'static str>) -> FakeSystem {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu");
        fake.runner.on(move |command| {
            let kind = command.get_args().get(1)?.to_string_lossy().into_owned();
            (command.program() == "blkid").then(|| {
                let found = Some(kind.trim_start_matches("TYPE=")) == suspend;
                Ok(Output::new(Status::exited(if found { 0 } else { 2 }), ""))
            })
        });
        fake
    }

    #[test]
    fn suspend_partitions_are_found_by_type() {
        let fake = machine(Some("swsuspend"));
        let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
        assert!(has_resume_device(&fake.system()));
        assert_eq!(
            fake.runner.lines(),
            ["blkid -t TYPE=suspend", "blkid -t TYPE=swsuspend"]
        );
        assert!(fake.kmsg().contains("resume device: swsuspend"));
        assert!(!has_resume_device(&machine(None).system()));
    }

    #[test]
    fn matches_the_legacy_prompt() {
        let cases: [(Option<&'static str>, &str, bool); 7] = [
            (None, "", false),
            (Some("suspend"), "NORESUME\n", false),
            (Some("swsupend"), "  DANGEROUS \n", false),
            (Some("suspend"), "no\n", false),
            (Some("suspend"), "", false),
            (Some("suspend"), "NORESUME\n", true),
            (Some("suspend"), "noresume\n", false),
        ];
        let mut parity = parity!("matches_the_legacy_prompt");
        for (suspend, input, already) in cases {
            let case = format!("{suspend:?} {input:?} {already}");
            let (success, legacy_noresume, stdout, logged): (bool, bool, String, String) = parity
                .legacy(case.clone(), || {
                    let dir = TempDir::new("zbm-resume");
                    if already {
                        dir.write("noresume", "");
                    }
                    let blkid = format!(
                        "blkid() {{ [ \"$2\" = \"TYPE={}\" ]; }}",
                        suspend.unwrap_or("none")
                    );
                    let core = std::fs::read_to_string(zbm_testkit::legacy::path(
                        "zfsbootmenu/lib/zfsbootmenu-core.sh",
                    ))
                    .unwrap();
                    let start = core.find("\nresume_prompt() {\n").unwrap() + 1;
                    let end = start + core[start..].find("\n}\n").unwrap() + 3;
                    let function =
                        core[start..end].replace("/libexec/zfsbootmenu-input", "zbm_input");
                    let legacy = legacy_core()
                        .code(&blkid)
                        .code("tput() { :; }")
                        .code(r#"zbm_input() { read -r input; printf '> ' >&2; echo "${input}"; }"#)
                        .code(&function)
                        .env("BASE", &dir.path().to_string_lossy())
                        .call("resume_prompt", &["tank"], input.as_bytes());
                    (
                        legacy.status.success(),
                        dir.join("noresume").exists(),
                        legacy.stdout_text(),
                        log_lines(&legacy.stderr_text()),
                    )
                });

            let fake = machine(suspend);
            if already {
                fake.files.file("/zfsbootmenu/noresume", "");
            }
            fake.console.line(input.trim_end_matches('\n'));
            if input.is_empty() {
                // The input ends: nothing to read.
                fake.console.push(zbm_testkit::fakes::ConsoleInput::End);
            }
            let state = State::new(&fake.files, "/zfsbootmenu");
            let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
            let rust = resume_prompt(&fake.system(), &state, "tank");
            assert_eq!(rust, success, "{case}");
            assert_eq!(
                fake.files.exists(Path::new("/zfsbootmenu/noresume")),
                legacy_noresume,
                "{case}"
            );
            // `read -p` writes its prompt to standard error.
            let shown = fake.console.output_text();
            let shown = shown.strip_suffix("> ").unwrap_or(&shown);
            assert_eq!(shown, stdout, "{case}");
            assert_eq!(crate::testing::stub_lines(&fake.kmsg()), logged, "{case}");
        }
    }
}
