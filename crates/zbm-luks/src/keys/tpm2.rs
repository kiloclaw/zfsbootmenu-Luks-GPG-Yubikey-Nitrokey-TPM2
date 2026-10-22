//! systemd-tpm2 tokens: `luks_have_tpm2`, `luks_tpm2_in_lockout`,
//! `luks_tpm2_native_unseal` and `luks_try_tpm2_token`.
//!
//! Without cryptsetup's plugin, the token is unsealed with tpm2-tools as
//! systemd's `tpm2_unseal()` does: the sealed object is loaded below the
//! storage root key, a policy session satisfies PolicyPCR (and
//! PolicyAuthValue when a PIN is set), and the LUKS passphrase is the base64
//! encoding of the unsealed secret. The secret is captured from
//! `tpm2_unseal`'s output and reaches `cryptsetup` on standard input; the PIN's
//! authorization value reaches `tpm2_unseal` on standard input as
//! `hex:<value>`, because tpm2-tools reads it as text.

use std::path::Path;
use std::time::Duration;

use zbm_core::exec::{Command, Input, Stream};
use zbm_core::secret::Secret;
use zbm_system::files::FileKind;

use super::{Outcome, Target};
use crate::crypto::{base64_decode, base64_encode, hex, pbkdf2_hmac_sha256, sha256};
use crate::session::{Key, Session};
use crate::tokens::{json_arr, json_bool, json_str};

/// The persistent handle of the storage root key of systemd 254 and later.
pub const SRK_HANDLE: &str = "0x81000001";

/// The tools of the native unsealing.
const TOOLS: [&str; 6] = [
    "tpm2_load",
    "tpm2_startauthsession",
    "tpm2_policypcr",
    "tpm2_policyauthvalue",
    "tpm2_unseal",
    "tpm2_flushcontext",
];

/// The authorization value of a TPM2 PIN, in hexadecimal: the SHA-256 of the
/// PIN without trailing zero bytes. systemd 255 and later salt the PIN first:
/// the base64 encoding of PBKDF2-HMAC-SHA256 with the token's salt, 10000
/// rounds and 32 bytes.
pub fn auth_value(pin: &[u8], salt: Option<&[u8]>) -> Secret {
    let salted = salt.map(|salt| base64_encode(pbkdf2_hmac_sha256(pin, salt, 10_000, 32).expose()));
    let digest = sha256(salted.as_ref().map_or(pin, Secret::expose));
    let len = digest
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |last| last + 1);
    hex(&digest[..len])
}

/// The sizes of the `TPM2B_PRIVATE` and the `TPM2B_PUBLIC` at the start of a
/// sealed blob, which must make up all of it: anything after them is an
/// import seed, which only the plugin handles.
pub fn split_blob(blob: &[u8]) -> Option<(&[u8], &[u8])> {
    let size = |at: usize| -> Option<usize> {
        let bytes = blob.get(at..at + 2)?;
        Some(usize::from(bytes[0]) << 8 | usize::from(bytes[1]))
    };
    let private = size(0)? + 2;
    let public = size(private)? + 2;
    (private + public == blob.len()).then(|| blob.split_at(private))
}

