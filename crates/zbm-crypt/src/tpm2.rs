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

use zbm_platform::{Command, Input, Runner, Secret, Stream};

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

/// tpm2-tools for one TPM, run through a [`Runner`].
#[derive(Clone, Copy)]
pub struct Tpm2<'a> {
    runner: &'a dyn Runner,
    tcti: &'a str,
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

    /// A command of `program` for this TPM, quiet and with no input, ended
    /// after [`TIMEOUT`].
    fn command(&self, program: &str) -> Command {
        Command::new(program)
            .env("TPM2TOOLS_TCTI", self.tcti)
            .timeout(TIMEOUT)
            .stdin(Input::Null)
            .stdout(Stream::Null)
            .stderr(Stream::Null)
    }

    /// Runs the `tool` command `command`.
    fn run(&self, tool: &'static str, command: &Command) -> Result<(), ToolFailed> {
        match self.runner.run(command) {
            Ok(output) if output.status.is_success() => Ok(()),
            _ => Err(ToolFailed(tool)),
        }
    }

    /// Whether the TPM refuses PIN-protected objects after too many wrong
    /// PINs: `inLockout: 1` in `tpm2_getcap properties-variable`.
    pub fn in_lockout(&self) -> bool {
        let getcap = self
            .command("tpm2_getcap")
            .arg("properties-variable")
            .stdout(Stream::Capture);
        let Ok(output) = self.runner.run(&getcap) else {
            return false;
        };
        output.stdout_text().lines().any(|line| {
            line.split_once("inLockout:")
                .is_some_and(|(_, value)| value.trim_start_matches(' ').starts_with('1'))
        })
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
        let _ = self
            .runner
            .run(&self.command("tpm2_flushcontext").arg("-t"));
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
        let output = self.tpm.runner.run(&unseal).ok()?;
        output
            .status
            .is_success()
            .then(|| output.into_secret().unwrap_or_default())
    }
}

impl Drop for PolicySession<'_, '_> {
    fn drop(&mut self) {
        let flush = self
            .tpm
            .command("tpm2_flushcontext")
            .arg(text(&self.context));
        let _ = self.tpm.runner.run(&flush);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_platform::{Output, Status};
    use zbm_testkit::FakeRunner;

    #[test]
    fn a_session_is_flushed_on_every_path() {
        let runner = FakeRunner::new();
        runner.respond_program("tpm2_unseal", 1, "");
        let tpm = Tpm2::new(&runner, "swtpm:port=2321");
        let work = Path::new("/w");
        {
            let session = tpm.policy_session(&work.join("s.ctx")).unwrap();
            session.auth_value().unwrap();
            assert!(session.unseal(&work.join("o.ctx"), None).is_none());
        }
        tpm.flush_transient();

        assert_eq!(
            runner.lines(),
            [
                "tpm2_startauthsession --policy-session -c 0x81000001 -S /w/s.ctx",
                "tpm2_sessionconfig /w/s.ctx --enable-encrypt",
                "tpm2_policyauthvalue -S /w/s.ctx",
                "tpm2_unseal -c /w/o.ctx -p session:/w/s.ctx",
                "tpm2_flushcontext /w/s.ctx",
                "tpm2_flushcontext -t",
            ]
        );
        assert!(runner.calls().iter().all(|command| {
            command.get_env().iter().any(|(key, value)| {
                key == "TPM2TOOLS_TCTI" && value.as_deref() == Some("swtpm:port=2321".as_ref())
            })
        }));
    }

    #[test]
    fn the_authorization_value_goes_to_standard_input() {
        let runner = FakeRunner::new();
        runner.on(|command| {
            (command.program() == "tpm2_unseal").then(|| {
                let given = matches!(
                    command.get_stdin(),
                    Input::Secret(secret) if secret.expose() == b"hex:abcd"
                );
                Ok(if given {
                    Output::with_secret(Status::exited(0), Secret::copy_from(b"key"))
                } else {
                    Output::new(Status::exited(1), "")
                })
            })
        });
        let tpm = Tpm2::new(&runner, "device:/dev/tpmrm0");
        let session = tpm.policy_session(Path::new("/s")).unwrap();

        let key = session.unseal(Path::new("/o"), Some(&Secret::copy_from(b"abcd")));
        assert_eq!(key.unwrap().expose(), b"key");
        assert!(runner.lines()[2].ends_with("-p session:/s+file:-"));
        assert!(
            session
                .unseal(Path::new("/o"), Some(&Secret::copy_from(b"0000")))
                .is_none()
        );
    }

    #[test]
    fn a_session_that_cannot_encrypt_is_flushed_and_refused() {
        let runner = FakeRunner::new();
        runner.respond_program("tpm2_sessionconfig", 1, "");
        let tpm = Tpm2::new(&runner, "x");

        let refused = tpm.policy_session(Path::new("/s")).err();

        assert_eq!(refused, Some(ToolFailed("tpm2_sessionconfig")));
        assert_eq!(
            runner.lines().last().map(String::as_str),
            Some("tpm2_flushcontext /s")
        );
    }

    #[test]
    fn lockout_is_read_from_the_variable_properties() {
        let runner = FakeRunner::new();
        runner.respond(
            "tpm2_getcap properties-variable",
            0,
            "TPM2_PT_PERMANENT:\n  inLockout:                 1\n",
        );
        assert!(Tpm2::new(&runner, "x").in_lockout());
        let clear = FakeRunner::new();
        clear.respond("tpm2_getcap properties-variable", 0, "  inLockout: 0\n");
        assert!(!Tpm2::new(&clear, "x").in_lockout());
        let failing = FakeRunner::new();
        failing.on(|_| Some(Err(std::io::Error::other("no tool"))));
        let tpm = Tpm2::new(&failing, "x");
        assert!(!tpm.in_lockout());
        assert_eq!(
            tpm.policy_session(Path::new("/s")).err(),
            Some(ToolFailed("tpm2_startauthsession"))
        );
        assert_eq!(
            tpm.load(Path::new("/p"), Path::new("/r"), Path::new("/c")),
            Err(ToolFailed("tpm2_load"))
        );
    }

    #[test]
    fn every_command_ends_after_its_time_out() {
        let runner = FakeRunner::new();
        let _ = Tpm2::new(&runner, "device:/dev/tpmrm0").in_lockout();
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].get_timeout(), Some(TIMEOUT));
    }
}
