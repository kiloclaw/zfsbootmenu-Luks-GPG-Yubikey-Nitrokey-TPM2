//! tpm2-tools: unsealing a systemd-tpm2 token's sealed object.
//!
//! [`Tpm2`] runs the tools against one TPM (`TPM2TOOLS_TCTI`). A policy
//! session is a [`PolicySession`]: it is flushed from the TPM when it goes
//! out of scope, on every path, so a failed unseal does not leave the TPM's
//! few session slots taken.
//!
//! Sessions are salted with the storage root key and encrypt their
//! responses, as systemd's own are: the unsealed key leaves the TPM
//! encrypted under a session key that only the TPM and this process know,
//! so a device on the bus of a discrete TPM reads nothing of it.
//!
//! A tool that fails leaves its last lines of standard error, which carry
//! the TPM's response code, in the log at debug, with every form of the
//! authorization value it was given replaced by `<auth>`.
//!
//! ```
//! use std::path::Path;
//! use zbm_crypt::tpm2::Tpm2;
//! use zbm_testkit::FakeRunner;
//!
//! let runner = FakeRunner::new();
//! let tpm = Tpm2::new(&runner, "device:/dev/tpmrm0");
//! let work = Path::new("/run/zbm-tpm2");
//! tpm.load(&work.join("pub"), &work.join("priv"), &work.join("obj.ctx"))?;
//! {
//!     let session = tpm.policy_session(&work.join("session.ctx"))?;
//!     session.pcrs("sha256:7")?;
//! }
//! let flush = "tpm2_flushcontext /run/zbm-tpm2/session.ctx";
//! assert_eq!(runner.lines().last().map(String::as_str), Some(flush));
//! # Ok::<(), zbm_crypt::tpm2::ToolFailed>(())
//! ```

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_platform::{Command, Input, Output, Runner, Secret, Stream};

use crate::measure::{self, Bank, Digests};
use crate::stderr;

/// The longest a `tpm2_*` command may run, for a TPM that stopped
/// answering; its commands take seconds at most.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The persistent handle of the storage root key of systemd 254 and later.
pub const SRK_HANDLE: &str = "0x81000001";

/// The tools that unsealing needs.
pub const TOOLS: [&str; 7] = [
    "tpm2_load",
    "tpm2_startauthsession",
    "tpm2_sessionconfig",
    "tpm2_policypcr",
    "tpm2_policyauthvalue",
    "tpm2_unseal",
    "tpm2_flushcontext",
];

/// A tool of tpm2-tools that did not succeed.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
#[error("{0} failed")]
pub struct ToolFailed(pub &'static str);

/// The values `tpm2_pcrread` prints: a line `  sha256:` for each bank,
/// then `    9 : 0x1A2B...` for each PCR, as (bank, PCR, value).
fn pcr_values(text: &str) -> Vec<(Bank, u32, Vec<u8>)> {
    let mut bank = None;
    let mut values = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_suffix(':') {
            bank = Bank::named(name);
            continue;
        }
        let parsed = line.split_once(':').and_then(|(pcr, value)| {
            let pcr = pcr.trim().parse().ok()?;
            let hex = value.trim().strip_prefix("0x")?;
            Some((pcr, measure::unhex(&hex.to_ascii_lowercase())?))
        });
        if let (Some(bank), Some((pcr, value))) = (bank, parsed)
            && value.len() == bank.length()
        {
            values.push((bank, pcr, value));
        }
    }
    values
}

/// tpm2-tools for one TPM, run through a [`Runner`].
#[derive(Clone, Copy)]
pub struct Tpm2<'a> {
    runner: &'a dyn Runner,
    tcti: &'a str,
}

/// What stands in the log for the authorization value.
const AUTH: &[u8] = b"<auth>";

