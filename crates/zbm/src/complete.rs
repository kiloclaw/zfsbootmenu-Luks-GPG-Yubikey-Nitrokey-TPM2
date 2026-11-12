//! `zbm complete KIND [PREFIX]`: the names of KIND that start with PREFIX,
//! one per line, each as one shell word, which the recovery shell's
//! `.bashrc` of an image of the Rust runtime completes with on Tab
//! ([`zbm_app::complete`], ADR 0018, point 23). Nobody types it: `zbm
//! --list` and `zbm help` leave it out.

use zbm_platform::{NoConsole, RealSystem, System};
use zbm_types::Completion;

use crate::table::COMPLETE;
use crate::{Io, USAGE};

/// `zbm complete` on this machine.
pub(crate) fn run(args: &[String], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    run_in(&real.with_console(&NoConsole), args, io)
}

/// `zbm complete` with `args` on `system`: 0 with the names, whichever
/// there are, and 2 for an unknown kind or more arguments.
fn run_in(system: &System<'_>, args: &[String], io: &mut Io) -> i32 {
    let (kind, prefix) = match args {
        [kind] => (kind, ""),
        [kind, prefix] => (kind, prefix.as_str()),
        _ => return usage(io),
    };
    let Some(kind) = Completion::named(kind) else {
        return usage(io);
    };
    for name in zbm_app::complete::list(system, kind, prefix) {
        if writeln!(io.stdout, "{name}").is_err() {
            break;
        }
    }
    0
}

/// Prints the usage on standard error: the exit status of a usage error.
fn usage(io: &mut Io) -> i32 {
    let _ = io.stderr.write_all(COMPLETE.usage.as_bytes());
    USAGE
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeSystem;

    use super::*;
    use crate::library_usage::FUNCTIONS;

    /// `zbm complete` with `args` on `fake`: its status and output.
    fn complete(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        let code = run_in(&fake.system(), &args, &mut io);
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn complete_prints_the_names_of_a_kind_one_per_line() {
        let fake = FakeSystem::new();
        assert_eq!(
            complete(&fake, &["efivarfs-modes"]),
            (0, "ro\nrw\n".to_owned(), String::new())
        );
        assert_eq!(
            complete(&fake, &["efivarfs-modes", "rw"]),
            (0, "rw\n".to_owned(), String::new())
        );
        fake.runner.respond("zfs version -j", 1, "");
        fake.runner.respond(
            "zfs list -H -p -t filesystem -o name,mountpoint,canmount,keystatus",
            0,
            "tank/ROOT/be\t/\tnoauto\t-\ntank/home\t/home\ton\t-\n",
        );
        assert_eq!(
            complete(&fake, &["boot-environments", "tank/"]),
            (0, "tank/ROOT/be\n".to_owned(), String::new())
        );
    }

    #[test]
    fn an_unknown_kind_or_more_arguments_print_the_usage_and_exit_2() {
        let fake = FakeSystem::new();
        let usage = (USAGE, String::new(), COMPLETE.usage.to_owned());
        assert_eq!(complete(&fake, &["kernels"]), usage);
        assert_eq!(complete(&fake, &[]), usage);
        assert_eq!(complete(&fake, &["efivarfs-modes", "r", "x"]), usage);
    }

    #[test]
    fn complete_is_hidden() {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        assert_eq!(crate::run("zbm", &["--list".to_owned()], &mut io), 0);
        let listed = String::from_utf8(stdout).unwrap();
        assert!(
            !listed.lines().any(|line| line.starts_with("complete ")),
            "{listed}"
        );
        // Neither `zbm help` nor the man page's commands name it.
        let commands = crate::shell_commands();
        assert!(!commands.contains("complete"), "{commands}");
        // It runs all the same.
        let fake = FakeSystem::new();
        assert_eq!(complete(&fake, &["efivarfs-modes", "ro"]).0, 0);
    }

    #[test]
    fn the_completed_functions_are_those_of_the_library_that_take_an_argument() {
        let natives = zbm_startup::zfunc::natives();
        for (name, _) in Completion::FUNCTIONS {
            let offered = FUNCTIONS.iter().any(|function| function.name == *name);
            assert!(offered || natives.contains(name), "{name}");
        }
        for function in FUNCTIONS {
            let takes = function.usage.trim_end() != format!("Usage: {}", function.name);
            let completed = Completion::FUNCTIONS
                .iter()
                .any(|(name, _)| *name == function.name);
            assert_eq!(completed, takes, "{}", function.name);
        }
    }

    #[test]
    fn every_kind_has_a_name_complete_takes() {
        let names: Vec<&str> = Completion::ALL.iter().map(|kind| kind.name()).collect();
        for kind in Completion::ALL {
            assert_eq!(Completion::named(kind.name()), Some(kind));
        }
        let count = |name: &&str| names.iter().filter(|other| *other == name).count();
        assert!(names.iter().all(|name| count(name) == 1), "{names:?}");
        // The usage names them all, and nothing else, after `Kinds:`.
        let (_, kinds) = COMPLETE.usage.split_once("Kinds:").unwrap();
        assert_eq!(kinds.split_whitespace().collect::<Vec<_>>(), names);
    }
}
