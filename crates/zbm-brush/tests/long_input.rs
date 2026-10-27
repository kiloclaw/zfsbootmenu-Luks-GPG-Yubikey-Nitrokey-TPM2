//! A line typed or pasted in one burst longer than the terminal library's
//! read buffer (1 KiB) reaches the shell whole: crossterm, which both of
//! brush's line editors read keys with, left the rest unread until more
//! input came (`patches/crossterm-edge-triggered-input.patch`), and the
//! shell waited for an Enter it had already been sent.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "the test runs the shell on a pseudo-terminal, waits for it, and fails loudly \
              when it cannot"
)]

use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::process::CommandExt as _;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

/// How long the shell may take to answer.
const PATIENCE: Duration = Duration::from_secs(20);

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

/// Waits until what the shell wrote holds `marker`; returns whether it came.
fn wait_for(output: &mpsc::Receiver<Vec<u8>>, marker: &str) -> bool {
    let deadline = Instant::now() + PATIENCE;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match output.recv_timeout(left) {
            Ok(bytes) => seen.extend(bytes),
            Err(_) => break,
        }
        if String::from_utf8_lossy(&seen).contains(marker) {
            return true;
        }
    }
    false
}

#[test]
fn a_burst_longer_than_the_read_buffer_reaches_the_basic_line_editor() {
    let (mut master, slave) = pty();
    let mut shell = Command::new(env!("CARGO_BIN_EXE_brush"))
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
            if sender.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    master.write_all(b"echo READY-$((20+1))\n").unwrap();
    assert!(wait_for(&output, "READY-21"), "the shell did not start");

    // About 3 KiB, as the VM tests type: 40 commands, then the marker,
    // which the line holds only as arithmetic.
    let words: Vec<String> = (0..40)
        .map(|n| format!("true {n:04}-{}", "x".repeat(60)))
        .collect();
    let line = format!("{}; echo DONE-$((40+2))\n", words.join("; "));
    assert!(line.len() > 2048);
    master.write_all(line.as_bytes()).unwrap();
    let done = wait_for(&output, "DONE-42");

    let _ = shell.kill();
    let _ = shell.wait();
    assert!(done, "the shell did not run a line of {} bytes", line.len());
}
