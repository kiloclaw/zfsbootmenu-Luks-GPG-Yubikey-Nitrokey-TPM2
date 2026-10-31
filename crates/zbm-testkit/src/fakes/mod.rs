//! Fakes of the system interfaces of `zbm-platform`.
//!
//! [`FakeSystem`] holds one fake of each interface and lends them out as a
//! [`System`]. Each fake records what the code did: the commands it ran
//! ([`FakeRunner`]), the files it changed ([`FakeFiles`]), how long it slept
//! ([`FakeClock`]), what it showed and read ([`FakeConsole`]), the signals it
//! sent ([`FakeProcesses`]) and the kernels it loaded ([`FakeKernelLoader`]);
//! [`FakeEnvironment`] holds the variables the test sets.
//! [`FakeSystem::assert_no_leak`] checks all of them for a secret.

mod clock;
mod console;
mod environment;
mod files;
mod hid;
mod kernel;
mod processes;

use std::ffi::OsStr;
use std::path::Path;

pub use clock::{FakeClock, START};
pub use console::{ConsoleInput, FakeConsole};
pub use environment::FakeEnvironment;
pub use files::{FakeFiles, FileOperation};
pub use hid::{FakeHid, FakeYubiKey, Respond};
pub use kernel::{FakeKernelLoader, Loaded, Loads};
pub use processes::FakeProcesses;

use zbm_platform::exec::Input;
use zbm_platform::{Context, System};

use crate::FakeRunner;

/// One fake of each system interface.
pub struct FakeSystem {
    /// External programs.
    pub runner: FakeRunner,
    /// Files: an empty root, `/dev/kmsg`, and nothing else.
    pub files: FakeFiles,
    /// The clock, shared with the console.
    pub clock: FakeClock,
    /// The console, whose timed-out key presses advance the clock.
    pub console: FakeConsole,
    /// Processes; this one has the ID 4242.
    pub processes: FakeProcesses,
    /// Environment variables: none until the test sets them.
    pub environment: FakeEnvironment,
    /// Kernels: none loaded until the test accepts them, so `kexec` runs.
    pub kernel: FakeKernelLoader,
    /// HID devices: no YubiKey until the test plugs one in.
    pub hid: FakeHid,
}

impl Default for FakeSystem {
    fn default() -> FakeSystem {
        FakeSystem::new()
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

impl FakeSystem {
    /// A new fake system.
    pub fn new() -> FakeSystem {
        let clock = FakeClock::new();
        let files = FakeFiles::new();
        files.device("/dev/kmsg");
        FakeSystem {
            runner: FakeRunner::new(),
            files,
            console: FakeConsole::new().with_clock(clock.clone()),
            clock,
            processes: FakeProcesses::default(),
            environment: FakeEnvironment::new(),
            kernel: FakeKernelLoader::default(),
            hid: FakeHid::default(),
        }
    }

    /// The fakes as a [`System`].
    pub fn system(&self) -> System<'_> {
        System {
            runner: &self.runner,
            files: &self.files,
            clock: &self.clock,
            console: &self.console,
            processes: &self.processes,
            environment: &self.environment,
            kernel: &self.kernel,
            hid: &self.hid,
        }
    }

    /// The fakes as a [`Context`]: the [`System`] without its console.
    pub fn context(&self) -> Context<'_> {
        Context::new(self.system())
    }

    /// The records written to `/dev/kmsg`.
    pub fn kmsg(&self) -> String {
        self.files.text("/dev/kmsg").unwrap_or_default()
    }

    /// Fails the test if `secret` appears where secrets must never go: in the
    /// program, arguments or environment of a command, in a command's input
    /// given as plain bytes rather than as a secret, in a file written or a
    /// path created, or on the console.
    pub fn assert_no_leak(&self, secret: &[u8]) {
        assert!(!secret.is_empty(), "an empty secret cannot be searched for");
        let in_os = |text: &OsStr| contains(text.as_encoded_bytes(), secret);
        for command in self.runner.calls() {
            let line = command.display();
            assert!(
                !in_os(command.program()) && !command.get_args().iter().any(|arg| in_os(arg)),
                "a secret is in the arguments of `{line}`"
            );
            for (key, value) in command.get_env() {
                assert!(
                    !in_os(key) && !value.as_deref().is_some_and(in_os),
                    "a secret is in the environment of `{line}`"
                );
            }
            if let Input::Bytes(bytes) = command.get_stdin() {
                assert!(
                    !contains(bytes, secret),
                    "a secret is passed to `{line}` as plain bytes instead of Input::Secret"
                );
            }
        }
        let in_path = |path: &Path| in_os(path.as_os_str());
        // What each file was given, all its writes in order: a secret
        // written in two parts, as a log written line by line may hold it,
        // is in no single write.
        let mut written: std::collections::BTreeMap<&Path, Vec<u8>> =
            std::collections::BTreeMap::new();
        let operations = self.files.operations();
        for operation in &operations {
            let leaked = match operation {
                FileOperation::Write(path, contents) | FileOperation::Append(path, contents) => {
                    written.entry(path).or_default().extend_from_slice(contents);
                    in_path(path)
                }
                FileOperation::CreateDir(path)
                | FileOperation::Remove(path)
                | FileOperation::SetMode(path, _) => in_path(path),
                FileOperation::Rename(from, to) => in_path(from) || in_path(to),
            };
            assert!(!leaked, "a secret is in a file operation: {operation:?}");
        }
        for (path, contents) in written {
            assert!(
                !contains(&contents, secret),
                "a secret was written to {}",
                path.display()
            );
        }
        assert!(
            !contains(&self.console.output(), secret),
            "a secret was written to the console"
        );
    }
}

#[cfg(test)]
mod tests;
