//! A software TPM 2.0 whose state persists in a directory.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::disks::run;
use crate::error::{Error, Result};

/// A software TPM, started by [`Swtpm::start`] for one machine.
#[derive(Debug)]
pub struct Swtpm {
    state: PathBuf,
    socket: PathBuf,
    pid: Option<u32>,
}

/// The TCTI of a software TPM listening on `port`, and on the next port for
/// its control channel, for the TPM tools.
pub fn tcti(port: u16) -> String {
    format!("swtpm:host=127.0.0.1,port={port}")
}

/// How many times [`start_tcp`] chooses other ports when `swtpm` finds the
/// ones it chose taken.
const TCP_TRIES: usize = 3;

/// How long a stopped `swtpm` may take to exit, in tenths of a second.
const EXIT_TENTHS: u32 = 50;

/// How many ports [`free_port`] tries for one whose next port is free too.
const PORT_TRIES: usize = 20;

/// A free TCP port on the loopback interface, chosen by the kernel so that
/// no other run or software TPM has it, whose next port is free too: the
/// TPM tools expect the control channel there.
fn free_port() -> Result<u16> {
    for _ in 0..PORT_TRIES {
        let first =
            TcpListener::bind(("127.0.0.1", 0)).map_err(Error::io("choosing a TCP port"))?;
        let port = first
            .local_addr()
            .map_err(Error::io("choosing a TCP port"))?
            .port();
        let next = port
            .checked_add(1)
            .map(|next| TcpListener::bind(("127.0.0.1", next)));
        if next.is_some_and(|next| next.is_ok()) {
            return Ok(port);
        }
    }
    Err(Error::Setup(
        "no two free TCP ports in a row for the software TPM".into(),
    ))
}

/// Whether `error` is `swtpm` failing because a port it was given is taken.
fn is_port_taken(error: &Error) -> bool {
    matches!(error, Error::Command { output, .. } if output.contains("Address already in use"))
}

/// Starts `swtpm` on a free TCP port, and the next for its control channel,
/// with the state in `state` and its process ID in `pid_file`; returns the
/// port, for [`tcti`].
///
/// # Errors
///
/// [`Error::Command`] when `swtpm` fails, [`Error::Io`] when no port is
/// free.
pub fn start_tcp(state: &Path, pid_file: &Path, flags: &str) -> Result<u16> {
    start_tcp_with(state, pid_file, flags, run)
}

/// [`start_tcp`], with `run` running `swtpm`.
fn start_tcp_with(
    state: &Path,
    pid_file: &Path,
    flags: &str,
    mut run: impl FnMut(&mut Command) -> Result<String>,
) -> Result<u16> {
    let mut tries = 1;
    loop {
        let port = free_port()?;
        let mut command = Command::new("swtpm");
        command
            .args(["socket", "--tpm2", "--tpmstate"])
            .arg(format!("dir={}", state.display()))
            .arg("--server")
            .arg(format!("type=tcp,port={port}"))
            .arg("--ctrl")
            .arg(format!("type=tcp,port={}", port + 1))
            .args(["--flags", flags, "-d", "--pid"])
            .arg(format!("file={}", pid_file.display()));
        match run(&mut command) {
            Ok(_) => return Ok(port),
            Err(error) if tries < TCP_TRIES && is_port_taken(&error) => tries += 1,
            Err(error) => return Err(error),
        }
    }
}

/// Stops the process whose ID `pid_file` holds, and waits for it to exit,
/// so that its ports and its state are free for the next software TPM.
pub fn stop_pid_file(pid_file: &Path) {
    if let Some(pid) = fs::read_to_string(pid_file)
        .ok()
        .and_then(|pid| pid.trim().parse().ok())
    {
        stop(pid);
    }
}

/// Stops process `pid` and waits up to five seconds for it to exit.
fn stop(pid: u32) {
    Command::new("kill").arg(pid.to_string()).output().ok();
    for _ in 0..EXIT_TENTHS {
        if !is_running(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Whether process `pid` runs: it exists and has not exited.
fn is_running(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        let state = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next());
        state.is_some_and(|state| !matches!(state, "Z" | "X"))
    })
}

impl Swtpm {
    /// A software TPM with its state in `state`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the directory cannot be made.
    pub fn new(state: &Path) -> Result<Swtpm> {
        fs::create_dir_all(state).map_err(Error::io(format!("creating {}", state.display())))?;
        let socket =
            std::env::temp_dir().join(format!("zbmtest-swtpm-{}.sock", std::process::id()));
        Ok(Swtpm {
            state: state.to_owned(),
            socket,
            pid: None,
        })
    }

    /// Starts it for a machine; returns its control socket.
    ///
    /// # Errors
    ///
    /// [`Error::Command`] when `swtpm` fails.
    pub fn start(&mut self) -> Result<PathBuf> {
        fs::remove_file(&self.socket).ok();
        let pid_file = self.socket.with_extension("sock.pid");
        run(Command::new("swtpm")
            .args(["socket", "--tpm2", "--tpmstate"])
            .arg(format!("dir={}", self.state.display()))
            .arg("--ctrl")
            .arg(format!("type=unixio,path={}", self.socket.display()))
            .args(["-d", "--pid"])
            .arg(format!("file={}", pid_file.display()))
            .args(["--flags", "startup-clear"]))?;
        thread::sleep(Duration::from_secs(1));
        self.pid = fs::read_to_string(&pid_file)
            .ok()
            .and_then(|pid| pid.trim().parse().ok());
        Ok(self.socket.clone())
    }

    /// Stops it.
    pub fn stop(&mut self) {
        if let Some(pid) = self.pid.take() {
            stop(pid);
        }
    }

    /// Resets the dictionary-attack lockout counter (the owner
    /// authorization is empty).
    ///
    /// # Errors
    ///
    /// [`Error::Command`] when `swtpm` or `tpm2_dictionarylockout` fails.
    pub fn clear_lockout(&self) -> Result<()> {
        let pid_file = self.socket.with_extension("sock.clr");
        let port = start_tcp(&self.state, &pid_file, "startup-clear")?;
        thread::sleep(Duration::from_secs(1));
        let result = run(Command::new("tpm2_dictionarylockout").args(["-T", &tcti(port), "-c"]));
        stop_pid_file(&pid_file);
        result.map(drop)
    }
}

impl Drop for Swtpm {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests;
