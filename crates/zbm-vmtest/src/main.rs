//! `zbm-vmtest`: runs the VM tests of ZFSBootMenu. See
//! `docs/luks/testing.rst`.

use std::path::PathBuf;
use std::process::ExitCode;

use zbm_vmtest::scenarios::{self, ALL};
use zbm_vmtest::{Settings, compare, runner};

/// The text of `zbm-vmtest help`.
const USAGE: &str = "\
Usage: zbm-vmtest <command>

Commands:
  run [<test>...]        Run the tests (all of them by default), each in a
                         process of its own, one virtual machine at a time,
                         and print a summary of their results; setups run
                         before the scenarios that need them
  list                   List the tests in the order run runs them
  scenario <test>        Run one test in this process
  compare <work> <work> [<test>...]
                         Compare the checks of two runs (two ZBMTEST_WORK
                         directories), such as those of two commits, message
                         by message
  help                   Show this text

Tests are named <group>-<scenario>: layouts-main, complex-hotplug, ...
Settings come from the environment: ZBMTEST_WORK, ZBMTEST_IMAGE,
ZBMTEST_RUNTIME, ZBMTEST_ACCEL, ZBMTEST_REUSE, ZBMTEST_ZBM_RUNTIME,
ZBMTEST_OLD_KERNEL, ZBMTEST_OLD_INITRD, ZBMTEST_REPO.
";

/// One command of the command line.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Run(Vec<String>),
    List,
    Scenario(String),
    Compare(PathBuf, PathBuf, Vec<String>),
    Help,
}

/// Parses the arguments after the program's name.
fn parse(args: &[String]) -> Result<Command, String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let owned = |names: &[&str]| {
        names
            .iter()
            .map(|&name| name.to_owned())
            .collect::<Vec<_>>()
    };
    let command = match words.as_slice() {
        [] | ["help" | "--help" | "-h"] => Command::Help,
        ["list"] => Command::List,
        ["run", tests @ ..] => Command::Run(owned(tests)),
        ["scenario", test] => Command::Scenario((*test).to_owned()),
        ["compare", first, second, tests @ ..] => {
            Command::Compare(first.into(), second.into(), owned(tests))
        }
        _ => return Err(format!("unknown command: {}", words.join(" "))),
    };
    let tests: &[String] = match &command {
        Command::Run(tests) => tests,
        Command::Scenario(test) => std::slice::from_ref(test),
        _ => &[],
    };
    if let Some(unknown) = tests.iter().find(|test| !scenarios::exists(test)) {
        return Err(format!(
            "unknown test {unknown}; `zbm-vmtest list` lists them"
        ));
    }
    Ok(command)
}

/// Runs `command`; returns whether it succeeded.
fn execute(command: Command) -> Result<bool, String> {
    let settings = Settings::from_env();
    match command {
        Command::Help => {
            print!("{USAGE}");
            Ok(true)
        }
        Command::List => {
            ALL.iter().for_each(|test| println!("{test}"));
            Ok(true)
        }
        Command::Run(tests) => {
            let tests = if tests.is_empty() {
                ALL.iter().map(|&test| test.to_owned()).collect()
            } else {
                tests
            };
            let program = std::env::current_exe().map_err(|error| error.to_string())?;
            runner::run(&settings, &program, &tests).map_err(|error| error.to_string())
        }
        Command::Scenario(test) => {
            let outcome = scenarios::run(&settings, &test).map_err(|error| error.to_string())?;
            Ok(outcome.is_pass())
        }
        Command::Compare(first, second, tests) => {
            Ok(compare::has_same_checks(&first, &second, &tests))
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args).and_then(execute) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("zbm-vmtest: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parse_line(line: &str) -> Result<Command, String> {
        let args: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        parse(&args)
    }

    #[test]
    fn commands_are_parsed() {
        assert_eq!(parse_line(""), Ok(Command::Help));
        assert_eq!(parse_line("list"), Ok(Command::List));
        assert_eq!(parse_line("run"), Ok(Command::Run(Vec::new())));
        assert_eq!(
            parse_line("run keys-setup keys-plugin"),
            Ok(Command::Run(vec![
                "keys-setup".into(),
                "keys-plugin".into()
            ]))
        );
        assert_eq!(
            parse_line("scenario procedures"),
            Ok(Command::Scenario("procedures".into()))
        );
        assert_eq!(
            parse_line("compare /a /b runtime-setup"),
            Ok(Command::Compare(
                "/a".into(),
                "/b".into(),
                vec!["runtime-setup".into()]
            ))
        );
    }

    #[test]
    fn unknown_commands_and_tests_are_refused() {
        assert!(parse_line("frobnicate").is_err());
        assert!(
            parse_line("run keys-nothing")
                .unwrap_err()
                .contains("keys-nothing")
        );
        assert!(parse_line("scenario").is_err());
    }

    #[test]
    fn help_and_list_succeed() {
        assert_eq!(execute(Command::Help), Ok(true));
        assert_eq!(execute(Command::List), Ok(true));
    }
}