/// `text` with each `needle` in it replaced by `with`, in a secret: what
/// is not yet replaced may be the authorization value.
fn replace(text: &[u8], needle: &[u8], with: &[u8]) -> Secret {
    let mut replaced = Secret::new();
    let mut rest = text;
    while let Some(at) = (!needle.is_empty())
        .then(|| {
            rest.windows(needle.len())
                .position(|window| window == needle)
        })
        .flatten()
    {
        let (before, after) = rest.split_at(at);
        replaced.push_bytes(before);
        replaced.push_bytes(with);
        rest = after.get(needle.len()..).unwrap_or_default();
    }
    replaced.push_bytes(rest);
    replaced
}

/// The bytes that the hexadecimal digits `hex` stand for, if they are
/// digits.
fn unhex(hex: &[u8]) -> Option<Secret> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Secret::new();
    for pair in hex.chunks_exact(2) {
        let digits = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(digits, 16).ok()?);
    }
    Some(bytes)
}

/// `said` with every form in which the authorization value `auth` (its
/// hexadecimal digits) reaches a tool replaced by `<auth>`: the text on
/// standard input that `file:-` reads, `hex:` and the digits; the digits
/// alone, in either case; and the bytes they stand for. tpm2-tools do not
/// print it, but nothing here relies on that. Every step is a secret, wiped
/// when dropped.
fn scrub(said: &[u8], auth: Option<&Secret>) -> Secret {
    let Some(auth) = auth.filter(|auth| !auth.is_empty()) else {
        return Secret::copy_from(said);
    };
    let mut given = Secret::copy_from(b"hex:");
    given.push_bytes(auth.expose());
    let (mut lower, mut upper) = (Secret::new(), Secret::new());
    for &byte in auth.expose() {
        lower.push(byte.to_ascii_lowercase());
        upper.push(byte.to_ascii_uppercase());
    }
    let raw = unhex(auth.expose()).unwrap_or_default();
    [given, lower, upper, raw]
        .iter()
        .fold(Secret::copy_from(said), |text, form| {
            replace(text.expose(), form.expose(), AUTH)
        })
}

/// The text of a path for a tool's argument.
fn text(path: &Path) -> String {
    path.display().to_string()
}

