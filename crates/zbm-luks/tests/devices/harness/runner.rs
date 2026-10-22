//! The device runner: the commands of the LUKS code, run for real or
//! answered by the stand-ins for device-mapper, `zpool` and security keys.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use zbm_platform::Secret;
use zbm_platform::exec::{Command, Input, Output, Runner, Status, SystemRunner};

use super::{CRYPTSETUP, Fido, record_name, run, try_run};

/// How the stand-in `cryptsetup luksDump` prints tokens.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Dump {
    /// As `cryptsetup` prints them.
    #[default]
    Plain,
    /// Every token after the first on standard error, as cryptsetup 2.8 does
    /// with systemd 261's token plugins.
    LaterTokensOnStderr,
}

/// The device runner of a case; see the module documentation.
pub struct DeviceRunner {
    pub(super) fake: PathBuf,
    pub(super) mapper: PathBuf,
    pub(super) env: Vec<(&'static str, String)>,
    pub(super) fido: Fido,
    pub(super) calls: Mutex<Vec<String>>,
    pub(super) minor: AtomicU32,
    /// The FIDO2 stand-in asks for the PIN.
    pub fido_needs_pin: Mutex<bool>,
    /// How `luksDump` prints tokens.
    pub dump: Mutex<Dump>,
    /// Whether the FIDO2 and YubiKey stand-ins answer (`kstubs` on `PATH`).
    pub key_stubs: bool,
}

impl DeviceRunner {
    fn output(
        status: i32,
        stdout: impl Into<Vec<u8>>,
        stderr: impl Into<Vec<u8>>,
    ) -> io::Result<Output> {
        let mut output = Output::new(
            Status::exited(u8::try_from(status).unwrap_or(1)),
            stdout.into(),
        );
        output.stderr = stderr.into();
        Ok(output)
    }

    /// The real program, with the lab's environment.
    fn real(&self, command: &Command) -> io::Result<Output> {
        let mut real = command.clone();
        for (name, value) in &self.env {
            if !command.get_env().iter().any(|(key, _)| key == name) {
                real = real.env(name, value);
            }
        }
        SystemRunner.run(&real)
    }

    fn args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn cryptsetup(&self, command: &Command) -> io::Result<Output> {
        let args = Self::args(command);
        match args.first().map(String::as_str) {
            Some("open") if !args.iter().any(|arg| arg == "--test-passphrase") => {
                self.open(command, &args)
            }
            Some("close") => self.close(args.get(1).map_or("", String::as_str)),
            Some("luksDump")
                if *self.dump.lock().unwrap() == Dump::LaterTokensOnStderr
                    && !args.iter().any(|arg| arg == "--dump-json-metadata") =>
            {
                let output = self.real(command)?;
                let text = output.stdout_text();
                let mut stdout = String::new();
                let mut stderr = String::new();
                let mut later = false;
                for line in text.split_inclusive('\n') {
                    if line.starts_with("  1: ") {
                        later = true;
                    }
                    if later {
                        stderr.push_str(line)
                    } else {
                        stdout.push_str(line)
                    }
                }
                let mut changed = Output::new(output.status, stdout.into_bytes());
                changed.stderr = stderr.into_bytes();
                Ok(changed)
            }
            _ => self.real(command),
        }
    }

