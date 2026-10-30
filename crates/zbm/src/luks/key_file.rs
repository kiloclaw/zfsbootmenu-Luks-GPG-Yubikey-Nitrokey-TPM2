//! `zluks open DEVICE NAME --key-file PATH`: one volume opened with a key
//! file and no other key method (ROADMAP.md DM6), through `zbm-app` as the
//! other commands of `zluks` are, with the journal of its events in a file
//! when asked. It took over from `zluks --pilot open`, the first pilot of
//! the refactoring (R0).

use std::fs::OpenOptions as FileOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use zbm_app::OpenOptions;
use zbm_app::model::{Action, Event, Outcome};
use zbm_trace::{Config, LevelFilter};

use crate::Io;

/// What `zluks open --key-file` was asked to do.
#[derive(Debug, Eq, PartialEq)]
pub struct KeyFileOpen {
    /// The volume and how to open it.
    pub action: Action,
    /// Where to append the journal, if anywhere.
    pub journal: Option<PathBuf>,
    /// Whether informational messages are printed.
    pub verbose: bool,
}

/// Whether `args`, the arguments of `zluks`, ask to open a volume with a key
/// file alone.
pub fn is_asked(args: &[String]) -> bool {
    args.first().is_some_and(|command| command == "open")
        && args.iter().any(|arg| arg == "--key-file")
}

/// Parses the arguments after `zluks open`.
///
/// # Errors
///
/// A message saying what is wrong with the arguments.
pub fn parse(args: &[String]) -> Result<KeyFileOpen, String> {
    let mut words = args.iter().map(String::as_str);
    let mut positional = Vec::new();
    let mut key_file = None;
    let mut journal = None;
    let mut options = OpenOptions::default();
    let mut verbose = false;
    while let Some(word) = words.next() {
        match word {
            "--key-file" => key_file = words.next().map(PathBuf::from),
            "--journal" => journal = words.next().map(PathBuf::from),
            "--allow-discards" => options.allow_discards = true,
            "--read-only" => options.read_only = true,
            "--verbose" => verbose = true,
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            value => positional.push(value),
        }
    }
    let [device, name] = positional.as_slice() else {
        return Err("expected DEVICE and NAME".to_owned());
    };
    let key_file = key_file.ok_or("--key-file PATH is required")?;
    Ok(KeyFileOpen {
        action: Action::OpenWithKeyFile {
            device: device.parse().map_err(|error| format!("{error}"))?,
            name: name.parse().map_err(|error| format!("{error}"))?,
            key_file,
            options,
        },
        journal,
        verbose,
    })
}

/// Sends the program's traces to standard error and, if asked, the journal.
///
/// # Errors
///
/// When the journal cannot be opened, or traces are sent elsewhere already.
pub fn install_tracing(parsed: &KeyFileOpen) -> Result<(), String> {
    install_tracing_to(parsed.journal.as_deref(), parsed.verbose)
}

/// Sends the program's traces to standard error, informational messages
/// too when `verbose`, and to `journal`, if any: `zluks open --key-file`'s
/// and its keyholder's.
///
/// # Errors
///
/// When the journal cannot be opened, or traces are sent elsewhere already.
#[allow(
    clippy::disallowed_methods,
    reason = "the journal file is opened by the program, as `--journal` names it"
)]
pub fn install_tracing_to(journal: Option<&Path>, verbose: bool) -> Result<(), String> {
    let journal = match journal {
        Some(path) => {
            let file = FileOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|error| format!("cannot open the journal {}: {error}", path.display()))?;
            Some(Box::new(file) as Box<dyn Write + Send>)
        }
        None => None,
    };
    let stderr = if verbose {
        LevelFilter::INFO
    } else {
        LevelFilter::WARN
    };
    zbm_trace::install(Config { stderr, journal }).map_err(|error| error.to_string())
}

/// Says how the action ended: what is open on standard output, or every
/// reason it is not on standard error; the exit status.
pub fn report(events: &[Event], io: &mut Io) -> i32 {
    match events.last() {
        Some(Event::Finished(Outcome::Done(done))) => {
            let _ = writeln!(io.stdout, "{done}");
            0
        }
        Some(Event::Finished(Outcome::Failed { what, why, .. })) => {
            let _ = writeln!(io.stderr, "zluks: {what}");
            for reason in why {
                let _ = writeln!(io.stderr, "  {reason}");
            }
            1
        }
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_command_line_parses_into_an_action() {
        assert!(is_asked(&args("open /dev/sda2 v --key-file /k")));
        assert!(!is_asked(&args("open /dev/sda2 v")));
        let parsed = parse(&args(
            "/dev/sda2 cryptroot --key-file /k --allow-discards --read-only --journal /j --verbose",
        ))
        .unwrap();

        let Action::OpenWithKeyFile {
            device,
            name,
            key_file,
            options,
        } = &parsed.action
        else {
            panic!("{parsed:?}");
        };
        assert_eq!(device.as_str(), "/dev/sda2");
        assert_eq!(name.as_str(), "cryptroot");
        assert_eq!(key_file, &PathBuf::from("/k"));
        assert!(options.allow_discards && options.read_only);
        assert_eq!(parsed.journal, Some(PathBuf::from("/j")));
        assert!(parsed.verbose);
    }

    #[test]
    fn invalid_command_lines_say_what_is_wrong() {
        let cases = [
            ("/dev/sda2 v", "--key-file PATH is required"),
            ("/dev/sda2 --key-file /k", "expected DEVICE and NAME"),
            (
                "sda2 v --key-file /k",
                "\"sda2\" is not a valid device path: it does not start with '/'",
            ),
            ("/dev/sda2 v --key-file /k --fast", "unknown option --fast"),
        ];

        for (line, message) in cases {
            assert_eq!(parse(&args(line)).unwrap_err(), message, "{line}");
        }
    }

    #[test]
    fn the_outcome_goes_to_standard_output_or_error() {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        let done = [Event::Finished(Outcome::Done("opened".to_owned()))];
        assert_eq!(report(&done, &mut io), 0);
        let failed = [Event::Finished(Outcome::Failed {
            what: "unable to open /dev/sda2".to_owned(),
            why: vec!["no key method opened /dev/sda2".to_owned()],
            next: Vec::new(),
        })];
        assert_eq!(report(&failed, &mut io), 1);
        assert_eq!(report(&[], &mut io), 1);
        assert_eq!(String::from_utf8(stdout).unwrap(), "opened\n");
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "zluks: unable to open /dev/sda2\n  no key method opened /dev/sda2\n"
        );
    }
}
