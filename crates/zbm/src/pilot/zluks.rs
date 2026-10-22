//! `zluks --pilot open`: unlocks one volume with a key file through every
//! layer of the target architecture: `zbm-app`, the unlocker of `zbm-luks`,
//! `zbm-crypt`, and `zbm-platform`'s traced runner. It runs beside the
//! legacy-shaped `zluks` and does not record the mapping in ZFSBootMenu's
//! state, so the teardown hook does not close it.

use std::fmt::Write as _;
use std::fs::OpenOptions as FileOptions;
use std::io::Write;
use std::path::PathBuf;

use zbm_app::{Action, App, AppError, OpenOptions, UnlockRequest};
use zbm_platform::{Context, RealFiles, SystemClock, SystemRunner};
use zbm_trace::{Config, LevelFilter};

use crate::{Io, USAGE as USAGE_STATUS};

/// The usage of `zluks --pilot`.
pub const ZLUKS_USAGE: &str = "\
Usage: zluks --pilot open DEVICE NAME --key-file PATH [options]

Opens the LUKS volume DEVICE as /dev/mapper/NAME with the key in PATH, through
the refactored code of REFACTOR-ROADMAP.md (pilot, R0).

Options:
  --allow-discards   pass discard (TRIM) requests through
  --read-only        open the mapping read-only
  --journal FILE     append a JSON line for every event to FILE
  --verbose          print informational messages, not only warnings
";

/// What `zluks --pilot open` was asked to do.
#[derive(Debug, Eq, PartialEq)]
pub struct OpenArgs {
    /// The volume and how to open it.
    pub request: UnlockRequest,
    /// Where to append the journal, if anywhere.
    pub journal: Option<PathBuf>,
    /// Whether informational messages are printed.
    pub verbose: bool,
}

/// Parses the arguments after `zluks --pilot`.
///
/// # Errors
///
/// A message saying what is wrong with the arguments.
pub fn parse_open(args: &[String]) -> Result<OpenArgs, String> {
    let mut words = args.iter().map(String::as_str);
    if words.next() != Some("open") {
        return Err("the only pilot command is `open`".to_owned());
    }
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
    Ok(OpenArgs {
        request: UnlockRequest {
            device: device.parse().map_err(|error| format!("{error}"))?,
            name: name.parse().map_err(|error| format!("{error}"))?,
            key_file,
            options,
        },
        journal,
        verbose,
    })
}

/// `zluks --pilot ARGS`.
pub fn zluks(args: &[String], io: &mut Io) -> i32 {
    let parsed = match parse_open(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            let _ = write!(io.stderr, "zluks: {message}\n\n{ZLUKS_USAGE}");
            return USAGE_STATUS;
        }
    };
    if let Err(message) = install_tracing(&parsed) {
        let _ = writeln!(io.stderr, "zluks: {message}");
        return 1;
    }
    let (runner, files, clock) = (SystemRunner, RealFiles, SystemClock::new());
    let app = App::new(Context::from_parts(&runner, &files, &clock));
    open(&app, parsed.request, io)
}

/// Sends the program's traces to standard error and, if asked, the journal.
#[allow(
    clippy::disallowed_methods,
    reason = "the journal file is opened by the program; R1 moves this into zbm-trace"
)]
fn install_tracing(parsed: &OpenArgs) -> Result<(), String> {
    let journal = match &parsed.journal {
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
    let stderr = if parsed.verbose {
        LevelFilter::INFO
    } else {
        LevelFilter::WARN
    };
    zbm_trace::install(Config { stderr, journal }).map_err(|error| error.to_string())
}

/// Opens the volume of `request` and reports the outcome; returns the exit
/// status.
pub fn open(app: &App<'_>, request: UnlockRequest, io: &mut Io) -> i32 {
    match app.perform(Action::Unlock(request)) {
        Ok(events) => {
            for event in events {
                let _ = writeln!(io.stdout, "{event}");
            }
            0
        }
        Err(error) => {
            let _ = write!(io.stderr, "{}", describe_error(&error));
            1
        }
    }
}

/// The error, then every key method that was tried and why it failed.
fn describe_error(error: &AppError) -> String {
    let mut text = format!("zluks: {error}\n");
    if let AppError::Unlock(failed) = error {
        for (method, refusal) in &failed.attempts {
            let _ = writeln!(text, "  {method}: {refusal}");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::fakes::FakeSystem;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    /// Runs `open` on a fake system and returns the status, standard output
    /// and standard error.
    fn run_open(fake: &FakeSystem, line: &str) -> (i32, String, String) {
        let parsed = parse_open(&args(line)).unwrap();
        let app = App::new(Context::new(fake.system()));
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        let status = open(&app, parsed.request, &mut io);
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
        (status, text(stdout), text(stderr))
    }

    #[test]
    fn the_open_command_line_parses_into_a_request() {
        let parsed = parse_open(&args(
            "open /dev/sda2 cryptroot --key-file /k --allow-discards --journal /j",
        ))
        .unwrap();

        assert_eq!(parsed.request.device.as_str(), "/dev/sda2");
        assert_eq!(parsed.request.name.as_str(), "cryptroot");
        assert_eq!(parsed.request.key_file, PathBuf::from("/k"));
        assert!(parsed.request.options.allow_discards);
        assert_eq!(parsed.journal, Some(PathBuf::from("/j")));
    }

    #[test]
    fn invalid_command_lines_say_what_is_wrong() {
        let cases = [
            ("close x", "the only pilot command is `open`"),
            ("open /dev/sda2 v", "--key-file PATH is required"),
            ("open /dev/sda2 --key-file /k", "expected DEVICE and NAME"),
            (
                "open sda2 v --key-file /k",
                "\"sda2\" is not a valid device path: it does not start with '/'",
            ),
            (
                "open /dev/sda2 v --key-file /k --fast",
                "unknown option --fast",
            ),
        ];

        for (line, message) in cases {
            assert_eq!(parse_open(&args(line)).unwrap_err(), message, "{line}");
        }
    }

    #[test]
    fn an_opened_volume_is_reported_on_standard_output() {
        let fake = FakeSystem::new();
        fake.files.file("/k", "secret");

        let (status, stdout, _) = run_open(&fake, "open /dev/sda2 cryptroot --key-file /k");

        assert_eq!(status, 0);
        assert_eq!(
            stdout,
            "opened /dev/sda2 as /dev/mapper/cryptroot (key file)\n"
        );
        fake.assert_no_leak(b"secret");
    }

    #[test]
    fn a_failure_lists_every_key_method_and_its_reason() {
        let fake = FakeSystem::new();

        let (status, _, stderr) = run_open(&fake, "open /dev/sda2 cryptroot --key-file /k");

        assert_eq!(status, 1);
        assert_eq!(
            stderr,
            "zluks: no key method opened /dev/sda2\n  key file: the key file /k does not exist\n"
        );
    }
}
