//! The lab's software TPM: starting it, enrolling TPM2 tokens with
//! `systemd-cryptenroll` as the host's systemd offers them, binding clevis to
//! it, reading how a token's PIN is hardened, and a systemd 262 Argon2id PIN
//! token made by hand.
//!
//! Where `systemd-cryptenroll` can harden a PIN with Argon2id (systemd 262
//! and later), `yes` gets fixed small costs, which systemd uses as given,
//! without its benchmark; before, `yes` is PBKDF2. The hand-made token is
//! built from the SB21 plan's known-answer vector V3 alone, which OpenSSL
//! computed, so that ZFSBootMenu's own Argon2id unsealing is checked against
//! a real TPM on every host, with values the code under test did not compute.

use std::fmt;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use zbm_crypt::token::Argon2idFields;

use super::{CRYPTSETUP, Lab, base64, run, try_run};

/// The fixed Argon2id costs of the PINs `yes` enrols on a host whose
/// `systemd-cryptenroll` has them: 64 MiB, 2 passes, 4 lanes.
const SMALL_ARGON2ID: [&str; 3] = [
    "--tpm2-argon2id-memory=64M",
    "--tpm2-argon2id-iterations=2",
    "--tpm2-argon2id-parallelism=4",
];

/// V3's PIN.
pub const V3_PIN: &str = "pässwörd 1";
/// V3's salt, the SHA-256 of `salt`, in base64.
const V3_SALT: &str = "Y0ea1poJCyWCd+yPum+ZQZov+ySJgVEGV8lEzNEUjpc=";
/// V3's costs: 1049599 bytes (1024 KiB), 3 passes, one lane.
const V3_COSTS: Argon2idFields = Argon2idFields {
    memcost_bytes: 1_049_599,
    iterations: 3,
    lanes: 1,
};
/// V3's sealed secret, the SHA-256 of `unsealed`, in hexadecimal.
const V3_UNSEALED: &str = "d5063820fd280baf7649198108ca65da3423d89b8db4c489bf760a6045630fde";
/// V3's authorization value: the SHA-256 of the base64 of the second half
/// of Argon2id's output, in hexadecimal.
const V3_AUTH: &str = "0c6703c802369b13f15c8de42260955939291cc380e585a6f7cfbc2bba64b079";
/// V3's LUKS passphrase: the base64 of HKDF-SHA256 of the first half and
/// the sealed secret.
const V3_PASSPHRASE: &str = "ze0Vg/k2P3jA60/7iG9YmTcMHNdvazb9se4eJD6bt9A=";
/// The persistent handle of systemd's storage root key.
const SRK: &str = "0x81000001";
/// The attributes of systemd's storage root key (its ECC template), which
/// tpm2-tools' default lacks `noda` of.
const SRK_ATTRIBUTES: &str =
    "fixedtpm|fixedparent|sensitivedataorigin|userwithauth|noda|restricted|decrypt";
/// The attributes of the object systemd seals a PIN's secret in: without
/// `userwithauth`, which tpm2-tools sets by default, only a policy session
/// opens it, and with a PIN, without `noda`.
pub const SEALED_ATTRIBUTES: &str = "fixedtpm|fixedparent";

/// How `systemd-cryptenroll` protects a TPM2 token's key.
#[derive(Clone, Copy, Debug)]
pub enum TpmPin<'p> {
    /// No PIN.
    No,
    /// The PIN, enrolled with `yes`: Argon2id at small fixed costs where
    /// the host's systemd has it (262 and later), PBKDF2 before.
    Yes(&'p str),
    /// The PIN, enrolled with `direct`: PBKDF2, on hosts whose systemd has
    /// Argon2id only.
    Direct(&'p str),
}

/// How a token's PIN is hardened, as the token records it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinKdf {
    /// Argon2id, with its costs (systemd 262 `yes`).
    Argon2id(Argon2idFields),
    /// PBKDF2 with the token's salt (systemd 255 to 261, or 262 `direct`).
    Pbkdf2,
    /// No PIN, or a PIN without salt (systemd before 255).
    None,
}

