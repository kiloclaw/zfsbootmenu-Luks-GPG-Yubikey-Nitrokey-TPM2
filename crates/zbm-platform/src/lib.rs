//! The ports through which ZFSBootMenu reaches the system, their real
//! implementations, and the [`Context`] that carries them.
//!
//! Code that runs programs, reads or writes files, waits, talks to the user or
//! signals processes does so through these traits, so that unit tests can
//! replace the system with the fakes of `zbm-testkit`, which record what the
//! code did (see "Testing" in `crates/DESIGN.md`):
//!
//! - [`Runner`]: external programs, described by a [`Command`] ([`exec`]);
//!   [`SystemRunner`].
//! - [`Files`]: files, directories, `/proc`, `/sys` and device nodes;
//!   [`RealFiles`].
//! - [`Clock`]: time and waiting; [`SystemClock`].
//! - [`Console`]: prompts, key presses and passphrases; [`Terminal`].
//! - [`Processes`]: the process ID and signals; [`SystemProcesses`].
//! - [`Environment`]: environment variables; [`ProcessEnvironment`].
//!
//! [`System`] bundles one of each, so functions take a single parameter for
//! all of them. [`Secret`] holds passphrases and keys and wipes them after
//! use; [`error::Error`] is the runtime's error for failed file operations and
//! programs. [`KernelLog`] writes the kernel log through [`Files`]: code logs
//! with `zbm_trace`'s macros, and a program installs the log once it knows the
//! log level ([`System::kernel_log`]); [`dmesg`] reads it back.
//! [`CrashReporter`] is the panic hook that leaves crash reports in the state
//! directory.
//!
//! Smaller helpers: a program's arguments as text ([`args`]), colours
//! ([`color`]), shell quoting ([`shell::quote`]), the ZFSBootMenu version
//! ([`version`]), and the file tests of Perl and the shell that the host tools
//! use on the build host's files ([`filetest`]).
//!
//! A service (unlocking a volume, importing a pool) is given a [`Context`]:
//! [`Context::new`] wraps the runner in a [`TracedRunner`], so every program
//! a service runs appears in the journal of `zbm-trace` with its arguments,
//! exit status and duration: the command journal.
//!
//! ```
//! use zbm_platform::{Command, Context};
//! use zbm_testkit::fakes::FakeSystem;
//!
//! let fake = FakeSystem::new();
//! fake.runner.respond("cryptsetup --version", 0, "cryptsetup 2.8.7\n");
//! let context = Context::new(fake.system());
//!
//! let command = Command::new("cryptsetup").arg("--version");
//! let output = context.runner().run(&command)?;
//! assert_eq!(output.stdout_text(), "cryptsetup 2.8.7\n");
//! # Ok::<(), std::io::Error>(())
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod args;
mod cancelling;
pub mod clock;
pub mod color;
pub mod console;
pub mod crash;
pub mod dmesg;
pub mod env;
pub mod error;
pub mod event;
pub mod exec;
pub mod files;
pub mod filetest;
pub mod hid;
pub mod interaction;
pub mod kernel;
pub mod log;
pub mod process;
mod quiet;
pub mod sandbox;
pub mod secret;
pub mod shell;
mod traced;
pub mod tree;
pub mod version;

pub use cancelling::CancellingRunner;
pub use clock::{Clock, SystemClock};
pub use console::{Console, NoConsole, Size, Terminal};
pub use crash::CrashReporter;
pub use env::{Environment, ProcessEnvironment};
pub use exec::{Command, Input, Output, Runner, Status, Stream, SystemRunner, TerminalRunner};
pub use files::{FileKind, FileLock, Files, LockWait, Metadata, RealFiles};
pub use hid::{FeatureReports, HidDevices, NoHid, Report, SystemHid};
pub use interaction::Interaction;
pub use kernel::{KernelLoader, NoKernelLoader, SystemKernelLoader};
pub use log::{FilesAppender, FilesSink, Installed, JOURNAL, JOURNAL_LIMIT, KernelLog};
pub use process::{Processes, Signal, SystemProcesses};
pub use quiet::QuietRunner;
pub use secret::Secret;
pub use traced::TracedRunner;

