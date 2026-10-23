//! What the device suites share: a lab of real LUKS volumes on loop devices,
//! a device runner that stands in for device-mapper, and the cases that run
//! the LUKS code against them.
//!
//! The suites replace the Bash suites of `tests/luks/unit`, case for case and
//! check for check. Like them, they use real LUKS2 headers, `cryptsetup`, a
//! software TPM (`swtpm`), `systemd-cryptenroll`, clevis and GnuPG, and stand
//! in for what a test machine lacks:
//!
//! - device-mapper: `cryptsetup open` checks the key with `--test-passphrase`
//!   and creates a block device node in the case's own mapper directory, and
//!   `cryptsetup close` removes it; nothing reaches the kernel's
//!   device-mapper, so no mount namespace is needed;
//! - `blkid` of a mapping reports the content the case gives the volume;
//! - `zpool` sees the pools the case writes;
//! - `tput` and `udevadm` do nothing;
//! - security keys (FIDO2, YubiKey challenge-response) are the stand-ins of
//!   `tests/luks/unit/kstubs`.
//!
//! Every command the LUKS code runs is recorded, in the order it ran.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

mod case;
mod gnupg;
mod runner;

pub use case::{Case, LinesConsole};
pub use gnupg::Gnupg;
pub use runner::{DeviceRunner, Dump};

/// The real `cryptsetup`.
const CRYPTSETUP: &str = "/usr/sbin/cryptsetup";

/// One suite at a time: they share `/dev/hidraw99`, which some cases move
/// away, and the loop devices.
static ONE_SUITE: Mutex<()> = Mutex::new(());

/// Whether the device suites run: only with `ZBM_DEVICE_TESTS=1`, and then
/// they must be able to, so that a run asked for cannot pass by skipping.
///
/// # Panics
///
/// With `ZBM_DEVICE_TESTS=1` but not as root, or without cryptsetup.
pub fn enabled(suite: &str) -> bool {
    if std::env::var_os("ZBM_DEVICE_TESTS").is_none_or(|value| value != "1") {
        eprintln!("skipped {suite}: set ZBM_DEVICE_TESTS=1 (cargo xtask test devices)");
        return false;
    }
    assert!(
        zbm_sys::credentials().uid == 0,
        "{suite}: the device suites need root"
    );
    assert!(
        Path::new(CRYPTSETUP).exists(),
        "{suite}: {CRYPTSETUP} is missing"
    );
    true
}

/// The results of a suite's checks, reported as the Bash suites report them.
pub struct Checks {
    suite: &'static str,
    passed: usize,
    failed: Vec<String>,
    case: String,
}

impl Checks {
    /// No checks yet.
    pub fn new(suite: &'static str) -> Checks {
        Checks {
            suite,
            passed: 0,
            failed: Vec::new(),
            case: String::new(),
        }
    }

    /// Starts the case `what`.
    pub fn case(&mut self, what: &str) {
        println!("== {what}");
        what.clone_into(&mut self.case);
    }

    /// Records the check `what`.
    pub fn check(&mut self, what: &str, ok: bool) {
        if ok {
            self.passed += 1;
            println!("  ok   - {what}");
        } else {
            println!("  FAIL - {what}");
            self.failed.push(format!("{}: {what}", self.case));
        }
    }

    /// Fails the test if a check failed, or if the suite ran a number of
    /// checks other than `expected`, the number of the Bash suite.
    pub fn finish(self, expected: usize) {
        let total = self.passed + self.failed.len();
        println!(
            "{} passed: {} failed: {}",
            self.suite,
            self.passed,
            self.failed.len()
        );
        assert!(
            self.failed.is_empty(),
            "failed checks:\n{}",
            self.failed.join("\n")
        );
        assert_eq!(
            total, expected,
            "{}: {total} checks, the Bash suite has {expected}",
            self.suite
        );
    }
}

