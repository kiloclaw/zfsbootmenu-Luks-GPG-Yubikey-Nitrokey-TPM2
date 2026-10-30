//! `zbm unlock serve`: the keyholder (ROADMAP.md, SB10; ADR 0002, zone Z1),
//! a process that opens LUKS volumes for the one that started it, and so
//! the only one with what opens them. Start-up and `zluks` start one for
//! each unlocking run ([`ProcessKeyholders`]) on a socket pair whose end it
//! finds in [`KEYHOLDER_FD`], and answer what it asks
//! (`zbm_app::keyholder`). It reads the configuration as `zluks` does, runs
//! the action it is asked, says how it ended, and ends, its secrets wiped.
//!
//! ```text
//! zbm unlock serve [--quiet] [--journal FILE] [--verbose]
//! ```
//!
//! `--quiet`: its programs leave the console to the client's front end,
//! which draws on it; `--journal` and `--verbose` trace as `zluks open
//! --key-file` asked.

#![allow(
    clippy::disallowed_methods,
    reason = "the keyholder is this program again, which the client starts \
              itself and talks to while it runs, which the Runner does not do"
)]

use std::io;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use zbm_app::Volumes;
use zbm_app::keyholder::{KEYHOLDER_FD, Keyholder, Keyholders, serve};
use zbm_app::model::{Event, Outcome};
use zbm_luks::unlocker::Luks;
use zbm_platform::env::{Environment as _, ProcessEnvironment};
use zbm_platform::sandbox::{self, Sandboxed as _};
use zbm_platform::{Context, QuietRunner, RealSystem, System};
use zbm_sys::Signal;
use zbm_sys::signals::Process;
use zbm_sys::socket::Connection;

use crate::Io;

/// The usage of `zbm unlock`.
const USAGE: &str = "\
Usage: zbm unlock serve [--quiet] [--journal FILE] [--verbose]

The keyholder: opens LUKS volumes for the program that started it, on the
connection it passed in ZBM_KEYHOLDER_FD. Start-up and zluks start it.
";

/// How long a client waits for its keyholder to end once the run is over,
/// before it kills it.
const END_WAIT: Duration = Duration::from_secs(5);

/// How the keyholder runs.
#[derive(Debug, Default, Eq, PartialEq)]
struct Options {
    quiet: bool,
    journal: Option<PathBuf>,
    verbose: bool,
}

/// The options of `zbm unlock serve`.
fn parse(args: &[String]) -> Result<Options, String> {
    let mut words = args.iter().map(String::as_str);
    if words.next() != Some("serve") {
        return Err("the command is serve".to_owned());
    }
    let mut options = Options::default();
    while let Some(word) = words.next() {
        match word {
            "--quiet" => options.quiet = true,
            "--verbose" => options.verbose = true,
            "--journal" => {
                let path = words.next().ok_or("--journal FILE needs a file")?;
                options.journal = Some(PathBuf::from(path));
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

/// `zbm unlock`: the keyholder.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let options = match parse(args) {
        Ok(options) => options,
        Err(message) => {
            let _ = writeln!(io.stderr, "zbm unlock: {message}");
            let _ = io.stderr.write_all(USAGE.as_bytes());
            return crate::USAGE;
        }
    };
    let connection = match connection() {
        Ok(connection) => connection,
        Err(message) => {
            let _ = writeln!(io.stderr, "zbm unlock: {message}");
            return 1;
        }
    };
    // Ctrl-\ ends the client's program no more than it ended the client;
    // the other signals are the client's, as when it unlocked itself.
    let _ = zbm_sys::discard_signal(Signal::Quit);
    if options.journal.is_some() || options.verbose {
        let traced =
            crate::luks::key_file::install_tracing_to(options.journal.as_deref(), options.verbose);
        if let Err(message) = traced {
            let _ = writeln!(io.stderr, "zbm unlock: {message}");
        }
    }
    match keep(&connection, &options) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm unlock: the client went: {error}");
            1
        }
    }
}

/// The keyholder's end of the connection, from its parent, which must be
/// of this process's user.
fn connection() -> Result<Connection, String> {
    let number = ProcessEnvironment
        .var(KEYHOLDER_FD)
        .and_then(|number| number.parse().ok())
        .ok_or_else(|| format!("no connection in {KEYHOLDER_FD}"))?;
    let connection = Connection::inherited(number).map_err(|error| error.to_string())?;
    let user = zbm_sys::credentials().uid;
    match connection.peer() {
        Ok(peer) if peer.uid == user => Ok(connection),
        _ => Err("the connection is not of this user".to_owned()),
    }
}