impl<'a> Tpm2<'a> {
    /// The tools for the TPM `tcti`, such as `device:/dev/tpmrm0`.
    pub fn new(runner: &'a dyn Runner, tcti: &'a str) -> Tpm2<'a> {
        Tpm2 { runner, tcti }
    }

    /// A command of `program` for this TPM, with no input and its standard
    /// output left out, ended after [`TIMEOUT`]. Its standard error is
    /// kept, for the log when it fails, and read as a secret is: a tool
    /// could echo the authorization value it was given.
    fn command(&self, program: &str) -> Command {
        Command::new(program)
            .env("TPM2TOOLS_TCTI", self.tcti)
            .timeout(TIMEOUT)
            .stdin(Input::Null)
            .stdout(Stream::Null)
            .stderr(Stream::CaptureSecret)
    }

    /// The output of the `tool` command `command` when it succeeds. When it
    /// does not, the log says why at debug: the last lines of its standard
    /// error, without the authorization value `auth` it was given.
    fn output(&self, tool: &str, command: &Command, auth: Option<&Secret>) -> Option<Output> {
        let mut output = self
            .runner
            .run(command)
            .inspect_err(|error| {
                zbm_trace::debug!("{program} did not start: {error}"; program = tool, %error);
            })
            .ok()?;
        // Wiped when dropped: a tool that echoed its input left it here, in
        // the only copy the runner made (`Stream::CaptureSecret`).
        let said = Secret::from_vec(std::mem::take(&mut output.stderr));
        if output.status.is_success() {
            return Some(output);
        }
        let status = output.status.shell_code();
        let text = String::from_utf8_lossy(scrub(said.expose(), auth).expose()).into_owned();
        if let Some(said) = stderr::last_lines(&text) {
            zbm_trace::debug!("{program} failed ({status}): {said}"; program = tool, status, %said);
        } else {
            zbm_trace::debug!("{program} failed ({status})"; program = tool, status);
        }
        None
    }

    /// Runs the `tool` command `command`.
    fn run(&self, tool: &'static str, command: &Command) -> Result<(), ToolFailed> {
        self.output(tool, command, None)
            .map(drop)
            .ok_or(ToolFailed(tool))
    }

    /// Whether the TPM refuses PIN-protected objects after too many wrong
    /// PINs: `inLockout: 1` in `tpm2_getcap properties-variable`.
    pub fn in_lockout(&self) -> bool {
        let getcap = self
            .command("tpm2_getcap")
            .arg("properties-variable")
            .stdout(Stream::Capture);
        let Some(output) = self.output("tpm2_getcap", &getcap, None) else {
            return false;
        };
        output.stdout_text().lines().any(|line| {
            line.split_once("inLockout:")
                .is_some_and(|(_, value)| value.trim_start_matches(' ').starts_with('1'))
        })
    }

    /// The PCR banks the TPM has active, with PCRs selected in them
    /// (`tpm2_getcap pcrs`): `selected-pcrs:` and a line `- sha256: [ 0, 1,
    /// ... ]` for each; `None` when the TPM does not answer.
    pub fn active_banks(&self) -> Option<Vec<Bank>> {
        let getcap = self
            .command("tpm2_getcap")
            .arg("pcrs")
            .stdout(Stream::Capture);
        let output = self.output("tpm2_getcap", &getcap, None)?;
        let banks = output
            .stdout_text()
            .lines()
            .filter_map(|line| {
                let (name, pcrs) = line.trim().strip_prefix("- ")?.split_once(':')?;
                let pcrs = pcrs.trim().trim_start_matches('[').trim_end_matches(']');
                Bank::named(name.trim()).filter(|_| !pcrs.trim().is_empty())
            })
            .collect();
        Some(banks)
    }

    /// The values of `pcrs` in each of `banks` (`tpm2_pcrread
    /// sha256:8,9`), as [`Digests`] by PCR; `None` when the TPM does not
    /// give them all.
    pub fn read_pcrs(&self, pcrs: &[u32], banks: &[Bank]) -> Option<Vec<(u32, Digests)>> {
        let list: Vec<String> = pcrs.iter().map(u32::to_string).collect();
        let selection: Vec<String> = banks
            .iter()
            .map(|bank| format!("{}:{}", bank.name(), list.join(",")))
            .collect();
        let read = self
            .command("tpm2_pcrread")
            .arg(selection.join("+"))
            .stdout(Stream::Capture);
        let output = self.output("tpm2_pcrread", &read, None)?;
        let values = pcr_values(&output.stdout_text());
        pcrs.iter()
            .map(|&pcr| {
                let digests = banks
                    .iter()
                    .map(|&bank| {
                        let value = values.iter().find(|(b, p, _)| *b == bank && *p == pcr)?;
                        Some((bank, value.2.clone()))
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some((pcr, Digests::new(digests)))
            })
            .collect()
    }

    /// Extends `pcr` in each of `banks` with its digest of `digests`
    /// (`tpm2_pcrextend 9:sha1=...,sha256=...`).
    ///
    /// # Errors
    ///
    /// When `tpm2_pcrextend` fails.
    pub fn extend(&self, pcr: u32, digests: &Digests, banks: &[Bank]) -> Result<(), ToolFailed> {
        let values: Vec<String> = banks
            .iter()
            .filter_map(|&bank| {
                digests
                    .get(bank)
                    .map(|digest| format!("{}={}", bank.name(), measure::hex(digest)))
            })
            .collect();
        let extend = self
            .command("tpm2_pcrextend")
            .arg(format!("{pcr}:{}", values.join(",")));
        self.run("tpm2_pcrextend", &extend)
    }

    /// Loads a sealed object below the storage root key into `context`
    /// (`tpm2_load`).
    ///
    /// # Errors
    ///
    /// [`ToolFailed`] when `tpm2_load` fails.
    pub fn load(&self, public: &Path, private: &Path, context: &Path) -> Result<(), ToolFailed> {
        let load = self.command("tpm2_load").args([
            "-C",
            SRK_HANDLE,
            "-u",
            &text(public),
            "-r",
            &text(private),
            "-c",
            &text(context),
        ]);
        self.run("tpm2_load", &load)
    }

    /// Starts a policy session saved in `context`, salted with the storage
    /// root key, whose responses are encrypted
    /// (`tpm2_startauthsession --policy-session -c`, then
    /// `tpm2_sessionconfig --enable-encrypt`).
    ///
    /// # Errors
    ///
    /// [`ToolFailed`] when either tool fails: an unsealed key is not read
    /// over a session that would carry it in the clear.
    pub fn policy_session(&self, context: &Path) -> Result<PolicySession<'_, 'a>, ToolFailed> {
        let start = self.command("tpm2_startauthsession").args([
            "--policy-session",
            "-c",
            SRK_HANDLE,
            "-S",
            &text(context),
        ]);
        self.run("tpm2_startauthsession", &start)?;
        // From here the session is flushed however this ends.
        let session = PolicySession {
            tpm: self,
            context: context.to_path_buf(),
        };
        let encrypt = self
            .command("tpm2_sessionconfig")
            .args([&text(context), "--enable-encrypt"]);
        self.run("tpm2_sessionconfig", &encrypt)?;
        Ok(session)
    }

    /// Flushes every transient object (`tpm2_flushcontext -t`).
    pub fn flush_transient(&self) {
        let flush = self.command("tpm2_flushcontext").arg("-t");
        let _ = self.output("tpm2_flushcontext", &flush, None);
    }
}

/// A policy session of the TPM, flushed when it is dropped.
pub struct PolicySession<'t, 'a> {
    tpm: &'t Tpm2<'a>,
    context: PathBuf,
}

impl PolicySession<'_, '_> {
    /// Adds the PCR values of `selection`, such as `sha256:0,7`, to the
    /// policy (`tpm2_policypcr`).
    ///
    /// # Errors
    ///
    /// [`ToolFailed`] when `tpm2_policypcr` fails.
    pub fn pcrs(&self, selection: &str) -> Result<(), ToolFailed> {
        let policy =
            self.tpm
                .command("tpm2_policypcr")
                .args(["-S", &text(&self.context), "-l", selection]);
        self.tpm.run("tpm2_policypcr", &policy)
    }

    /// Adds the object's authorization value to the policy
    /// (`tpm2_policyauthvalue`).
    ///
    /// # Errors
    ///
    /// [`ToolFailed`] when `tpm2_policyauthvalue` fails.
    pub fn auth_value(&self) -> Result<(), ToolFailed> {
        let policy = self
            .tpm
            .command("tpm2_policyauthvalue")
            .args(["-S", &text(&self.context)]);
        self.tpm.run("tpm2_policyauthvalue", &policy)
    }

    /// Unseals the loaded `object` under this session's policy, with the
    /// authorization value `auth` (hexadecimal, on standard input) when it
    /// needs one; `None` when the TPM refuses.
    pub fn unseal(&self, object: &Path, auth: Option<&Secret>) -> Option<Secret> {
        let mut policy = format!("session:{}", text(&self.context));
        let mut stdin = Input::Null;
        if let Some(auth) = auth {
            policy.push_str("+file:-");
            let mut value = Secret::copy_from(b"hex:");
            value.push_bytes(auth.expose());
            stdin = Input::secret(value);
        }
        let unseal = self
            .tpm
            .command("tpm2_unseal")
            .args(["-c", &text(object), "-p", &policy])
            .stdin(stdin)
            .stdout(Stream::CaptureSecret);
        let output = self.tpm.output("tpm2_unseal", &unseal, auth)?;
        Some(output.into_secret().unwrap_or_default())
    }
}

impl Drop for PolicySession<'_, '_> {
    fn drop(&mut self) {
        let flush = self
            .tpm
            .command("tpm2_flushcontext")
            .arg(text(&self.context));
        let _ = self.tpm.output("tpm2_flushcontext", &flush, None);
    }
}

#[cfg(test)]
mod tests;
