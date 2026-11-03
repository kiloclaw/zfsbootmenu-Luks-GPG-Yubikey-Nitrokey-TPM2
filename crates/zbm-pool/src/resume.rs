//! The resume check: finding a suspend partition and asking the user.
//!
//! Importing a pool read-write and then resuming from hibernation, whose
//! image still holds the pool's old state, can destroy the pool. Before a
//! read-write import, ZFSBootMenu looks for a suspend partition and, if it
//! finds one, asks the user through the front end's [`Interaction`]
//! whether to go on and whether to add `noresume` to the boot
//! environment's command line.

use zbm_platform::Context;
use zbm_platform::interaction::{Interaction, Resume, ResumeRequest};
use zbm_state::State;
use zbm_tools::devices::Devices;
use zbm_types::PoolName;

use crate::error::WritableError;

/// The partition types of a hibernation image, from dracut's `95resume`
/// module.
pub const SUSPEND_TYPES: [&str; 3] = ["suspend", "swsuspend", "swsupend"];

/// Whether a block device holds a hibernation image: a partition of one of
/// the [`SUSPEND_TYPES`].
pub fn has_resume_device(context: &Context<'_>) -> bool {
    SUSPEND_TYPES.iter().any(|kind| {
        let found = Devices::new(context.runner(), context.files()).has_type(kind);
        if found {
            zbm_trace::debug!("resume device: {kind}"; %kind);
        }
        found
    })
}

/// Why a read-write import may go ahead as far as hibernation goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum ResumeChoice {
    /// The machine has no suspend partition.
    NoResumeDevice,
    /// The boot environment gets `noresume`, chosen now or before.
    Noresume,
    /// The user chose to go on without `noresume`.
    Dangerous,
}

/// Whether `pool` may be imported read-write as far as hibernation goes: yes
/// without a suspend partition, or when the user already chose `noresume`;
/// otherwise the user decides, through `interaction`. [`Resume::Noresume`]
/// records the choice in the state directory, so the boot environment's
/// command line gets `noresume`; [`Resume::Dangerous`] goes on without it.
///
/// # Errors
///
/// [`WritableError::ResumeRefused`] when the user refused.
pub fn resume_prompt(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    pool: &PoolName,
) -> Result<ResumeChoice, WritableError> {
    zbm_trace::debug!("pool set to {pool}"; %pool);
    if !has_resume_device(context) {
        return Ok(ResumeChoice::NoResumeDevice);
    }
    // A noresume already given is never taken back.
    if state.noresume().is_some() {
        return Ok(ResumeChoice::Noresume);
    }
    let request = ResumeRequest {
        pool: pool.as_str(),
    };
    match interaction.resume(&request) {
        Resume::Dangerous => Ok(ResumeChoice::Dangerous),
        Resume::Noresume => {
            if let Err(error) = state.set_noresume() {
                zbm_trace::error!("{error}"; %error);
            }
            Ok(ResumeChoice::Noresume)
        }
        Resume::Refuse => Err(WritableError::ResumeRefused {
            pool: pool.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use zbm_platform::KernelLog;
    use zbm_platform::exec::{Output, Status};
    use zbm_platform::files::Files;
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines, stub_lines};
    use zbm_testkit::{TempDir, parity};
    use zbm_tools::console::ConsoleInteraction;

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
        assert!(has_resume_device(&Context::new(fake.system())));
        assert_eq!(
            fake.runner.lines(),
            ["blkid -t TYPE=suspend", "blkid -t TYPE=swsuspend"]
        );
        assert!(fake.kmsg().contains("resume device: swsuspend"));
        assert!(!has_resume_device(&Context::new(machine(None).system())));
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
                        legacy.status.is_success(),
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
            let tank = "tank".parse::<PoolName>().unwrap();
            let interaction = ConsoleInteraction::new(fake.system());
            let rust = resume_prompt(&Context::new(fake.system()), &interaction, &state, &tank);
            assert_eq!(rust.is_ok(), success, "{case}");
            assert_eq!(
                fake.files.exists(Path::new("/zfsbootmenu/noresume")),
                legacy_noresume,
                "{case}"
            );
            // `read -p` writes its prompt to standard error.
            let shown = fake.console.output_text();
            let shown = shown.strip_suffix("> ").unwrap_or(&shown);
            assert_eq!(shown, stdout, "{case}");
            assert_eq!(stub_lines(&fake.kmsg()), logged, "{case}");
        }
    }
}