/// Runs `program` with `args`, `stdin` on its standard input and `env`, and
/// returns its standard output; setup that fails fails the test.
pub fn run(program: &str, args: &[&str], stdin: &[u8], env: &[(&str, &str)]) -> String {
    let output = try_run(program, args, stdin, env);
    assert!(
        output.status.success(),
        "{program} {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

/// Runs `program` and returns what happened, whatever it was.
pub fn try_run(
    program: &str,
    args: &[&str],
    stdin: &[u8],
    env: &[(&str, &str)],
) -> std::process::Output {
    use std::io::Write as _;
    let mut child = std::process::Command::new(program)
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("{program}: {error}"));
    let mut input = child.stdin.take().unwrap();
    input.write_all(stdin).unwrap();
    drop(input);
    child.wait_with_output().unwrap()
}

/// The base64 encoding of `bytes`, as `base64 -w0` prints it.
pub fn base64(bytes: &[u8]) -> String {
    run("base64", &["-w0"], bytes, &[])
}

/// Random bytes from `/dev/urandom`.
pub fn random(len: usize) -> Vec<u8> {
    use std::io::Read as _;
    let mut bytes = vec![0; len];
    std::fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    bytes
}

/// The LUKS format of a volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// LUKS1, which has no tokens.
    Luks1,
    /// LUKS2.
    Luks2,
}

/// The FIDO2 credential that the stand-in security key answers for.
#[derive(Clone, Debug)]
pub struct Fido {
    /// The credential ID, in base64.
    pub credential: String,
    /// The salt of the hmac-secret, in base64.
    pub salt: String,
    /// The hmac-secret, in base64: the passphrase of the token's key slot.
    pub hmac: String,
}

impl Fido {
    fn new() -> Fido {
        Fido {
            credential: base64(&random(64)),
            salt: base64(&random(32)),
            hmac: base64(&random(32)),
        }
    }

