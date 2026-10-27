//! The recovery shell on a terminal, as a serial line gives it: brush with
//! its basic line editor on a pseudo-terminal, sent lines the way the VM
//! tests type them.
//!
//! - A line sent in one burst longer than the terminal library's read
//!   buffer (1 KiB) reaches the shell whole: crossterm, which both of
//!   brush's line editors read keys with, left the rest unread until more
//!   input came (`patches/crossterm-edge-triggered-input.patch`), and the
//!   shell waited for an Enter it had already been sent.
//! - A line that ends with a backslash goes on on the next line, as in
//!   bash: brush ran it alone and each following line as a command of its
//!   own (`patches/brush-interactive-line-continuation.patch`).

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "the test runs the shell on a pseudo-terminal, waits for it, and fails loudly \
              when it cannot"
)]

use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

/// How long the shell may take to answer.
const PATIENCE: Duration = Duration::from_secs(20);

/// The pause between the lines of one command, as the VM tests leave.
const BETWEEN_LINES: Duration = Duration::from_millis(200);

/// A pseudo-terminal: its controlling side and the terminal side.
fn pty() -> (File, File) {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let path = std::str::from_utf8(name.as_bytes()).unwrap().to_owned();
    let slave = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    (File::from(master), slave)
}

/// brush with its basic line editor on a pseudo-terminal, and what it
/// writes there.
struct Shell {
    child: Child,
    master: File,
    output: mpsc::Receiver<Vec<u8>>,
}

impl Shell {
    /// Starts the shell and waits until it runs commands.
    fn start() -> Shell {
        let (master, slave) = pty();
        let child = Command::new(env!("CARGO_BIN_EXE_brush"))
            .args(["--input-backend", "basic", "--norc", "--noprofile", "-i"])
            .env_clear()
            .env("PATH", "/nonexistent")
            .env("TERM", "vt100")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .process_group(0)
            .spawn()
            .unwrap();
        let mut reader = master.try_clone().unwrap();
        let (sender, output) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(count @ 1..) = reader.read(&mut buffer) {
                let read = buffer.get(..count).unwrap_or_default().to_vec();
                if sender.send(read).is_err() {
                    break;
                }
            }
        });
        let mut shell = Shell {
            child,
            master,
            output,
        };
        shell.send(b"echo READY-$((20+1))\n");
        assert!(shell.wait_for("READY-21"), "the shell did not start");
        shell
    }

    /// Types `bytes` at once.
    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
    }

    /// Waits until what the shell wrote holds `marker`; returns whether it
    /// came. The markers are arithmetic in the commands, so the shell's echo
    /// of what was typed never holds them.
    fn wait_for(&self, marker: &str) -> bool {
        let deadline = Instant::now() + PATIENCE;
        let mut seen = Vec::new();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.output.recv_timeout(left) {
                Ok(bytes) => seen.extend(bytes),
                Err(_) => break,
            }
            if String::from_utf8_lossy(&seen).contains(marker) {
                return true;
            }
        }
        false
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_burst_longer_than_the_read_buffer_reaches_the_basic_line_editor() {
    let mut shell = Shell::start();

    // About 3 KiB, as the VM tests type: 40 commands, then the marker.
    let words: Vec<String> = (0..40)
        .map(|n| format!("true {n:04}-{}", "x".repeat(60)))
        .collect();
    let line = format!("{}; echo DONE-$((40+2))\n", words.join("; "));
    assert!(line.len() > 2048);
    shell.send(line.as_bytes());

    assert!(
        shell.wait_for("DONE-42"),
        "the shell did not run a line of {} bytes",
        line.len()
    );
}

#[test]
fn a_line_that_ends_with_a_backslash_goes_on_on_the_next_one() {
    let mut shell = Shell::start();

    // The documentation's `zpool create` commands, one vdev a line.
    for line in [
        "echo JOINED-$((6*7)) \\\n",
        "  first \\\n",
        "  second; echo RC=$?\n",
    ] {
        shell.send(line.as_bytes());
        thread::sleep(BETWEEN_LINES);
    }

    assert!(
        shell.wait_for("JOINED-42 first second"),
        "the three lines did not run as one command"
    );
}
