//! The interfaces through which the ZFSBootMenu runtime reaches the system.
//!
//! Code that runs programs, reads or writes files, waits, talks to the user or
//! signals processes does so through these traits, so that unit tests can
//! replace the system with the fakes of `zbm-testutil`, which record what the
//! code did (see "Testing" in `crates/DESIGN.md`):
//!
//! - [`Runner`] (from `zbm-core`): external programs; [`SystemRunner`].
//! - [`Files`]: files, directories, `/proc`, `/sys` and device nodes;
//!   [`RealFiles`].
//! - [`Clock`]: time and waiting; [`SystemClock`].
//! - [`Console`]: prompts, key presses and passphrases; [`Terminal`].
//! - [`Processes`]: the process ID and signals; [`SystemProcesses`].
//! - [`Environment`]: environment variables; [`ProcessEnvironment`].
//!
//! [`System`] bundles one of each, so functions take a single parameter for
//! all of them. [`KernelLog`] writes the kernel log through [`Files`].

pub mod clock;
pub mod console;
pub mod env;
pub mod files;
pub mod log;
pub mod process;

pub use clock::{Clock, SystemClock};
pub use console::{Console, Size, Terminal};
pub use env::{Environment, ProcessEnvironment};
pub use files::{FileKind, Files, Metadata, RealFiles};
pub use log::KernelLog;
pub use process::{Processes, Signal, SystemProcesses};
pub use zbm_core::exec::{Runner, SystemRunner};

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
}

impl<'a> System<'a> {
    /// A log on this system's files, for process `self.processes.id()`.
    pub fn kernel_log(&self, loglevel: &str, base: &std::path::Path) -> KernelLog<'a> {
        KernelLog::new(self.files, loglevel, base, self.processes.id())
    }

    /// Whether `name` is an executable file in a directory of `PATH`, as
    /// `command -v` finds it.
    pub fn command_exists(&self, name: &str) -> bool {
        let path = self.environment.var("PATH").unwrap_or_default();
        path.split(':').filter(|dir| !dir.is_empty()).any(|dir| {
            self.files
                .metadata(&std::path::Path::new(dir).join(name))
                .is_ok_and(|metadata| metadata.kind == FileKind::File && metadata.is_executable())
        })
    }
}

/// The real system, owning its parts, from which a [`System`] is borrowed.
pub struct RealSystem {
    runner: SystemRunner,
    files: RealFiles,
    clock: SystemClock,
    processes: SystemProcesses,
    environment: ProcessEnvironment,
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
        }
    }

    /// The console on standard input and output, which the start-up sequence
    /// connects to the control terminal.
    pub fn terminal(&self) -> std::io::Result<Terminal<'_>> {
        Terminal::standard(&self.runner)
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
        }
    }
}

impl Default for RealSystem {
    fn default() -> RealSystem {
        RealSystem::new()
    }
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
        let log = system.kernel_log("x", std::path::Path::new("/nonexistent"));
        assert!(!log.error("not written: the level is invalid"));
    }
}