    fn open(&self, command: &Command, args: &[String]) -> io::Result<Output> {
        let mut options = Vec::new();
        let mut positional = Vec::new();
        let mut rest = args.iter().skip(1);
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "--type" | "--header" | "--keyfile-offset" | "--keyfile-size" | "--key-slot"
                | "--token-id" | "--token-type" => {
                    options.push(arg.clone());
                    options.extend(rest.next().cloned());
                }
                "--allow-discards" | "--readonly" => {}
                flag if flag.starts_with("--perf-") => {}
                flag if flag.starts_with("--") => options.push(arg.clone()),
                _ => positional.push(arg.clone()),
            }
        }
        let device = positional.first().cloned().unwrap_or_default();
        let name = positional.get(1).cloned().unwrap_or_default();
        let node = self.mapper.join(&name);
        if node.exists() {
            return Self::output(5, "", "exists\n");
        }
        let mut test = Command::new(CRYPTSETUP)
            .arg("open")
            .arg("--test-passphrase")
            .args(options.iter())
            .arg(&device)
            .stdin(command.get_stdin().clone())
            .stdout(command.get_stdout());
        test = if command.merges_stderr() {
            test.merge_stderr()
        } else {
            test.stderr(command.get_stderr())
        };
        for (key, value) in command.get_env() {
            if let Some(value) = value {
                test = test.env(key, value);
            }
        }
        let output = self.real(&test)?;
        if !output.status.is_success() {
            return Ok(output);
        }
        let minor = self.minor.fetch_add(1, Ordering::SeqCst);
        let node_text = node.display().to_string();
        let made = try_run(
            "mknod",
            &[&node_text, "b", "7", &minor.to_string()],
            b"",
            &[],
        );
        if !made.status.success() {
            return Self::output(5, "", "exists\n");
        }
        let _ = std::fs::write(
            self.fake.join("holders").join(record_name(&device)),
            format!("{name}\n"),
        );
        let _ = std::fs::write(self.fake.join("maps").join(&name), format!("{device}\n"));
        Ok(output)
    }

    fn close(&self, name: &str) -> io::Result<Output> {
        if self.fake.join("busy").join(name).exists() {
            return Self::output(5, "", "busy\n");
        }
        let node = self.mapper.join(name);
        if !node.exists() {
            return Self::output(4, "", "");
        }
        let map = self.fake.join("maps").join(name);
        let device = std::fs::read_to_string(&map).unwrap_or_default();
        let _ = std::fs::remove_file(&node);
        let _ = std::fs::remove_file(&map);
        let _ = std::fs::remove_file(self.fake.join("holders").join(record_name(device.trim())));
        Self::output(0, "", "")
    }

    fn blkid(&self, command: &Command) -> io::Result<Output> {
        for arg in Self::args(command) {
            if let Ok(name) = Path::new(&arg).strip_prefix(&self.mapper) {
                let Ok(device) = std::fs::read_to_string(self.fake.join("maps").join(name)) else {
                    return Self::output(2, "", "");
                };
                let content =
                    std::fs::read(self.fake.join("content").join(record_name(device.trim())))
                        .unwrap_or_default();
                return Self::output(0, content, "");
            }
        }
        self.real(command)
    }

    fn zpool(&self, command: &Command) -> io::Result<Output> {
        let args = Self::args(command);
        let pools = self.fake.join("pools");
        match args.first().map(String::as_str) {
            Some("list") => {
                let mut names: Vec<String> = std::fs::read_dir(&pools)
                    .map(|entries| {
                        entries
                            .filter_map(|entry| {
                                entry
                                    .ok()
                                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                names.sort();
                let text: String = names.iter().map(|name| format!("{name}\n")).collect();
                Self::output(0, text, "")
            }
            Some("status") => {
                let pool = args.last().cloned().unwrap_or_default();
                let mut text = format!("  pool: {pool}\nconfig:\n");
                for vdev in std::fs::read_to_string(pools.join(&pool))
                    .unwrap_or_default()
                    .lines()
                {
                    let real = std::fs::canonicalize(vdev).unwrap_or_else(|_| PathBuf::from(vdev));
                    text.push_str(&format!("\t  {}  ONLINE 0 0 0\n", real.display()));
                }
                Self::output(0, text, "")
            }
            Some("export") => {
                let pool = args.get(1).cloned().unwrap_or_default();
                if std::fs::remove_file(pools.join(&pool)).is_err() {
                    return Self::output(1, "", "");
                }
                let exported = self.fake.join("exported");
                let mut text = std::fs::read_to_string(&exported).unwrap_or_default();
                text.push_str(&format!("{pool}\n"));
                std::fs::write(exported, text).unwrap();
                Self::output(0, "", "")
            }
            _ => Self::output(0, "", ""),
        }
    }

    fn fido2_assert(&self, command: &Command) -> io::Result<Output> {
        let args = Self::args(command);
        let input = match command.get_stdin() {
            Input::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            Input::Secret(secret) => String::from_utf8_lossy(secret.expose()).into_owned(),
            _ => String::new(),
        };
        let mut lines = input.lines();
        let cdh = lines.next().unwrap_or_default();
        let rp = lines.next().unwrap_or_default();
        let credential = lines.next().unwrap_or_default();
        let salt = lines.next().unwrap_or_default();
        if rp != "io.systemd.cryptsetup"
            || credential != self.fido.credential
            || salt != self.fido.salt
        {
            return Self::output(1, "", "bad input\n");
        }
        let line = format!(" {} ", args.join(" "));
        if !line.contains(" -G -h ") {
            return Self::output(1, "", "");
        }
        if *self.fido_needs_pin.lock().unwrap() && !line.contains(" pin=true ") {
            return Self::output(1, "", "FIDO_ERR_PIN_REQUIRED\n");
        }
        let text = format!("{cdh}\n{rp}\nAUTHDATA\nSIG\n{}\n", self.fido.hmac);
        Ok(Output::with_secret(
            Status::exited(0),
            Secret::copy_from(text.as_bytes()),
        ))
    }

    fn ykchalresp(command: &Command) -> io::Result<Output> {
        let args = Self::args(command);
        if args.first().map(String::as_str) != Some("-2") {
            return Self::output(1, "", "");
        }
        let challenge = if args.get(1).map(String::as_str) == Some("-i-") {
            match command.get_stdin() {
                Input::Secret(secret) => secret.expose().to_vec(),
                Input::Bytes(bytes) => bytes.clone(),
                _ => Vec::new(),
            }
        } else {
            args.get(1).cloned().unwrap_or_default().into_bytes()
        };
        let digest = run(
            "openssl",
            &["dgst", "-sha1", "-hmac", "yk-secret"],
            &challenge,
            &[],
        );
        let response = digest.rsplit(' ').next().unwrap_or_default();
        Ok(Output::with_secret(
            Status::exited(0),
            Secret::copy_from(format!("{response}\n").as_bytes()),
        ))
    }
}

impl Runner for DeviceRunner {
    fn run(&self, command: &Command) -> io::Result<Output> {
        self.calls.lock().unwrap().push(command.display());
        let program = command.program().to_string_lossy().into_owned();
        match program.rsplit('/').next().unwrap_or_default() {
            "cryptsetup" => self.cryptsetup(command),
            "blkid" => self.blkid(command),
            "zpool" => self.zpool(command),
            "tput" | "udevadm" => Self::output(0, "", ""),
            "fido2-token" if self.key_stubs => {
                let listed = Self::args(command).first().map(String::as_str) == Some("-L");
                Self::output(
                    0,
                    if listed {
                        concat!(
                            "/dev/hidraw99: vendor=0x1050, product=0x0407 ",
                            "(Yubico YubiKey OTP+FIDO+CCID)\n"
                        )
                    } else {
                        ""
                    },
                    "",
                )
            }
            "fido2-assert" if self.key_stubs => self.fido2_assert(command),
            "ykchalresp" if self.key_stubs => Self::ykchalresp(command),
            "ykinfo" if self.key_stubs => Self::output(0, "1234567\n", ""),
            _ => self.real(command),
        }
    }
}
