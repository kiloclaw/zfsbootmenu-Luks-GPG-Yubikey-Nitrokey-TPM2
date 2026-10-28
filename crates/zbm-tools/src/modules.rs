//! Kernel modules: `modprobe`, `modinfo` and `insmod`.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::time::Duration;

use zbm_platform::{Command, Input, Output, Runner, Stream};

/// The longest loading a module may take: a driver waits for its device,
/// and up to a minute for each firmware file it asks for.
const LOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// The module tools, run through a [`Runner`].
#[derive(Clone, Copy)]
pub struct Modules<'a> {
    runner: &'a dyn Runner,
}

impl<'a> Modules<'a> {
    /// The tools, run through `runner`.
    pub fn new(runner: &'a dyn Runner) -> Modules<'a> {
        Modules { runner }
    }

    /// Loads `module` if the kernel has it, quietly (`modprobe -q MODULE`);
    /// whether it loaded is for the caller to find out.
    pub fn probe_quietly(&self, module: &str) {
        let command = Command::new("modprobe")
            .args(["-q", module])
            .timeout(LOAD_TIMEOUT)
            .stdin(Input::Null)
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        let _ = self.runner.run(&command);
    }

    /// Loads `module` and what it needs (`modprobe MODULE`), with its error
    /// messages in the output.
    ///
    /// # Errors
    ///
    /// The error that kept `modprobe` from starting.
    pub fn load(&self, module: &str) -> io::Result<Output> {
        self.runner.run(
            &Command::new("modprobe")
                .arg(module)
                .timeout(LOAD_TIMEOUT)
                .merge_stderr(),
        )
    }

    /// The file of `module` (`modinfo -F filename MODULE`).
    ///
    /// # Errors
    ///
    /// The error that kept `modinfo` from starting.
    pub fn filename(&self, module: &str) -> io::Result<Output> {
        self.runner
            .run(&Command::new("modinfo").args(["-F", "filename", module]))
    }

    /// Inserts the module in `file` with `parameters` (`insmod FILE ...`),
    /// with its error messages in the output.
    ///
    /// # Errors
    ///
    /// The error that kept `insmod` from starting.
    pub fn insert(&self, file: &str, parameters: &[&str]) -> io::Result<Output> {
        let command = Command::new("insmod")
            .arg(file)
            .timeout(LOAD_TIMEOUT)
            .args(parameters.iter().copied())
            .merge_stderr();
        self.runner.run(&command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;

    #[test]
    fn modules_are_probed_loaded_found_and_inserted() {
        let runner = FakeRunner::new();
        runner.respond("modinfo -F filename spl", 0, "/lib/modules/spl.ko\n");
        let modules = Modules::new(&runner);

        modules.probe_quietly("tpm_crb");
        modules.load("zfs").unwrap();
        let file = modules.filename("spl").unwrap().stdout_text();
        modules.insert(file.trim_end(), &["spl_hostid=0"]).unwrap();

        assert_eq!(
            runner.lines(),
            [
                "modprobe -q tpm_crb",
                "modprobe zfs",
                "modinfo -F filename spl",
                "insmod /lib/modules/spl.ko spl_hostid=0",
            ]
        );
        let calls = runner.calls();
        assert_eq!(calls[0].get_stdout(), Stream::Null);
        assert!(calls[1].merges_stderr() && calls[3].merges_stderr());
    }
}