/// One implementation of each system interface.
#[derive(Clone, Copy)]
pub struct System<'a> {
    /// External programs.
    pub runner: &'a dyn Runner,
    /// Files and directories.
    pub files: &'a dyn Files,
    /// Time and waiting.
    pub clock: &'a dyn Clock,
    /// The user.
    pub console: &'a dyn Console,
    /// The process and signals.
    pub processes: &'a dyn Processes,
    /// Environment variables.
    pub environment: &'a dyn Environment,
    /// Loading and starting another kernel.
    pub kernel: &'a dyn KernelLoader,
    /// HID devices' feature reports, which YubiKeys answer.
    pub hid: &'a dyn HidDevices,
}

impl<'a> System<'a> {
    /// This system with `runner` for its programs, such as a
    /// [`TracedRunner`] over its own, so that the journal records each one.
    #[must_use]
    pub fn with_runner(self, runner: &'a dyn Runner) -> System<'a> {
        System { runner, ..self }
    }
}

impl System<'_> {
    /// Sends this thread's kernel log messages to this system's files, for
    /// process `self.processes.id()`, until the returned guard is dropped.
    pub fn kernel_log(&self, loglevel: &str, base: &std::path::Path) -> Installed {
        KernelLog::new(self.files, loglevel, base, self.processes.id()).install()
    }

    /// Whether `name` is an executable file in a directory of `PATH`, as
    /// `command -v` finds it.
    pub fn command_exists(&self, name: &str) -> bool {
        command_exists(self.files, self.environment, name)
    }
}

/// The real system, owning its parts, from which a [`System`] is borrowed.
pub struct RealSystem {
    runner: SystemRunner,
    files: RealFiles,
    clock: SystemClock,
    processes: SystemProcesses,
    environment: ProcessEnvironment,
    kernel: SystemKernelLoader,
    hid: SystemHid,
}

impl RealSystem {
    /// The real system; its clock starts now.
    pub fn new() -> RealSystem {
        RealSystem {
            runner: SystemRunner,
            files: RealFiles,
            clock: SystemClock::new(),
            processes: SystemProcesses,
            environment: ProcessEnvironment,
            kernel: SystemKernelLoader,
            hid: SystemHid,
        }
    }

    /// The console on standard input and output, which the start-up sequence
    /// connects to the control terminal.
    ///
    /// # Errors
    ///
    /// When standard input or output cannot be duplicated.
    pub fn terminal(&self) -> std::io::Result<Terminal> {
        Terminal::standard()
    }

    /// The ports of the real system without a console, for programs that
    /// need none, such as `zluks`.
    pub fn context(&self) -> Context<'_> {
        Context::new(self.with_console(&NoConsole))
    }

    /// The system, with `console` as the user's console.
    pub fn with_console<'a>(&'a self, console: &'a dyn Console) -> System<'a> {
        System {
            runner: &self.runner,
            files: &self.files,
            clock: &self.clock,
            console,
            processes: &self.processes,
            environment: &self.environment,
            kernel: &self.kernel,
            hid: &self.hid,
        }
    }
}

impl Default for RealSystem {
    fn default() -> RealSystem {
        RealSystem::new()
    }
}

/// The ports a service may use: every part of the [`System`] but the
/// console, which only front ends reach, through their [`Interaction`].
///
/// It is cheap to copy: it holds references to the system's parts. Front ends
/// build one per program run and pass it by reference.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    /// External programs, each reported to the journal.
    runner: TracedRunner<'a>,
    /// Files, directories and device nodes.
    files: &'a dyn Files,
    /// Time and waiting.
    clock: &'a dyn Clock,
    /// The process and signals.
    processes: &'a dyn Processes,
    /// Environment variables.
    environment: &'a dyn Environment,
    /// HID devices' feature reports.
    hid: &'a dyn HidDevices,
}

