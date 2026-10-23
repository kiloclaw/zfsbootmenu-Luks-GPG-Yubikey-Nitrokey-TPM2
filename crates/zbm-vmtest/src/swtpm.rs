//! A software TPM 2.0 whose state persists in a directory.

use std::fs;
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

/// The TCTI of a software TPM listening on `port`, for the TPM tools.
pub fn tcti(port: u16) -> String {
    format!("swtpm:host=127.0.0.1,port={port}")
}

/// A TCP port for a software TPM that the host's tools talk to; `base` and
/// `span` keep different uses apart, the process ID different runs.
pub fn port(base: u16, span: u32) -> u16 {
    base + u16::try_from(std::process::id() % span).unwrap_or_default()
}

/// Starts `swtpm` on TCP `port` (and `port + 1` for control) with the state
/// in `state`; returns its process ID file.
///
/// # Errors
///
/// [`Error::Command`] when `swtpm` fails.
pub fn start_tcp(state: &Path, port: u16, pid_file: &Path, flags: &str) -> Result<()> {
    run(Command::new("swtpm")
        .args(["socket", "--tpm2", "--tpmstate"])
        .arg(format!("dir={}", state.display()))
        .arg("--server")
        .arg(format!("type=tcp,port={port}"))
        .arg("--ctrl")
        .arg(format!("type=tcp,port={}", port + 1))
        .args(["--flags", flags, "-d", "--pid"])
        .arg(format!("file={}", pid_file.display())))?;
    Ok(())
}

/// Stops the process whose ID `pid_file` holds.
pub fn stop_pid_file(pid_file: &Path) {
    if let Ok(pid) = fs::read_to_string(pid_file) {
        Command::new("kill").arg(pid.trim()).output().ok();
    }
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
            Command::new("kill").arg(pid.to_string()).output().ok();
        }
    }

    /// Resets the dictionary-attack lockout counter (the owner
    /// authorization is empty).
    ///
    /// # Errors
    ///
    /// [`Error::Command`] when `swtpm` or `tpm2_dictionarylockout` fails.
    pub fn clear_lockout(&self) -> Result<()> {
        let port = port(2400, 1000);
        let pid_file = self.socket.with_extension("sock.clr");
        start_tcp(&self.state, port, &pid_file, "startup-clear")?;
        thread::sleep(Duration::from_secs(1));
        let result = run(Command::new("tpm2_dictionarylockout").args(["-T", &tcti(port), "-c"]));
        stop_pid_file(&pid_file);
        thread::sleep(Duration::from_secs(1));
        result.map(drop)
    }
}

impl Drop for Swtpm {
    fn drop(&mut self) {
        self.stop();
    }
}
