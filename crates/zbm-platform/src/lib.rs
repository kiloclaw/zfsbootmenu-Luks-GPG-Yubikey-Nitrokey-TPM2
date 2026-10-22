//! The ports through which ZFSBootMenu's services reach the system, and the
//! [`Context`] that carries them.
//!
//! A service (unlocking a volume, importing a pool) never runs a program or
//! reads a file itself: it asks the ports in the [`Context`] it is given.
//! Programs and tests choose what is behind them, the real system or the
//! fakes of `zbm-testkit`, which is what makes every service testable
//! without root, devices or ZFS.
//!
//! The ports are the interfaces of `zbm-core` and `zbm-system`, re-exported
//! here so that services depend on one crate. REFACTOR-ROADMAP.md (R2) moves
//! them into this crate.
//!
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

mod traced;

pub use traced::TracedRunner;
pub use zbm_core::exec::{Command, Input, Output, Runner, Status, Stream};
pub use zbm_core::secret::Secret;
pub use zbm_system::{Clock, Files, RealFiles, RealSystem, System, SystemClock, SystemRunner};

/// The ports a service may use.
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
}

impl<'a> Context<'a> {
    /// The ports of `system`, with its runner traced.
    pub fn new(system: System<'a>) -> Context<'a> {
        Context {
            runner: TracedRunner::new(system.runner, system.clock),
            files: system.files,
            clock: system.clock,
        }
    }

    /// The ports `runner`, `files` and `clock`, with the runner traced.
    ///
    /// For programs that need no console, such as `zluks --pilot`: a full
    /// [`System`] includes one, which needs a terminal.
    pub fn from_parts(
        runner: &'a dyn Runner,
        files: &'a dyn Files,
        clock: &'a dyn Clock,
    ) -> Context<'a> {
        Context {
            runner: TracedRunner::new(runner, clock),
            files,
            clock,
        }
    }

    /// External programs.
    pub fn runner(&self) -> &dyn Runner {
        &self.runner
    }

    /// Files, directories and device nodes.
    pub fn files(&self) -> &'a dyn Files {
        self.files
    }

    /// Time and waiting.
    pub fn clock(&self) -> &'a dyn Clock {
        self.clock
    }
}