impl<'a> Context<'a> {
    /// The ports of `system` without its console, with its runner traced.
    /// A runner traced already reports each program once.
    pub fn new(system: System<'a>) -> Context<'a> {
        Context {
            runner: TracedRunner::new(system.runner, system.clock),
            files: system.files,
            clock: system.clock,
            processes: system.processes,
            environment: system.environment,
            hid: system.hid,
        }
    }

    /// External programs.
    pub fn runner(&self) -> &dyn Runner {
        &self.runner
    }

    /// These ports with the programs `runner` runs, traced, such as a
    /// [`CancellingRunner`] around [`Context::runner`].
    pub fn with_runner<'b>(&self, runner: &'b dyn Runner) -> Context<'b>
    where
        'a: 'b,
    {
        Context {
            runner: TracedRunner::new(runner, self.clock),
            ..*self
        }
    }

    /// Files, directories and device nodes.
    pub fn files(&self) -> &'a dyn Files {
        self.files
    }

    /// Time and waiting.
    pub fn clock(&self) -> &'a dyn Clock {
        self.clock
    }

    /// The process and signals.
    pub fn processes(&self) -> &'a dyn Processes {
        self.processes
    }

    /// Environment variables.
    pub fn environment(&self) -> &'a dyn Environment {
        self.environment
    }

    /// HID devices' feature reports.
    pub fn hid(&self) -> &'a dyn HidDevices {
        self.hid
    }

    /// Sends this thread's kernel log messages to this context's files, for
    /// this process, until the returned guard is dropped.
    pub fn kernel_log(&self, loglevel: &str, base: &std::path::Path) -> Installed {
        KernelLog::new(self.files, loglevel, base, self.processes.id()).install()
    }

    /// Whether `name` is an executable file in a directory of `PATH`, as
    /// `command -v` finds it.
    pub fn command_exists(&self, name: &str) -> bool {
        command_exists(self.files, self.environment, name)
    }
}

/// Whether `name` is an executable file in a directory of `PATH` in
/// `environment`, as `command -v` finds it.
fn command_exists(files: &dyn Files, environment: &dyn Environment, name: &str) -> bool {
    let path = environment.var("PATH").unwrap_or_default();
    path.split(':').filter(|dir| !dir.is_empty()).any(|dir| {
        files
            .metadata(&std::path::Path::new(dir).join(name))
            .is_ok_and(|metadata| metadata.kind == FileKind::File && metadata.is_executable())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_system_is_assembled_from_its_parts() {
        let real = RealSystem::default();
        let terminal = real.terminal().unwrap();
        let system = real.with_console(&terminal);
        assert_eq!(system.processes.id(), std::process::id());
        assert!(system.environment.var("PATH").is_some());
        assert!(system.files.exists(std::path::Path::new("/proc/self")));
        let _log = system.kernel_log("x", std::path::Path::new("/nonexistent"));
        assert!(!zbm_trace::enabled(zbm_trace::Level::Error));
    }

    #[test]
    fn a_system_can_take_another_runner() {
        let real = RealSystem::default();
        let terminal = real.terminal().unwrap();
        let system = real.with_console(&terminal);
        let traced = TracedRunner::new(system.runner, system.clock);
        let traced_system = system.with_runner(&traced);
        assert!(std::ptr::addr_eq(traced_system.runner, &raw const traced));
        assert!(std::ptr::addr_eq(traced_system.files, system.files));
    }

    #[test]
    fn a_context_hands_out_the_parts_it_was_given() {
        let real = RealSystem::default();
        let context = real.context();
        let _runner = context.runner();
        assert!(context.files().exists(std::path::Path::new("/proc/self")));
        let before = context.clock().now();
        assert!(context.clock().now() >= before);
        assert_eq!(context.processes().id(), std::process::id());
        assert!(context.environment().var("PATH").is_some());
        assert!(context.command_exists("sh"));
        let _log = context.kernel_log("x", std::path::Path::new("/nonexistent"));
    }
}