impl fmt::Display for PinKdf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PinKdf::Argon2id(costs) => write!(
                f,
                "argon2id ({} bytes, {}, {})",
                costs.memcost_bytes, costs.iterations, costs.lanes
            ),
            PinKdf::Pbkdf2 => f.write_str("pbkdf2"),
            PinKdf::None => f.write_str("none"),
        }
    }
}

impl PinKdf {
    /// Whether a PIN that `yes` enrolled on this host can be hardened as
    /// this: with the small Argon2id costs, or PBKDF2 where systemd 262 fell
    /// back to it without Argon2id in its libcrypto; before systemd 262,
    /// PBKDF2, or no salt before 255.
    pub fn is_what_yes_enrols(self) -> bool {
        let small = Argon2idFields {
            memcost_bytes: 64 << 20,
            iterations: 2,
            lanes: 4,
        };
        match self {
            PinKdf::Argon2id(costs) => cryptenroll_has_argon2id() && costs == small,
            PinKdf::Pbkdf2 => true,
            PinKdf::None => !cryptenroll_has_argon2id(),
        }
    }
}

/// Whether the host's `systemd-cryptenroll` can harden a TPM2 PIN with
/// Argon2id, as its help says; read once. What it enrolled is read from the
/// token ([`Lab::pin_kdf`]): without Argon2id in its libcrypto, systemd 262
/// enrols `direct` for `yes`, and still lists the options.
pub fn cryptenroll_has_argon2id() -> bool {
    static HAS: OnceLock<bool> = OnceLock::new();
    *HAS.get_or_init(|| {
        let help = try_run("systemd-cryptenroll", &["--help"], b"", &[]);
        String::from_utf8_lossy(&help.stdout).contains("--tpm2-argon2id-memory")
    })
}

/// What `clevis decrypt` with `env` makes of the clevis `token`, as
/// `cryptsetup token export` prints it: its JWE goes to it in its compact
/// form, as ZFSBootMenu decrypts it. `clevis luks pass` and `clevis luks
/// list` find the token in the `luksDump` text, which, with systemd's token
/// plugins and cryptsetup 2.8, lacks every token after the first.
pub fn clevis_decrypt(token: &str, env: &[(&str, &str)]) -> std::process::Output {
    let jwe = run(
        "jose",
        &["fmt", "-j-", "-Og", "jwe", "-o-"],
        token.as_bytes(),
        &[],
    );
    let compact = run("jose", &["jwe", "fmt", "-i-", "-c"], jwe.as_bytes(), &[]);
    try_run("clevis", &["decrypt"], compact.as_bytes(), env)
}

/// The bytes of the hexadecimal `text`.
fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