/// Answers the run its client asks on `connection`, on the real system.
fn keep(connection: &Connection, options: &Options) -> io::Result<()> {
    let real = RealSystem::new();
    let terminal = real.terminal()?;
    let system = real.with_console(&terminal);
    let Ok(conf) = zbm_startup::preinit::read_conf(&system, Path::new("/")) else {
        return serve(connection, system.runner, |_, _| {
            vec![Event::Finished(Outcome::Failed {
                what: "unable to unlock".to_owned(),
                why: vec![format!("unable to source {}", zbm_config::conf::PATH)],
                next: Vec::new(),
            })]
        });
    };
    let settings = crate::luks::settings(&system, &conf);
    let _log = system.kernel_log(conf.get("loglevel").unwrap_or_default(), &settings.base);
    let quiet_runner = QuietRunner::new(system.runner);
    let runs = if options.quiet {
        System {
            runner: &quiet_runner,
            ..system
        }
    } else {
        system
    };
    serve(connection, system.runner, |interaction, request| {
        let mut settings = settings.clone();
        settings.retry = request.retry;
        let luks = Luks {
            context: Context::new(runs),
            settings: &settings,
            interaction,
        };
        Volumes::new(luks).apply(&request.action)
    })
}

/// Starts keyholders as processes of this program, `zbm unlock serve`.
#[derive(Clone, Debug)]
pub struct ProcessKeyholders {
    program: PathBuf,
    args: Vec<String>,
}

impl Default for ProcessKeyholders {
    fn default() -> ProcessKeyholders {
        ProcessKeyholders::new()
    }
}

impl ProcessKeyholders {
    /// Keyholders of this program.
    pub fn new() -> ProcessKeyholders {
        ProcessKeyholders {
            program: crate::applets::zbm(),
            args: vec!["unlock".to_owned(), "serve".to_owned()],
        }
    }

    /// The same keyholders, of `program`.
    #[must_use]
    pub fn of(mut self, program: &Path) -> ProcessKeyholders {
        program.clone_into(&mut self.program);
        self
    }

    /// The same keyholders, whose programs leave the console alone, for a
    /// client whose front end draws on it.
    #[must_use]
    pub fn quiet(mut self) -> ProcessKeyholders {
        self.args.push("--quiet".to_owned());
        self
    }

    /// The same keyholders, tracing to `journal`, and on standard error
    /// when `verbose`, as `zluks open --key-file` asked.
    #[must_use]
    pub fn traced(mut self, journal: Option<&Path>, verbose: bool) -> ProcessKeyholders {
        if let Some(journal) = journal {
            self.args.push("--journal".to_owned());
            self.args.push(journal.to_string_lossy().into_owned());
        }
        if verbose {
            self.args.push("--verbose".to_owned());
        }
        self
    }
}

impl Keyholders for ProcessKeyholders {
    fn start(&self) -> io::Result<Box<dyn Keyholder + '_>> {
        let (ours, theirs) = Connection::pair()?;
        let mut command = Command::new(&self.program);
        command.arg0("zbm").args(&self.args);
        let end = std::os::fd::AsFd::as_fd(&theirs).try_clone_to_owned()?;
        let number = zbm_sys::pass_fd(&mut command, end);
        command.env(KEYHOLDER_FD, number.to_string());
        command.sandboxed(&sandbox::Keyholder)?;
        let child = command.spawn()?;
        // The keyholder's end is its alone now.
        drop(command);
        drop(theirs);
        zbm_trace::info!("keyholder {keyholder} started"; keyholder = child.id());
        Ok(Box::new(Running {
            connection: ours,
            _process: Collected(child),
        }))
    }
}

/// A keyholder process. Its fields drop in order: the connection closes,
/// which ends a keyholder that did not finish, and then it is collected.
struct Running {
    connection: Connection,
    _process: Collected,
}

impl Keyholder for Running {
    fn connection(&self) -> &Connection {
        &self.connection
    }
}

/// A child process, collected when this is dropped; one that does not end
/// in [`END_WAIT`] is killed first.
struct Collected(Child);

impl Drop for Collected {
    fn drop(&mut self) {
        let keyholder = self.0.id();
        let ended = Process::open(keyholder)
            .and_then(|process| process.wait_exit(Some(END_WAIT)))
            .unwrap_or(true);
        if !ended {
            zbm_trace::warn!("keyholder {keyholder} did not end, so it is killed"; keyholder);
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests;
