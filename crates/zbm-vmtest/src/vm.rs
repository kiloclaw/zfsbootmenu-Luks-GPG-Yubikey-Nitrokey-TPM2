//! A QEMU virtual machine driven through its serial console.

use std::fs::{self, File};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::console::{Console, Size};
use crate::error::{Error, Result};
use crate::qemu::{self, Machine, Sockets};
use crate::record::Test;
use crate::settings::PW;
use crate::swtpm::Swtpm;

/// The columns of the recovery shell's terminal, so that what commands
/// print does not wrap.
const SHELL_COLUMNS: u16 = 250;

/// How long QEMU may take to open its serial console socket.
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// The prompts of a command in the recovery shell that the harness answers
/// as a user would, then the line that ends the command.
pub const SHELL_PROMPTS: [&str; 6] = [
    r"Type 'yes' in capital letters\): ",
    r"Enter passphrase for [^\n]*: ",
    r"Verify passphrase: ",
    r"Enter any existing passphrase: ",
    r"Enter new passphrase for key slot: ",
    r"RC=(\d+)\r*\n",
];

/// How to run a command in the recovery shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sh {
    /// How long it may take, in seconds.
    pub timeout: u64,
    /// Whether to record its output.
    pub show: bool,
    /// Whether to leave it out of the steps.
    pub quiet: bool,
    /// The passphrase that answers `cryptsetup`'s prompts.
    pub pw: &'static str,
}

impl Default for Sh {
    fn default() -> Sh {
        Sh {
            timeout: 1800,
            show: false,
            quiet: false,
            pw: PW,
        }
    }
}

impl Sh {
    /// Records the output.
    #[must_use]
    pub fn show(self) -> Sh {
        Sh { show: true, ..self }
    }

    /// Leaves the command out of the steps.
    #[must_use]
    pub fn quiet(self) -> Sh {
        Sh {
            quiet: true,
            ..self
        }
    }

    /// Allows `seconds`.
    #[must_use]
    pub fn timeout(self, seconds: u64) -> Sh {
        Sh {
            timeout: seconds,
            ..self
        }
    }
}

/// A running virtual machine.
pub struct Vm<'t> {
    test: &'t Test,
    label: String,
    /// QEMU; `None` for a console without a machine ([`Vm::on_console`]).
    child: Option<Child>,
    console: Console,
    sockets: Sockets,
    swtpm: Option<Swtpm>,
}

impl<'t> Vm<'t> {
    /// Starts `machine` for `test`; its console is logged in
    /// `serial-<label>.log`, QEMU's own messages in `qemu-<label>.log`.
    ///
    /// # Errors
    ///
    /// When QEMU or the software TPM cannot be started.
    pub fn start(test: &'t Test, label: &str, machine: &Machine) -> Result<Vm<'t>> {
        let base = std::env::temp_dir().join(format!("zbmtest-{}-{label}", std::process::id()));
        let mut sockets = Sockets {
            serial: base.with_extension("serial.sock"),
            monitor: base.with_extension("mon.sock"),
            tpm: None,
        };
        fs::remove_file(&sockets.serial).ok();
        fs::remove_file(&sockets.monitor).ok();
        let mut swtpm = machine.tpm.as_deref().map(Swtpm::new).transpose()?;
        if let Some(swtpm) = &mut swtpm {
            sockets.tpm = Some(swtpm.start()?);
        }
        let args = qemu::args(machine, &test.settings().accel, &sockets);
        test.step(&format!("VM {label}: starting"));
        let qemu_log = test.dir().join(format!("qemu-{label}.log"));
        let log = File::create(&qemu_log).map_err(Error::io("creating the QEMU log"))?;
        let mut child = Command::new("qemu-system-x86_64")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(Error::io("opening the QEMU log"))?)
            .stderr(log)
            .spawn()
            .map_err(Error::io("starting qemu-system-x86_64"))?;
        let stream = match connect(&sockets.serial, &mut child) {
            Ok(stream) => stream,
            Err(error) => {
                child.kill().ok();
                child.wait().ok();
                let output = fs::read_to_string(&qemu_log).unwrap_or_default();
                return Err(Error::Setup(format!(
                    "{error}; QEMU said: {}",
                    output.trim()
                )));
            }
        };
        let serial_log = File::create(test.dir().join(format!("serial-{label}.log")))
            .map_err(Error::io("creating the serial log"))?;
        let reader = stream
            .try_clone()
            .map_err(Error::io("opening the serial console"))?;
        let console = Console::attach(reader, stream, serial_log, Size::default());
        Ok(Vm {
            test,
            label: label.to_owned(),
            child: Some(child),
            console,
            sockets,
            swtpm,
        })
    }