impl Session<'_> {
    /// A tpm2-tools command for the TPM of the settings.
    fn tpm2_command(&self, program: &str) -> Command {
        self.command(program)
            .env("TPM2TOOLS_TCTI", &self.settings().tpm2_tcti)
            .stdout(Stream::Null)
            .stderr(Stream::Null)
    }

    fn succeeds(&self, command: &Command) -> bool {
        self.system()
            .runner
            .run(command)
            .is_ok_and(|output| output.status.success())
    }

    /// Whether the TPM device is there, as `[ -c ]` finds it.
    fn tpm2_device_present(&self, device: &str) -> bool {
        self.system()
            .files
            .metadata(Path::new(device))
            .is_ok_and(|metadata| metadata.kind == FileKind::CharDevice)
    }

    /// Whether a TPM2 is available (`luks_have_tpm2`). A TPM that is not a
    /// device (a simulator) always is; otherwise the drivers are loaded once,
    /// and the device is given a moment to appear.
    pub fn have_tpm2(&mut self) -> bool {
        let Some(device) = self.settings().tpm2_tcti.strip_prefix("device:") else {
            return true;
        };
        if self.tpm2_device_present(device) {
            return true;
        }
        if !self.tpm2_probed {
            self.tpm2_probed = true;
            for module in ["tpm_crb", "tpm_tis"] {
                let modprobe = self
                    .command("modprobe")
                    .args(["-q", module])
                    .stdout(Stream::Null)
                    .stderr(Stream::Null);
                let _ = self.system().runner.run(&modprobe);
            }
            for _ in 0..30 {
                self.settle();
                if self.tpm2_device_present(device) {
                    return true;
                }
                // Give up early when the kernel found no TPM at all.
                let tpms = self.system().files.read_dir(Path::new("/sys/class/tpm"));
                if tpms.unwrap_or_default().is_empty() {
                    break;
                }
                self.system().clock.sleep(Duration::from_millis(100));
            }
        }
        self.tpm2_device_present(device)
    }

    /// Whether the TPM refuses PIN-protected objects after too many wrong PINs
    /// (`luks_tpm2_in_lockout`).
    pub fn tpm2_in_lockout(&self) -> bool {
        if !self.system().command_exists("tpm2_getcap") {
            return false;
        }
        let getcap = self
            .tpm2_command("tpm2_getcap")
            .arg("properties-variable")
            .stdout(Stream::Capture);
        let Ok(output) = self.system().runner.run(&getcap) else {
            return false;
        };
        output.stdout_text().lines().any(|line| {
            line.find("inLockout:")
                .is_some_and(|at| line[at + 10..].trim_start_matches(' ').starts_with('1'))
        })
    }

    fn tpm2_lockout_message(&self) {
        self.say(&self.paint(
            "red",
            "The TPM is locked against PIN guessing after too many wrong PINs.\n",
        ));
        self.say(&self.paint(
            "red",
            "Use another unlock method; the lockout clears by itself over time.\n",
        ));
    }

    /// Unseals a systemd-tpm2 token with tpm2-tools
    /// (`luks_tpm2_native_unseal`).
    pub fn tpm2_native_unseal(&self, json: &str, pin: Option<&Secret>) -> Outcome {
        let log = self.log();
        let system = self.system();
        if let Some(tool) = TOOLS.iter().find(|tool| !system.command_exists(tool)) {
            log.debug(format!("native TPM2 unlocking needs {tool}"));
            return Outcome::Unsupported;
        }
        // Signed PCR policies, pcrlock and pre-254 primary keys need the
        // plugin.
        if json_str(json, "tpm2-pubkey").is_some()
            || json_bool(json, "tpm2_pcrlock").as_deref() == Some("true")
        {
            log.debug("TPM2 token uses a signed or pcrlock policy");
            return Outcome::Unsupported;
        }
        if json_str(json, "tpm2_srk").is_none() {
            log.debug("TPM2 token predates systemd's persistent SRK");
            return Outcome::Unsupported;
        }
        let Some(blob) = json_str(json, "tpm2-blob") else {
            return Outcome::Unsupported;
        };
        let pcrs = json_arr(json, "tpm2-pcrs").unwrap_or_default();
        let bank = json_str(json, "tpm2-pcr-bank").unwrap_or_else(|| "sha256".to_owned());

        let blob = base64_decode(&blob).unwrap_or_default();
        let Some((private, public)) = split_blob(&blob) else {
            log.debug("TPM2 blob has an unsupported layout");
            return Outcome::Unsupported;
        };

        let auth = pin.map(|pin| {
            let salt =
                json_str(json, "tpm2_salt").map(|salt| base64_decode(&salt).unwrap_or_default());
            auth_value(pin.expose(), salt.as_deref())
        });

        let Some(work) = self
            .temporary_dir("/run/zbm-tpm2")
            .or_else(|| self.temporary_dir("/tmp/zbm-tpm2"))
        else {
            return Outcome::Failed;
        };
        let files = system.files;
        let _ = files.set_mode(Path::new(&work), 0o700);
        let path = |name: &str| format!("{work}/{name}");
        let written = files.write(Path::new(&path("priv")), private).is_ok()
            && files.write(Path::new(&path("pub")), public).is_ok();

        let mut outcome = Outcome::Failed;
        let load = self.tpm2_command("tpm2_load").args([
            "-C",
            SRK_HANDLE,
            "-u",
            &path("pub"),
            "-r",
            &path("priv"),
            "-c",
            &path("obj.ctx"),
        ]);
        let session = self.tpm2_command("tpm2_startauthsession").args([
            "--policy-session",
            "-S",
            &path("session.ctx"),
        ]);
        if written && self.succeeds(&load) && self.succeeds(&session) {
            let policy_pcr = self.tpm2_command("tpm2_policypcr").args([
                "-S",
                &path("session.ctx"),
                "-l",
                &format!("{bank}:{pcrs}"),
            ]);
            let policy_auth = self
                .tpm2_command("tpm2_policyauthvalue")
                .args(["-S", &path("session.ctx")]);
            let mut policy = format!("session:{}", path("session.ctx"));
            let mut stdin = Input::Null;
            if let Some(auth) = &auth {
                policy.push_str("+file:-");
                let mut text = Secret::copy_from(b"hex:");
                text.push_bytes(auth.expose());
                stdin = Input::Secret(text);
            }
            let unseal = self
                .tpm2_command("tpm2_unseal")
                .args(["-c", &path("obj.ctx"), "-p", &policy])
                .stdin(stdin)
                .stdout(Stream::CaptureSecret);
            if (pcrs.is_empty() || self.succeeds(&policy_pcr))
                && (auth.is_none() || self.succeeds(&policy_auth))
                && let Ok(output) = system.runner.run(&unseal)
                && output.status.success()
            {
                let secret = output.secret.unwrap_or_default();
                outcome = Outcome::Key(base64_encode(secret.expose()));
            }
            let flush = self
                .tpm2_command("tpm2_flushcontext")
                .arg(path("session.ctx"));
            let _ = system.runner.run(&flush);
        }
        let flush = self.tpm2_command("tpm2_flushcontext").arg("-t");
        let _ = system.runner.run(&flush);

        for name in ["priv", "pub", "obj.ctx", "session.ctx"] {
            let _ = files.remove_file(Path::new(&path(name)));
        }
        let _ = files.remove_dir(Path::new(&work));
        outcome
    }

    /// Unseals with ZFSBootMenu's own code and opens the volume; `Err` with
    /// whether the token is unsupported.
    fn tpm2_native_open(
        &self,
        target: Target<'_>,
        json: &str,
        pin: Option<&Secret>,
    ) -> Result<(), bool> {
        match self.tpm2_native_unseal(json, pin) {
            Outcome::Key(key) => {
                if self.cryptsetup_open(target.name, target.device, Key::Stdin(&key), target.args) {
                    Ok(())
                } else {
                    Err(false)
                }
            }
            Outcome::Failed => Err(false),
            Outcome::Unsupported => Err(true),
        }
    }

    /// Opens a volume with a systemd-tpm2 token (`luks_try_tpm2_token`). A PIN
    /// that already worked is tried once, then the user is asked, up to three
    /// times: wrong PINs count towards the TPM's dictionary-attack lockout, so
    /// no PIN is asked for during a lockout.
    pub fn try_tpm2_token(&mut self, target: Target<'_>, id: &str, json: &str) -> bool {
        let device = target.device;
        if !self.have_tpm2() {
            self.log()
                .info(format!("no TPM2 available for token {id} on {device}"));
            return false;
        }

        if json_bool(json, "tpm2-pin").as_deref() != Some("true") {
            // Without a PIN, unlocking needs no interaction.
            if self.have_token_plugin("systemd-tpm2") && self.open_token_plugin(target, id, None) {
                return true;
            }
            let result = self.tpm2_native_open(target, json, None);
            if result == Err(true) && !self.have_token_plugin("systemd-tpm2") {
                self.log().warning(format!(
                    "TPM2 token {id} on {device} needs the systemd-tpm2 cryptsetup plugin"
                ));
            }
            if result.is_ok() {
                self.log().info(format!("unlocked {device} with TPM2"));
            }
            return result.is_ok();
        }

        if self.tpm2_in_lockout() {
            self.say("\n");
            self.tpm2_lockout_message();
            self.log().warning(format!(
                "TPM2 in dictionary-attack lockout; skipping PIN token {id} on {device}"
            ));
            return false;
        }

        let tries = 3;
        let mut attempt = 0;
        let cached: Vec<Secret> = self.tpm2_pins.clone();
        for cached in cached.iter().map(Some).chain([None]) {
            loop {
                let pin = match cached {
                    Some(pin) => pin.clone(),
                    None => {
                        attempt += 1;
                        if attempt > tries {
                            return false;
                        }
                        self.say("\n");
                        let line =
                            format!("Unlock {} with the TPM\n", self.paint("yellow", device));
                        self.say(&self.paint("cyan", &line));
                        match self.read_secret("TPM2 PIN (leave empty to use another method): ") {
                            Some(pin) if !pin.is_empty() => pin,
                            _ => return false,
                        }
                    }
                };

                let opened = if self.have_token_plugin("systemd-tpm2") {
                    self.open_token_plugin(target, id, Some(&pin))
                } else {
                    match self.tpm2_native_open(target, json, Some(&pin)) {
                        Ok(()) => true,
                        Err(true) => {
                            self.log().warning(format!(
                                "TPM2 token {id} on {device} needs the systemd-tpm2 \
                                 cryptsetup plugin"
                            ));
                            return false;
                        }
                        Err(false) => false,
                    }
                };

                if opened {
                    if cached.is_none() {
                        self.tpm2_pins.push(pin);
                    }
                    self.log()
                        .info(format!("unlocked {device} with TPM2 and PIN"));
                    return true;
                }
                drop(pin);

                if self.tpm2_in_lockout() {
                    self.tpm2_lockout_message();
                    return false;
                }
                // A cached PIN is only ever tried once.
                if cached.is_some() {
                    break;
                }
                self.say(&self.paint(
                    "red",
                    "The TPM did not release the key (wrong PIN or changed PCRs).\n",
                ));
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::tests::with_session;
    use crate::session::Settings;
    use crate::testing::{legacy_function, legacy_luks};
    use zbm_core::exec::{Output, Status};
    use zbm_testutil::fakes::FakeSystem;
    use zbm_testutil::{have_program, repo_root};

    fn fixture(name: &str) -> String {
        let path = repo_root()
            .join("crates/zbm-luks/tests/fixtures")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    /// The PIN block of the legacy `luks_tpm2_native_unseal`, which turns the
    /// PIN into the authorization value.
    fn legacy_auth(json: &str, pin: &str) -> String {
        let function = legacy_function(
            "zfsbootmenu/lib/zfsbootmenu-luks-keys.sh",
            "luks_tpm2_native_unseal",
        );
        let start = function.find("  if [ -n \"${pin}\" ]; then\n").unwrap();
        let end = start + function[start..].find("\n  fi\n\n  ret=1").unwrap() + 5;
        let block = &function[start..end];
        let code = format!(
            "derive() {{ local json=\"$1\" pin=\"$2\" work=/nonexistent salt hexsalt auth\n\
             {block}\necho \"${{auth}}\"; }}"
        );
        let output = legacy_luks().code(&code).call("derive", &[json, pin], b"");
        output.stdout_text().trim_end().to_owned()
    }

    #[test]
    fn authorization_values_match_the_legacy_derivation() {
        if !have_program("sha256sum") || !have_program("openssl") {
            return;
        }
        let salted = fixture("token-tpm2-pin.json");
        let plain = fixture("token-tpm2.json");
        for pin in ["1234", "0000", "pïn with spaces", "-x", "a\\b"] {
            for json in [&salted, &plain] {
                let salt = json_str(json, "tpm2_salt").map(|salt| base64_decode(&salt).unwrap());
                let rust = auth_value(pin.as_bytes(), salt.as_deref());
                assert_eq!(
                    String::from_utf8_lossy(rust.expose()),
                    legacy_auth(json, pin),
                    "{pin:?}"
                );
            }
        }
    }

    #[test]
    fn trailing_zero_bytes_are_left_out() {
        // A PIN whose hash ends in a zero byte is rare, so check the trimming
        // directly.
        let digest = sha256(b"1234");
        let full = hex(&digest);
        assert_eq!(auth_value(b"1234", None).expose(), full.expose());
        let mut found = None;
        for n in 0_u32..5000 {
            let pin = n.to_string();
            if sha256(pin.as_bytes())[31] == 0 {
                found = Some(pin);
                break;
            }
        }
        let pin = found.unwrap();
        let auth = auth_value(pin.as_bytes(), None);
        assert!(auth.len() <= 62 && auth.len().is_multiple_of(2));
        assert!(!auth.expose().ends_with(b"00"));
    }

    #[test]
    fn blobs_hold_a_private_and_a_public_part() {
        let blob =
            base64_decode(&json_str(&fixture("token-tpm2.json"), "tpm2-blob").unwrap()).unwrap();
        let (private, public) = split_blob(&blob).unwrap();
        assert_eq!(private.len(), 0x9e + 2);
        assert_eq!(private.len() + public.len(), blob.len());
        let mut seeded = blob.clone();
        seeded.extend_from_slice(b"seed");
        assert!(split_blob(&seeded).is_none());
        assert!(split_blob(&blob[..1]).is_none());
        assert!(split_blob(&[0, 0, 0]).is_none());
        assert_eq!(split_blob(&[0, 0, 0, 0]), Some((&[0, 0][..], &[0, 0][..])));
    }

    /// A machine with every tool, whose `tpm2_unseal` releases `secret` when
    /// it gets `auth` (or none) on standard input.
    fn tpm_machine(auth: Option<Vec<u8>>, secret: &'static [u8]) -> FakeSystem {
        let fake = FakeSystem::new();
        fake.environment.set("PATH", "/bin");
        fake.files.dir("/run");
        for tool in TOOLS.iter().chain(&["tpm2_getcap"]) {
            fake.files.file_with_mode(format!("/bin/{tool}"), "", 0o755);
        }
        fake.runner.on(move |command| {
            if command.program() != "tpm2_unseal" {
                return None;
            }
            let given = match command.get_stdin() {
                Input::Secret(text) => Some(text.expose().to_vec()),
                _ => None,
            };
            Some(Ok(if given == auth {
                Output::with_secret(Status::exited(0), Secret::copy_from(secret))
            } else {
                Output::new(Status::exited(1), "")
            }))
        });
        fake
    }

    #[test]
    fn unsealing_runs_the_policy_and_keeps_the_secret_off_files() {
        let json = fixture("token-tpm2-pin.json");
        let salt = base64_decode(&json_str(&json, "tpm2_salt").unwrap()).unwrap();
        let mut auth = b"hex:".to_vec();
        auth.extend_from_slice(auth_value(b"1234", Some(&salt)).expose());
        let fake = tpm_machine(Some(auth), b"\x01sealed\x00secret");
        let mut settings = Settings::new("/zfsbootmenu");
        settings.tpm2_tcti = "swtpm:port=2321".to_owned();
        with_session(&fake, settings, |session| {
            let pin = Secret::copy_from(b"1234");
            match session.tpm2_native_unseal(&json, Some(&pin)) {
                Outcome::Key(key) => assert_eq!(key.expose(), b"AXNlYWxlZABzZWNyZXQ="),
                _ => panic!("the token was not unsealed"),
            }
            let wrong = Secret::copy_from(b"0000");
            assert!(matches!(
                session.tpm2_native_unseal(&json, Some(&wrong)),
                Outcome::Failed
            ));
        });
        let lines = fake.runner.lines();
        let work = "/run/zbm-tpm2.4242-0";
        assert_eq!(
            lines[..6],
            [
                format!("tpm2_load -C 0x81000001 -u {work}/pub -r {work}/priv -c {work}/obj.ctx"),
                format!("tpm2_startauthsession --policy-session -S {work}/session.ctx"),
                format!("tpm2_policypcr -S {work}/session.ctx -l sha256:7"),
                format!("tpm2_policyauthvalue -S {work}/session.ctx"),
                format!("tpm2_unseal -c {work}/obj.ctx -p session:{work}/session.ctx+file:-"),
                format!("tpm2_flushcontext {work}/session.ctx"),
            ]
        );
        assert_eq!(lines[6], "tpm2_flushcontext -t");
        assert!(fake.runner.calls().iter().all(|call| {
            call.get_env().iter().any(|(key, value)| {
                key == "TPM2TOOLS_TCTI" && value.as_deref() == Some("swtpm:port=2321".as_ref())
            })
        }));
        // Nothing is left behind, and the PIN went nowhere it could be read.
        assert!(
            !fake
                .files
                .paths()
                .iter()
                .any(|path| path.starts_with("/run/zbm-tpm2"))
        );
        fake.assert_no_leak(b"1234");
        fake.assert_no_leak(b"sealed");
    }

    #[test]
    fn tokens_that_need_the_plugin_are_reported() {
        let fake = tpm_machine(None, b"s");
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            let signed = r#"{"type":"systemd-tpm2","tpm2-pubkey":"AA==","tpm2_srk":"x"}"#;
            assert!(matches!(
                session.tpm2_native_unseal(signed, None),
                Outcome::Unsupported
            ));
            let old = r#"{"type":"systemd-tpm2","tpm2-blob":"AAAAAA=="}"#;
            assert!(matches!(
                session.tpm2_native_unseal(old, None),
                Outcome::Unsupported
            ));
            let seeded = r#"{"tpm2_srk":"x","tpm2-blob":"AAAAAAA="}"#;
            assert!(matches!(
                session.tpm2_native_unseal(seeded, None),
                Outcome::Unsupported
            ));
        });
        let debug = fake.kmsg();
        assert!(debug.contains("TPM2 token uses a signed or pcrlock policy"));
        assert!(debug.contains("TPM2 token predates systemd's persistent SRK"));
        assert!(debug.contains("TPM2 blob has an unsupported layout"));
    }

    #[test]
    fn pins_are_asked_for_and_kept_for_other_volumes() {
        let json = fixture("token-tpm2-pin.json");
        let salt = base64_decode(&json_str(&json, "tpm2_salt").unwrap()).unwrap();
        let mut auth = b"hex:".to_vec();
        auth.extend_from_slice(auth_value(b"1234", Some(&salt)).expose());
        let fake = tpm_machine(Some(auth), b"secret");
        fake.files.device("/dev/tpmrm0");
        fake.console.secret(b"0000").secret(b"1234");
        let args = [];
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            let a = Target {
                name: "a",
                device: "/dev/sda",
                args: &args,
            };
            assert!(session.try_tpm2_token(a, "0", &json));
            // The second volume gets the PIN that worked, without asking.
            let b = Target {
                name: "b",
                device: "/dev/sdb",
                args: &args,
            };
            assert!(session.try_tpm2_token(b, "0", &json));
        });
        let output = fake.console.output_text();
        assert_eq!(output.matches("TPM2 PIN (leave empty").count(), 2);
        assert!(output.contains("The TPM did not release the key"));
        let opened: Vec<String> = fake
            .runner
            .lines()
            .into_iter()
            .filter(|line| line.starts_with("cryptsetup open"))
            .collect();
        assert_eq!(
            opened,
            [
                "cryptsetup open --type luks --key-file=- /dev/sda a",
                "cryptsetup open --type luks --key-file=- /dev/sdb b",
            ]
        );
        fake.assert_no_leak(b"1234");
    }

    #[test]
    fn a_locked_tpm_is_not_asked_for_pins() {
        let fake = tpm_machine(None, b"s");
        fake.files.device("/dev/tpmrm0");
        fake.runner.respond(
            "tpm2_getcap properties-variable",
            0,
            "TPM2_PT_PERMANENT:\n  inLockout:                 1\n",
        );
        let args = [];
        let target = Target {
            name: "a",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(!session.try_tpm2_token(target, "0", &fixture("token-tpm2-pin.json")));
        });
        assert!(
            fake.console
                .output_text()
                .contains("locked against PIN guessing")
        );
        assert!(
            fake.kmsg()
                .contains("TPM2 in dictionary-attack lockout; skipping PIN token 0")
        );
    }

    #[test]
    fn missing_tpm_devices_are_probed_once() {
        let fake = FakeSystem::new();
        fake.files.dir("/sys/class/tpm");
        let args = [];
        let target = Target {
            name: "a",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(!session.try_tpm2_token(target, "0", "{}"));
            assert!(!session.try_tpm2_token(target, "0", "{}"));
        });
        assert_eq!(
            fake.runner.lines(),
            [
                "modprobe -q tpm_crb",
                "modprobe -q tpm_tis",
                "udevadm settle"
            ]
        );
    }
}