    /// The JSON of a systemd-fido2 token for `slot`, requiring the PIN if
    /// `pin`.
    pub fn token(&self, slot: &str, pin: bool) -> String {
        let fields = [
            r#""type":"systemd-fido2""#.to_owned(),
            format!(r#""keyslots":["{slot}"]"#),
            format!(r#""fido2-credential":"{}""#, self.credential),
            format!(r#""fido2-salt":"{}""#, self.salt),
            r#""fido2-rp":"io.systemd.cryptsetup""#.to_owned(),
            format!(r#""fido2-clientPin-required":{pin}"#),
            r#""fido2-up-required":true,"fido2-uv-required":false"#.to_owned(),
        ];
        format!("{{{}}}", fields.join(","))
    }
}

/// The volumes and tools of a suite, which it sets up once and its cases
/// share. Dropping it detaches the loop devices and stops the TPM.
pub struct Lab {
    /// The suite's directory.
    pub dir: PathBuf,
    /// The TCTI of the software TPM, if the suite has one.
    pub tcti: String,
    /// The FIDO2 credential of the stand-in security key.
    pub fido: Fido,
    loops: Mutex<Vec<String>>,
    swtpm: Option<std::process::Child>,
    cases: AtomicU32,
    _one: MutexGuard<'static, ()>,
}

impl Lab {
    /// A new lab in `/var/tmp/zbm-luks-devices/<suite>`, with a software TPM
    /// listening on `tpm_port` if it is given.
    pub fn new(suite: &str, tpm_port: Option<u16>) -> Lab {
        let one = ONE_SUITE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = PathBuf::from("/var/tmp/zbm-luks-devices").join(suite);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut lab = Lab {
            dir,
            tcti: String::new(),
            fido: Fido::new(),
            loops: Mutex::new(Vec::new()),
            swtpm: None,
            cases: AtomicU32::new(0),
            _one: one,
        };
        if let Some(port) = tpm_port {
            lab.start_tpm(port);
        }
        if !Path::new("/dev/hidraw99").exists() {
            run("mknod", &["/dev/hidraw99", "c", "1", "3"], b"", &[]);
        }
        lab
    }

    fn start_tpm(&mut self, port: u16) {
        let state = self.dir.join("tpm");
        std::fs::create_dir_all(&state).unwrap();
        let state_text = state.display().to_string();
        let _ = try_run(
            "swtpm_setup",
            &["--tpm2", "--tpmstate", &state_text, "--overwrite"],
            b"",
            &[],
        );
        let server = format!("type=tcp,port={port}");
        let control = format!("type=tcp,port={}", port + 1);
        let tpmstate = format!("dir={state_text}");
        let child = std::process::Command::new("swtpm")
            .args([
                "socket",
                "--tpm2",
                "--tpmstate",
                &tpmstate,
                "--server",
                &server,
            ])
            .args(["--ctrl", &control, "--flags", "not-need-init,startup-clear"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        self.swtpm = Some(child);
        self.tcti = format!("swtpm:host=127.0.0.1,port={port}");
        std::thread::sleep(Duration::from_secs(1));
    }

    /// The environment of the TPM tools.
    pub fn tpm_env(&self) -> Vec<(&'static str, String)> {
        if self.tcti.is_empty() {
            Vec::new()
        } else {
            vec![
                ("SYSTEMD_TPM2_DEVICE", self.tcti.clone()),
                ("TPM2TOOLS_TCTI", self.tcti.clone()),
            ]
        }
    }

    /// Runs a TPM tool or `systemd-cryptenroll` with the TPM's environment.
    pub fn with_tpm(
        &self,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> std::process::Output {
        let tpm = self.tpm_env();
        let mut all: Vec<(&str, &str)> = tpm.iter().map(|(k, v)| (*k, v.as_str())).collect();
        all.extend_from_slice(env);
        try_run(program, args, b"", &all)
    }

    /// A file of the lab.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// The text of a path of the lab.
    pub fn text(&self, name: &str) -> String {
        self.path(name).display().to_string()
    }

    /// Attaches `image` to a loop device and returns the device.
    pub fn attach(&self, image: &Path, partitions: bool) -> String {
        let image = image.display().to_string();
        let mut args = vec!["-f", "--show"];
        if partitions {
            args.insert(0, "-P");
        }
        args.push(&image);
        let device = run("losetup", &args, b"", &[]);
        self.loops.lock().unwrap().push(device.clone());
        device
    }

    /// Detaches `device`, which [`Lab::attach`] attached.
    pub fn detach(&self, device: &str) {
        let _ = try_run("losetup", &["-d", device], b"", &[]);
        self.loops
            .lock()
            .unwrap()
            .retain(|loop_device| loop_device != device);
    }

    /// Creates an empty image of `megabytes`.
    pub fn image(&self, name: &str, megabytes: u64) -> PathBuf {
        let path = self.path(name);
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(megabytes << 20).unwrap();
        path
    }

    /// Formats `image` as LUKS with `passphrase` (PBKDF2 with 1000 iterations,
    /// which is fast) and the label `label`.
    pub fn format(
        target: &str,
        passphrase: &[u8],
        format: Format,
        label: Option<&str>,
        header: Option<&str>,
    ) {
        let kind = match format {
            Format::Luks1 => "luks1",
            Format::Luks2 => "luks2",
        };
        let mut args = vec!["luksFormat", "-q", "--type", kind, "--pbkdf", "pbkdf2"];
        args.extend(["--pbkdf-force-iterations", "1000"]);
        if let Some(label) = label {
            args.extend(["--label", label]);
        }
        if let Some(header) = header {
            args.extend(["--header", header]);
        }
        args.extend(["--key-file=-", target]);
        run(CRYPTSETUP, &args, passphrase, &[]);
    }

    /// A new LUKS volume named `name` on a loop device, with `passphrase`.
    pub fn luks(&self, name: &str, passphrase: &[u8], format: Format, label: bool) -> String {
        let image = self.image(&format!("{name}.img"), 32);
        let image_text = image.display().to_string();
        Self::format(&image_text, passphrase, format, label.then_some(name), None);
        self.attach(&image, false)
    }

    /// Adds the contents of `key_file` as a key of `device`, whose
    /// passphrase is `pw`.
    pub fn add_key(device: &str, key_file: &Path) {
        let key = key_file.display().to_string();
        run(
            CRYPTSETUP,
            &[
                "luksAddKey",
                "-q",
                "--pbkdf",
                "pbkdf2",
                "--pbkdf-force-iterations",
                "1000",
                "--key-file=-",
                device,
                &key,
            ],
            b"pw",
            &[],
        );
    }

    /// Imports the token `json` into `device`.
    pub fn import_token(&self, device: &str, json: &str) {
        let file = self.path(&format!(
            "token-{}.json",
            self.cases.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::write(&file, json).unwrap();
        let file = file.display().to_string();
        run(
            CRYPTSETUP,
            &["token", "import", "--json-file", &file, device],
            b"",
            &[],
        );
    }

    /// Enrolls the TPM in `device` (passphrase `pw`), bound to `pcrs`, with
    /// `pin` if one is given.
    pub fn enroll_tpm(&self, device: &str, pcrs: &str, pin: Option<&str>) {
        let tpm = format!("--tpm2-device={}", self.tcti);
        let pcrs = format!("--tpm2-pcrs={pcrs}");
        let mut args = vec![tpm.as_str(), pcrs.as_str()];
        let mut env = vec![("PASSWORD", "pw")];
        if let Some(pin) = pin {
            args.push("--tpm2-with-pin=yes");
            env.push(("NEWPIN", pin));
        }
        args.push(device);
        let output = self.with_tpm("systemd-cryptenroll", &args, &env);
        assert!(
            output.status.success(),
            "systemd-cryptenroll: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Binds clevis with a TPM2 pin to `device`, whose passphrase is `pw`.
    pub fn bind_clevis(&self, device: &str) {
        let tpm = self.tpm_env();
        let env: Vec<(&str, &str)> = tpm.iter().map(|(k, v)| (*k, v.as_str())).collect();
        run(
            "clevis",
            &["luks", "bind", "-y", "-k", "-", "-d", device, "tpm2", "{}"],
            b"pw",
            &env,
        );
    }

    /// The UUID of a LUKS volume.
    pub fn uuid(device: &str) -> String {
        run(CRYPTSETUP, &["luksUUID", device], b"", &[])
    }

    /// Starts a case.
    pub fn case(&self) -> Case<'_> {
        let n = self.cases.fetch_add(1, Ordering::SeqCst);
        Case::new(self, &self.dir.join("cases").join(n.to_string()))
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        for device in self.loops.lock().unwrap().drain(..) {
            let _ = try_run("losetup", &["-d", &device], b"", &[]);
        }
        if let Some(mut swtpm) = self.swtpm.take() {
            let _ = swtpm.kill();
            let _ = swtpm.wait();
        }
        let _ = std::fs::remove_file("/dev/hidraw99");
    }
}

/// The basename of a device after following links, as the stub names its
/// records.
fn record_name(device: &str) -> String {
    let real = std::fs::canonicalize(device).unwrap_or_else(|_| PathBuf::from(device));
    real.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether nothing decrypted is left behind (`noleft`).
pub fn no_leftovers() -> bool {
    ["/run", "/tmp"].iter().all(|dir| {
        std::fs::read_dir(dir).map_or(true, |entries| {
            !entries.filter_map(Result::ok).any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                ["zbm-key.", "zbm-gpg.", "zbm-tpm2."]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
            })
        })
    })
}

/// Whether a line of `text` is exactly `line`.
pub fn has_line(text: &str, line: &str) -> bool {
    text.lines().any(|candidate| candidate == line)
}

/// Whether `path` is mounted.
pub fn mounted(pattern: &str) -> bool {
    std::fs::read_to_string("/proc/mounts")
        .unwrap_or_default()
        .contains(pattern)
}

/// Writes an executable script.
pub fn script(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