    /// A machine that is only a console, without QEMU: for tests of the
    /// harness, which play the guest's side.
    pub fn on_console(test: &'t Test, label: &str, console: Console) -> Vm<'t> {
        let base = std::env::temp_dir().join(format!("zbmtest-{}-{label}", std::process::id()));
        Vm {
            test,
            label: label.to_owned(),
            child: None,
            console,
            sockets: Sockets {
                serial: base.with_extension("serial.sock"),
                monitor: base.with_extension("mon.sock"),
                tpm: None,
            },
            swtpm: None,
        }
    }

    /// The test this machine belongs to.
    pub fn test(&self) -> &'t Test {
        self.test
    }

    /// The machine's label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The log of its serial console.
    pub fn serial_log(&self) -> PathBuf {
        self.test.dir().join(format!("serial-{}.log", self.label))
    }

    /// The console, for what [`Vm`] does not wrap.
    pub fn console(&mut self) -> &mut Console {
        &mut self.console
    }

    /// Waits up to `seconds` for one of `patterns`; returns the index of the
    /// one that matched.
    ///
    /// # Errors
    ///
    /// See [`Console::expect`].
    pub fn expect(&mut self, patterns: &[&str], seconds: u64) -> Result<usize> {
        self.console.expect(patterns, Duration::from_secs(seconds))
    }

    /// Waits up to `seconds` for `pattern`.
    ///
    /// # Errors
    ///
    /// See [`Console::expect`].
    pub fn expect1(&mut self, pattern: &str, seconds: u64) -> Result<()> {
        self.expect(&[pattern], seconds).map(drop)
    }

    /// Waits up to `seconds` until the screen shows something other than
    /// `before`: a dialog acted on was typed into, closed or asked again.
    ///
    /// # Errors
    ///
    /// See [`Console::wait_for_change`].
    pub fn wait_for_change(&mut self, before: &str, seconds: u64) -> Result<()> {
        self.console
            .wait_for_change(before, Duration::from_secs(seconds))
    }

    /// Waits up to `seconds` for the machine to stop.
    ///
    /// # Errors
    ///
    /// [`Error::Timeout`] when it is still running.
    pub fn expect_eof(&mut self, seconds: u64) -> Result<()> {
        self.console.expect_eof(Duration::from_secs(seconds))
    }

    /// The text before the last match, without carriage returns.
    pub fn before(&self) -> String {
        self.console.before()
    }

    /// Group `n` of the last match.
    pub fn group(&self, n: usize) -> String {
        self.console.group(n).to_owned()
    }

    /// Sends `text` as typed.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the console is closed.
    pub fn send(&mut self, text: &str) -> Result<()> {
        self.console.send(text)
    }

    /// Sends `text` and a newline.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the console is closed.
    pub fn sendline(&mut self, text: &str) -> Result<()> {
        self.console.sendline(text)
    }

    /// Runs a monitor command; returns its reply.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the monitor cannot be reached.
    pub fn monitor(&self, command: &str) -> Result<String> {
        qemu::monitor(&self.sockets.monitor, command)
    }

    /// Presses `key` (a QEMU key name, such as `ctrl-alt-f2`) for 50 ms.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the monitor cannot be reached, and
    /// [`Error::Command`] when QEMU refused the key (it answers only then).
    pub fn send_key(&self, key: &str) -> Result<()> {
        let command = format!("sendkey {key} 50");
        let reply = self.monitor(&command)?;
        let answer: Vec<&str> = reply
            .lines()
            .filter(|line| !(line.starts_with("QEMU ") && line.contains(" monitor")))
            .collect();
        if answer.is_empty() {
            return Ok(());
        }
        Err(Error::Command {
            command,
            status: "refused".to_owned(),
            output: answer.join("\n"),
        })
    }

