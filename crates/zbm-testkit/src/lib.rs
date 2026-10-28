//! Test helpers of the Rust port of ZFSBootMenu.
//!
//! Only tests use this crate (as a dev-dependency), so nothing here reaches a
//! binary or the boot image.
//!
//! - [`FakeRunner`] replaces the system's programs: it records every command
//!   and answers it.
//! - [`fakes`] replaces the other ports of `zbm-platform` (files, the clock,
//!   the console, processes) and bundles them as a `FakeSystem`.
//! - [`machine`] builds a `FakeMachine`: a `FakeSystem` with LUKS volumes,
//!   pools and key files that its fake programs answer for.
//! - [`mod@assert`] holds the assertions: `assert_ran!`, `assert_not_ran!`,
//!   `assert_no_secret_leaked!` and the table-driven `cases!`.
//! - [`legacy`] runs the Bash and Perl code in `legacy/`, and [`mod@parity`]
//!   records its results as golden files, so parity tests compare the Rust code
//!   with the legacy code without running it.
//! - [`efi`] describes EFI images without what comes from the host's stub.
//! - [`Rng`] generates test inputs from a seed, so a failing generated case can
//!   be reproduced.
//! - [`TempDir`] is a scratch directory that is removed afterwards.
//! - [`png`] writes drawn frames as PNG files, the reference images of
//!   snapshot tests.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]
#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::missing_panics_doc,
    reason = "this is test code: a helper fails the test by panicking, \
              as the standard library's assertions do, \
              so panics are its errors and need no `# Panics` section"
)]

use std::io;
use std::sync::{Arc, Mutex};

use zbm_platform::exec::{Command, Output, Runner, Status};

pub mod assert;
pub mod efi;
pub mod fakes;
mod host;
pub mod legacy;
pub mod machine;
pub mod parity;
pub mod png;
pub mod redirect;
mod rng;
mod sync;
pub mod zfs;
pub mod zfs_script;

pub use host::{TempDir, can_run, have_program, repo_root, write_executable};
pub use rng::Rng;
pub use sync::Shared;

/// A response function: returns the output for commands it handles and `None`
/// for the others.
type Responder = Arc<dyn Fn(&Command) -> Option<io::Result<Output>> + Send + Sync>;

/// A [`Runner`] that records commands and answers them without running
/// anything.
///
/// Responders are tried in the order they were added; a command no responder
/// handles succeeds with no output.
#[derive(Default)]
pub struct FakeRunner {
    calls: Mutex<Vec<Command>>,
    responders: Mutex<Vec<Responder>>,
}

impl FakeRunner {
    /// A runner with no responders.
    pub fn new() -> FakeRunner {
        FakeRunner::default()
    }

    /// Adds a responder.
    pub fn on(
        &self,
        responder: impl Fn(&Command) -> Option<io::Result<Output>> + Send + Sync + 'static,
    ) {
        sync::lock(&self.responders).push(Arc::new(responder));
    }

    /// Adds a responder tried before all the others, such as one that does
    /// something as a command runs and returns `None` to let the others
    /// answer it.
    pub fn first(
        &self,
        responder: impl Fn(&Command) -> Option<io::Result<Output>> + Send + Sync + 'static,
    ) {
        sync::lock(&self.responders).insert(0, Arc::new(responder));
    }

    /// Answers commands whose program and arguments, joined by spaces, equal
    /// `line`.
    pub fn respond(&self, line: &str, status: u8, stdout: &str) {
        let line = line.to_owned();
        let output = Output::new(Status::exited(status), stdout.as_bytes());
        self.on(move |command| (command.display() == line).then(|| Ok(output.clone())));
    }

    /// Answers commands whose program is `program`, whatever their arguments.
    pub fn respond_program(&self, program: &str, status: u8, stdout: &str) {
        let program = program.to_owned();
        let output = Output::new(Status::exited(status), stdout.as_bytes());
        self.on(move |command| (command.program() == program.as_str()).then(|| Ok(output.clone())));
    }

    /// The commands run so far.
    pub fn calls(&self) -> Vec<Command> {
        sync::lock(&self.calls).clone()
    }

    /// The commands run so far, each as its program and arguments joined by spaces.
    pub fn lines(&self) -> Vec<String> {
        sync::lock(&self.calls)
            .iter()
            .map(Command::display)
            .collect()
    }
}

impl Runner for FakeRunner {
    fn run(&self, command: &Command) -> io::Result<Output> {
        sync::lock(&self.calls).push(command.clone());
        // Out of the lock, so that a responder may run commands itself.
        let responders = sync::lock(&self.responders).clone();
        responders
            .iter()
            .find_map(|responder| responder(command))
            .unwrap_or_else(|| Ok(Output::new(Status::exited(0), Vec::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_runner_records_and_answers() {
        let runner = FakeRunner::new();
        runner.respond("zfs list -H -o name pool", 0, "pool\n");
        runner.respond_program("objcopy", 1, "");
        let listed = runner
            .run(&Command::new("zfs").args(["list", "-H", "-o", "name", "pool"]))
            .unwrap();
        assert_eq!(listed.stdout_text(), "pool\n");
        let copied = runner.run(&Command::new("objcopy").arg("x")).unwrap();
        assert_eq!(copied.status.code(), Some(1));
        let other = runner.run(&Command::new("true")).unwrap();
        assert!(other.status.is_success());
        assert_eq!(
            runner.lines(),
            ["zfs list -H -o name pool", "objcopy x", "true"]
        );
    }
}
