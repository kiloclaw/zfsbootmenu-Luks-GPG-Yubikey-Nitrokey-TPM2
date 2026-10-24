//! Helpers for the harness's own tests: a guest that plays a script on a
//! console, and tests in a scratch work directory.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::thread::{self, JoinHandle};

use crate::console::{Console, Size};
use crate::record::Test;
use crate::settings::Settings;
use crate::vm::Vm;

/// One step of a guest's script.
#[derive(Clone, Copy, Debug)]
pub enum Step {
    /// The guest prints this.
    Say(&'static str),
    /// The guest reads a line, which must be this.
    Hear(&'static str),
    /// The guest reads key presses, as many bytes as these, which must be
    /// these.
    Keys(&'static str),
}

/// A guest playing a script on the other end of a console.
pub struct Guest {
    thread: JoinHandle<Vec<String>>,
}

impl Guest {
    /// Plays `script` on `stream`, then closes it.
    pub fn play(stream: UnixStream, script: Vec<Step>) -> Guest {
        let thread = thread::spawn(move || {
            let mut mismatches = Vec::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            for step in script {
                match step {
                    Step::Say(text) => writer.write_all(text.as_bytes()).unwrap(),
                    Step::Hear(expected) => {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if line.trim_end_matches('\n') != expected {
                            mismatches.push(format!("heard {line:?}, expected {expected:?}"));
                        }
                    }
                    Step::Keys(expected) => {
                        let mut keys = vec![0; expected.len()];
                        reader.read_exact(&mut keys).unwrap();
                        if keys != expected.as_bytes() {
                            let keys = String::from_utf8_lossy(&keys);
                            mismatches.push(format!("pressed {keys:?}, expected {expected:?}"));
                        }
                    }
                }
            }
            mismatches
        });
        Guest { thread }
    }

    /// Waits for the end of the script; returns what was heard wrongly.
    pub fn finish(self) -> Vec<String> {
        self.thread.join().unwrap()
    }
}

/// A machine that is a console with `script` played on its other end.
pub fn scripted<'t>(test: &'t Test, script: Vec<Step>) -> (Vm<'t>, Guest) {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let reader = ours.try_clone().unwrap();
    let log = std::fs::File::create(test.dir().join("serial-scripted.log")).unwrap();
    let console = Console::attach(reader, ours, log, Size::default());
    (
        Vm::on_console(test, "scripted", console),
        Guest::play(theirs, script),
    )
}

/// Settings with a work directory of their own under the temporary
/// directory; [`cleanup`] removes it.
pub fn settings(name: &str) -> Settings {
    let work = std::env::temp_dir().join(format!("zbm-vmtest-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&work).ok();
    let mut settings = Settings::from_vars(|_| None, false, false);
    settings.work = work;
    settings
}

/// Removes the work directory of `settings`.
pub fn cleanup(settings: &Settings) {
    std::fs::remove_dir_all(&settings.work).ok();
}

/// The check lines of a test's `steps.log`, without the time.
pub fn checks(test: &Test) -> Vec<String> {
    let steps = std::fs::read_to_string(test.dir().join("steps.log")).unwrap();
    steps
        .lines()
        .map(|line| line.get(11..).unwrap_or_default())
        .filter(|line| line.starts_with("ok   - ") || line.starts_with("FAIL - "))
        .map(str::to_owned)
        .collect()
}