    /// Plugs in the disk `path` with the serial `serial`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the monitor cannot be reached.
    pub fn hotplug_disk(&self, path: &Path, serial: &str) -> Result<()> {
        self.test.step(&format!("hot-plugging disk {serial}"));
        let drive = format!("file={},if=none,id=drv-{serial},format=raw", path.display());
        let device = format!("virtio-blk-pci,drive=drv-{serial},serial={serial},id=dev-{serial}");
        for command in [
            format!("drive_add 0 {drive}"),
            format!("device_add {device}"),
        ] {
            let reply = self.monitor(&command)?;
            if !reply.is_empty() && reply != "OK" {
                let name = command.split(' ').next().unwrap_or_default();
                self.test.show(&format!("monitor: {name}: {reply}"));
            }
        }
        Ok(())
    }

    /// Inserts the USB stick `path` as the drive `id`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the monitor cannot be reached.
    pub fn hotplug_usb(&self, path: &Path, id: &str) -> Result<()> {
        self.test.step(&format!("inserting USB stick {id}"));
        self.monitor(&format!(
            "drive_add 0 file={},if=none,id={id},format=raw",
            path.display()
        ))?;
        self.monitor(&format!(
            "device_add usb-storage,bus=xhci.0,drive={id},id=dev-{id}"
        ))?;
        Ok(())
    }

    /// From the import retry loop, opens the recovery shell and prepares it:
    /// Escape at the retry prompt on the console, or at the question of the
    /// start-up view (Rust images with the menu), once it is drawn.
    ///
    /// # Errors
    ///
    /// When the loop or the shell does not appear.
    pub fn recovery_shell(&mut self) -> Result<()> {
        let patterns = [crate::boot::prompt::RETRY, crate::boot::dialogs::RETRY];
        if self.expect(&patterns, 1800)? == 1 {
            self.test
                .step("the start-up view asks to import again -> Esc");
        }
        self.send("\x1b")?;
        self.shell_ready()
    }

    /// Prepares the recovery shell for commands: no echo, wide lines, a
    /// prompt the harness knows.
    ///
    /// # Errors
    ///
    /// When the shell does not appear.
    pub fn shell_ready(&mut self) -> Result<()> {
        // A shell prepared before shows its own prompt.
        self.expect(&["zfsbootmenu", "zbm# "], 600)?;
        thread::sleep(Duration::from_secs(3));
        self.sendline(&format!(
            "stty -echo cols {SHELL_COLUMNS}; export PS1='zbm# ' PS2=''"
        ))?;
        // The terminal stays that wide after the shell: a menu opened from
        // it, or after it, draws in 250 columns.
        self.console.set_columns(SHELL_COLUMNS);
        thread::sleep(Duration::from_secs(2));
        self.sh_with("true", Sh::default().quiet()).map(drop)
    }

    /// Runs a shell command; returns its exit status and output.
    ///
    /// # Errors
    ///
    /// When the command does not end in time.
    pub fn sh(&mut self, command: &str) -> Result<(i32, String)> {
        self.sh_with(command, Sh::default())
    }

    /// Runs a shell command and records its output.
    ///
    /// # Errors
    ///
    /// When the command does not end in time.
    pub fn sh_show(&mut self, command: &str) -> Result<(i32, String)> {
        self.sh_with(command, Sh::default().show())
    }

    /// Runs a shell command as `options` say, answering `cryptsetup`'s
    /// prompts as a user would; returns its exit status and output.
    ///
    /// # Errors
    ///
    /// When the command does not end in time.
    pub fn sh_with(&mut self, command: &str, options: Sh) -> Result<(i32, String)> {
        let mut lines: Vec<String> = command.split('\n').map(str::to_owned).collect();
        if let Some(last) = lines.last_mut() {
            last.push_str("; echo RC=$?");
        }
        for line in &lines {
            self.sendline(line)?;
            if lines.len() > 1 {
                thread::sleep(Duration::from_millis(200));
            }
        }
        loop {
            match self.expect(&SHELL_PROMPTS, options.timeout)? {
                0 => self.sendline("YES")?,
                1..=4 => self.sendline(options.pw)?,
                _ => break,
            }
        }
        let rc: i32 = self.console.group(1).parse().unwrap_or(-1);
        let out = without_echo(&shell_output(&self.before()), &lines);
        if !options.quiet {
            let first: String = command
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(150)
                .collect();
            let more = if lines.len() > 1 { " ..." } else { "" };
            self.test.step(&format!("$ {first}{more} -> {rc}"));
        }
        if options.show {
            self.test.show(&out);
        }
        Ok((rc, out))
    }