/// The hexadecimal of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Lab {
    /// Starts the software TPM, listening on `port` and its control channel
    /// on the next port.
    pub(super) fn start_tpm(&mut self, port: u16) {
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

    /// The environment of the TPM tools: systemd's, tpm2-tools' and
    /// clevis-pin-tpm2's (`TCTI`), which the clevis scripts run in place of
    /// their own tpm2 pin where it is installed (Fedora); without it,
    /// clevis-pin-tpm2 would take the host's own TPM.
    pub fn tpm_env(&self) -> Vec<(&'static str, String)> {
        if self.tcti.is_empty() {
            Vec::new()
        } else {
            vec![
                ("SYSTEMD_TPM2_DEVICE", self.tcti.clone()),
                ("TPM2TOOLS_TCTI", self.tcti.clone()),
                ("TCTI", self.tcti.clone()),
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

    /// Runs a TPM tool with the TPM's environment; setup that fails fails
    /// the test.
    fn tpm_must(&self, program: &str, args: &[&str]) {
        let output = self.with_tpm(program, args, &[]);
        assert!(
            output.status.success(),
            "{program} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Enrolls the TPM in `device` (passphrase `pw`), bound to `pcrs`, with
    /// the PIN `pin` asks for.
    pub fn enroll_tpm(&self, device: &str, pcrs: &str, pin: TpmPin<'_>) {
        let tpm = format!("--tpm2-device={}", self.tcti);
        let pcrs = format!("--tpm2-pcrs={pcrs}");
        let mut args = vec![tpm.as_str(), pcrs.as_str()];
        let mut env = vec![("PASSWORD", "pw")];
        match pin {
            TpmPin::No => {}
            TpmPin::Yes(pin) => {
                args.push("--tpm2-with-pin=yes");
                if cryptenroll_has_argon2id() {
                    args.extend(SMALL_ARGON2ID);
                }
                env.push(("NEWPIN", pin));
            }
            TpmPin::Direct(pin) => {
                args.push("--tpm2-with-pin=direct");
                env.push(("NEWPIN", pin));
            }
        }
        args.push(device);
        let output = self.with_tpm("systemd-cryptenroll", &args, &env);
        assert!(
            output.status.success(),
            "systemd-cryptenroll: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// How the PIN of the first systemd-tpm2 token of `device` is hardened.
    pub fn pin_kdf(device: &str) -> PinKdf {
        let dump = run(
            CRYPTSETUP,
            &["luksDump", "--dump-json-metadata", device],
            b"",
            &[],
        );
        let tokens = zbm_crypt::token::from_metadata(&dump)
            .unwrap_or_else(|error| panic!("the tokens of {device}: {error}"));
        let Some(token) = tokens
            .iter()
            .find(|token| token.kind == "systemd-tpm2" && token.flag("tpm2-pin") == Some(true))
        else {
            return PinKdf::None;
        };
        match token.argon2id() {
            Ok(Some(costs)) => PinKdf::Argon2id(costs),
            _ if token.text("tpm2_salt").is_some() => PinKdf::Pbkdf2,
            _ => PinKdf::None,
        }
    }

    /// Adds to `device` (passphrase `pw`) a systemd 262 TPM2 token whose PIN
    /// is hardened with Argon2id, made by hand from V3's constants: V3's
    /// passphrase in key slot 1, and V3's sealed secret sealed to the lab's
    /// TPM below systemd's storage root key, behind a `PolicyAuthValue` with
    /// V3's authorization value, with the attributes systemd gives it. The
    /// storage root key is made, from systemd's template, when the TPM has
    /// none. The token is complete, as systemd 262 writes one without PCRs,
    /// so that systemd's plugin accepts it at `cryptsetup token import`.
    pub fn make_argon2id_token(&self, device: &str) {
        let dir = self.path("argon2id");
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str| dir.join(name).display().to_string();
        if !self
            .with_tpm("tpm2_readpublic", &["-c", SRK], &[])
            .status
            .success()
        {
            let primary = file("srk.ctx");
            self.tpm_must(
                "tpm2_createprimary",
                &[
                    "-C",
                    "o",
                    "-g",
                    "sha256",
                    "-G",
                    "ecc",
                    "-a",
                    SRK_ATTRIBUTES,
                    "-c",
                    &primary,
                ],
            );
            self.tpm_must("tpm2_evictcontrol", &["-C", "o", "-c", &primary, SRK]);
            // Without a resource manager, the TPM keeps the transient copy
            // and has few slots.
            self.tpm_must("tpm2_flushcontext", &["-t"]);
        }
        let (trial, policy) = (file("trial.ctx"), file("policy"));
        self.tpm_must("tpm2_startauthsession", &["-S", &trial]);
        self.tpm_must("tpm2_policyauthvalue", &["-S", &trial, "-L", &policy]);
        self.tpm_must("tpm2_flushcontext", &[&trial]);
        let (secret, public, private) = (file("secret"), file("pub"), file("priv"));
        std::fs::write(&secret, unhex(V3_UNSEALED)).unwrap();
        let auth = format!("hex:{V3_AUTH}");
        self.tpm_must(
            "tpm2_create",
            &[
                "-C",
                SRK,
                "-L",
                &policy,
                "-a",
                SEALED_ATTRIBUTES,
                "-p",
                &auth,
                "-i",
                &secret,
                "-u",
                &public,
                "-r",
                &private,
            ],
        );
        let blob = [
            std::fs::read(&private).unwrap(),
            std::fs::read(&public).unwrap(),
        ]
        .concat();
        let policy_hash = hex(&std::fs::read(&policy).unwrap());
        let fields = [
            r#""type":"systemd-tpm2","keyslots":["1"]"#.to_owned(),
            format!(r#""tpm2-blob":"{}""#, base64(&blob)),
            r#""tpm2-pcrs":[],"tpm2-primary-alg":"ecc""#.to_owned(),
            format!(r#""tpm2-policy-hash":"{policy_hash}","tpm2-pin":true"#),
            // ZFSBootMenu needs it present only: the session is salted to the
            // TPM's own key at SRK.
            format!(r#""tpm2_srk":"{}""#, base64(b"systemd's SRK")),
            format!(r#""tpm2_salt":"{V3_SALT}""#),
            format!(r#""tpm2_argon2id_memcost":{}"#, V3_COSTS.memcost_bytes),
            format!(r#""tpm2_argon2id_iterations":{}"#, V3_COSTS.iterations),
            format!(r#""tpm2_argon2id_lanes":{}"#, V3_COSTS.lanes),
        ];
        let key = file("passphrase");
        std::fs::write(&key, V3_PASSPHRASE).unwrap();
        run(
            CRYPTSETUP,
            &[
                "luksAddKey",
                "-q",
                "--key-slot",
                "1",
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
        self.import_token(device, &format!("{{{}}}", fields.join(",")));
    }

    /// The attributes of the object [`Lab::make_argon2id_token`] sealed, as
    /// `tpm2_print` names them.
    pub fn sealed_attributes(&self) -> String {
        let public = self.path("argon2id").join("pub").display().to_string();
        let text = run("tpm2_print", &["-t", "TPM2B_PUBLIC", &public], b"", &[]);
        text.lines()
            .skip_while(|line| line.trim() != "attributes:")
            .find_map(|line| line.trim().strip_prefix("value: "))
            .unwrap_or_else(|| panic!("no attributes in {text}"))
            .to_owned()
    }

    /// Binds clevis with a TPM2 pin to `device`, whose passphrase is `pw`,
    /// and checks that the binding gives its key back from the lab's TPM: a
    /// binding sealed to another TPM fails here, where it is made.
    pub fn bind_clevis(&self, device: &str) {
        let tpm = self.tpm_env();
        let env: Vec<(&str, &str)> = tpm.iter().map(|(k, v)| (*k, v.as_str())).collect();
        run(
            "clevis",
            &["luks", "bind", "-y", "-k", "-", "-d", device, "tpm2", "{}"],
            b"pw",
            &env,
        );
        let dump = run(
            CRYPTSETUP,
            &["luksDump", "--dump-json-metadata", device],
            b"",
            &[],
        );
        let tokens = zbm_crypt::token::from_metadata(&dump)
            .unwrap_or_else(|error| panic!("the tokens of {device}: {error}"));
        let clevis: Vec<_> = tokens
            .iter()
            .filter(|token| token.kind == "clevis")
            .collect();
        assert!(!clevis.is_empty(), "no clevis token on {device}: {dump}");
        for token in clevis {
            let json = run(
                CRYPTSETUP,
                &["token", "export", "--token-id", &token.id, device],
                b"",
                &[],
            );
            let key = clevis_decrypt(&json, &env);
            assert!(
                key.status.success() && !key.stdout.is_empty(),
                "the clevis token {} of {device} does not open with the lab's TPM: {}",
                token.id,
                String::from_utf8_lossy(&key.stderr)
            );
        }
    }
}
