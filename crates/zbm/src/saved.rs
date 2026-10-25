//! The logs saved on the EFI system partition (`zbm.save_logs`, ROADMAP.md
//! #9), from the recovery shell and the supervisor:
//!
//! - `zbm debug saved` lists the saved boots and their files;
//! - `zbm debug save` saves this boot's logs now;
//! - after the start-up program crashes, the supervisor has `zbm debug save
//!   --after-crash` save them, with the crash report, when `zbm.save_logs`
//!   asks for it: in a process of its own, since probing devices and
//!   mounting the partition can hang, which process 1 must not.
//!
//! The commands use the partition `zbm.save_logs` names, or the ESP when it
//! is not given; the supervisor saves only when it is.

use std::path::Path;

use zbm_boot::saved_logs::{self, Destination};
use zbm_config::args::{Args, ZbmArgs};
use zbm_config::spec::cmdline::SAVE_LOGS;
use zbm_platform::{Files, NoConsole, RealSystem, System};

use crate::{Io, USAGE};

/// Where the command line that start-up saved in `base` (`zbm.cmdline`) has
/// `zbm.save_logs` save logs; `None` when it does not ask for it.
pub fn destination(files: &dyn Files, base: &Path) -> Option<Destination> {
    let kcl = files
        .read_to_string(&base.join("zbm.cmdline"))
        .unwrap_or_default();
    let value = ZbmArgs::from_kcl(&kcl).lookup(&SAVE_LOGS, &mut Vec::new());
    Destination::from_option(value.as_deref())
}

/// `zbm debug saved` or `zbm debug save` on `system`, with the state
/// directory `base`.
pub fn run_in(system: &System<'_>, base: &Path, args: &[&str], io: &mut Io) -> i32 {
    let destination = destination(system.files, base).unwrap_or(Destination::Esp);
    match args {
        ["saved"] => match saved_logs::list(system, &destination) {
            Ok((device, boots)) => {
                let _ = writeln!(io.stdout, "saved logs on {device}:");
                for boot in boots {
                    let files: Vec<String> = boot
                        .files
                        .iter()
                        .map(|(name, size)| format!("{name} ({size} bytes)"))
                        .collect();
                    let _ = writeln!(io.stdout, "{}: {}", boot.name, files.join(", "));
                }
                0
            }
            Err(error) => {
                let _ = writeln!(io.stderr, "zbm debug: {error}");
                1
            }
        },
        // The supervisor, after a crash, with the kernel log as standard
        // error: only when the command line asks for it.
        ["save", "--after-crash"] => {
            if let Some(message) = after_crash_in(system, base) {
                let _ = writeln!(io.stderr, "<3>ZFSBootMenu: {message}");
            }
            0
        }
        ["save"] => match saved_logs::save(system, base, &destination) {
            Ok(saved) => {
                let _ = writeln!(io.stdout, "saved {} on {}", saved.directory, saved.device);
                0
            }
            Err(error) => {
                let _ = writeln!(io.stderr, "zbm debug: {error}");
                1
            }
        },
        _ => USAGE,
    }
}

/// `zbm debug saved` or `zbm debug save` on the machine.
pub fn run(base: &Path, args: &[&str], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    run_in(&real.with_console(&NoConsole), base, args, io)
}

/// After a crash: saves the logs, the report among them, when the saved
/// command line in `base` asks for it; returns what to tell the user.
pub fn after_crash_in(system: &System<'_>, base: &Path) -> Option<String> {
    let destination = destination(system.files, base)?;
    Some(match saved_logs::save(system, base, &destination) {
        Ok(saved) => format!("logs saved in {} on {}", saved.directory, saved.device),
        Err(error) => format!("unable to save the logs: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeSystem;

    use super::*;

    /// A machine with one ESP, a journal, and `cmdline` saved by start-up.
    fn machine(cmdline: &str) -> FakeSystem {
        let fake = FakeSystem::new();
        fake.runner.respond(
            "lsblk -rnpo NAME,PARTTYPE",
            0,
            "/dev/vda1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n",
        );
        fake.files
            .file("/zfsbootmenu/zbm.cmdline", cmdline)
            .file("/zfsbootmenu/journal.jsonl", "{}\n");
        fake
    }

    fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        let code = run_in(&fake.system(), Path::new("/zfsbootmenu"), args, &mut io);
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
        (code, text(stdout), text(stderr))
    }

    #[test]
    fn the_saved_command_line_names_where_logs_go() {
        let files = zbm_testkit::fakes::FakeFiles::new();
        let base = Path::new("/zfsbootmenu");
        assert_eq!(destination(&files, base), None);
        files.file(
            "/zfsbootmenu/zbm.cmdline",
            "quiet\nzbm.save_logs=LABEL=EFI\n",
        );
        assert_eq!(
            destination(&files, base),
            Some(Destination::Device("LABEL=EFI".to_owned()))
        );
        files.file("/zfsbootmenu/zbm.cmdline", "zbm.save_logs=off\n");
        assert_eq!(destination(&files, base), None);
    }

    #[test]
    fn logs_are_saved_and_listed_from_the_recovery_shell() {
        let fake = machine("quiet\n");

        let (code, stdout, stderr) = call(&fake, &["save"]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(
            stdout,
            "saved EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z on /dev/vda1\n"
        );
        let (code, stdout, _) = call(&fake, &["saved"]);
        assert_eq!(code, 0);
        assert_eq!(
            stdout,
            "saved logs on /dev/vda1:\n\
             2026-01-01T00-00-00Z: journal.jsonl (3 bytes), kernel.log (0 bytes)\n"
        );
        assert_eq!(call(&fake, &["saved", "now"]).0, USAGE);
    }

    #[test]
    fn without_an_esp_the_commands_say_so() {
        let fake = FakeSystem::new();

        assert_eq!(
            call(&fake, &["saved"]),
            (
                1,
                String::new(),
                "zbm debug: no EFI system partition found\n".to_owned()
            )
        );
        assert_eq!(
            call(&fake, &["save"]).2,
            "zbm debug: no EFI system partition found\n"
        );
    }

    #[test]
    fn a_crash_saves_the_logs_only_when_asked() {
        let quiet = machine("quiet\n");
        assert_eq!(
            after_crash_in(&quiet.system(), Path::new("/zfsbootmenu")),
            None
        );

        let asked = machine("zbm.save_logs\n");
        let told = after_crash_in(&asked.system(), Path::new("/zfsbootmenu"));
        assert_eq!(
            told.as_deref(),
            Some("logs saved in EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z on /dev/vda1")
        );
        let failing = machine("zbm.save_logs=LABEL=none\n");
        let told = after_crash_in(&failing.system(), Path::new("/zfsbootmenu"));
        assert_eq!(
            told.as_deref(),
            Some("unable to save the logs: LABEL=none: no such partition")
        );
    }

    #[test]
    fn after_a_crash_the_command_tells_the_kernel_log_only_when_asked() {
        let quiet = machine("quiet\n");
        assert_eq!(
            call(&quiet, &["save", "--after-crash"]),
            (0, String::new(), String::new())
        );

        let asked = machine("zbm.save_logs\n");
        let (code, _, told) = call(&asked, &["save", "--after-crash"]);
        assert_eq!(code, 0);
        assert!(
            told.starts_with("<3>ZFSBootMenu: logs saved in EFI/zfsbootmenu/logs/"),
            "{told}"
        );
    }
}