    /// Runs a setup command that must succeed; returns its output.
    ///
    /// # Errors
    ///
    /// [`Error::Setup`] when it fails, after recording a failed check.
    pub fn must(&mut self, command: &str) -> Result<String> {
        self.must_with(command, Sh::default())
    }

    /// [`Vm::must`] with options.
    ///
    /// # Errors
    ///
    /// [`Error::Setup`] when it fails, after recording a failed check.
    pub fn must_with(&mut self, command: &str, options: Sh) -> Result<String> {
        let (rc, out) = self.sh_with(command, options)?;
        if rc != 0 {
            self.test.show(&out);
            let short: String = command.chars().take(120).collect();
            self.test.check(false, &format!("command failed: {short}"));
            return Err(Error::Setup(format!(
                "setup command failed (rc={rc}): {command}"
            )));
        }
        Ok(out)
    }

    /// Defines the shell functions of `functions.sh` in the shell.
    ///
    /// # Errors
    ///
    /// When the file cannot be read or the shell does not answer.
    pub fn send_functions(&mut self) -> Result<()> {
        let path = self.test.settings().guest_files().join("functions.sh");
        let text =
            fs::read_to_string(&path).map_err(Error::io(format!("reading {}", path.display())))?;
        for line in text.lines() {
            if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
                self.sendline(line)?;
                thread::sleep(Duration::from_millis(50));
            }
        }
        thread::sleep(Duration::from_secs(1));
        self.sh_with("type mkluks >/dev/null", Sh::default().quiet())
            .map(drop)
    }

    /// Powers the machine off through the kernel's SysRq, or stops it.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the console is closed.
    pub fn poweroff(&mut self) -> Result<()> {
        self.sendline("echo o > /proc/sysrq-trigger")?;
        if self.expect_eof(180).is_err() {
            self.kill();
        }
        if let Some(child) = &mut self.child {
            child.wait().ok();
        }
        self.stop_tpm();
        Ok(())
    }

    /// Stops the machine.
    pub fn kill(&mut self) {
        if let Some(child) = &mut self.child {
            if matches!(child.try_wait(), Ok(None)) {
                child.kill().ok();
            }
            child.wait().ok();
        }
        self.stop_tpm();
    }

    /// Stops the software TPM.
    pub fn stop_tpm(&mut self) {
        if let Some(swtpm) = &mut self.swtpm {
            swtpm.stop();
        }
    }
}

impl Drop for Vm<'_> {
    fn drop(&mut self) {
        self.kill();
        fs::remove_file(&self.sockets.serial).ok();
        fs::remove_file(&self.sockets.monitor).ok();
    }
}

/// Connects to QEMU's serial console socket once QEMU has opened it.
fn connect(socket: &Path, child: &mut Child) -> Result<UnixStream> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(stream) = UnixStream::connect(socket) {
            return Ok(stream);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(Error::Setup(format!("qemu-system-x86_64 ended ({status})")));
        }
        if Instant::now() >= deadline {
            return Err(Error::Setup("QEMU did not open its serial console".into()));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// The output of a shell command: the lines that are not blank, without the
/// harness's prompt.
pub fn shell_output(before: &str) -> String {
    let lines: Vec<&str> = before
        .split('\n')
        .filter(|line| !line.trim().is_empty() && line.trim() != "zbm#")
        .map(|line| line.strip_prefix("zbm# ").unwrap_or(line))
        .collect();
    lines.join("\n")
}

/// `output` without the lines of `sent` that the shell echoed first.
///
/// [`Vm::shell_ready`] turns echo off, and Bash's line editor honours it,
/// but brush's draws what is typed whatever the terminal's modes: in Rust
/// images the output starts with the command.
pub fn without_echo(output: &str, sent: &[String]) -> String {
    let lines: Vec<&str> = output.split('\n').collect();
    let echoed = lines
        .iter()
        .zip(sent)
        .take_while(|(line, sent)| line.trim_end() == sent.trim_end())
        .count();
    lines.get(echoed..).unwrap_or_default().join("\n")
}

#[cfg(test)]
mod tests;
