//! clevis and jose: recovering the key of a clevis token.
//!
//! The token's JWE is turned into its compact form with `jose` and decrypted
//! with `clevis decrypt`, whose pins (`tpm2`, `tang`, `sss`) do the rest; the
//! key is captured as a secret and never written to a file.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::time::Duration;
use zbm_platform::{Command, Input, Runner, Secret, Stream};

use crate::token::Token;

/// The longest `clevis decrypt` may run: a Tang server over the network, or
/// a TPM, that stopped answering.
const TIMEOUT: Duration = Duration::from_secs(120);

/// clevis and jose, run through a [`Runner`].
#[derive(Clone, Copy)]
pub struct Clevis<'a> {
    runner: &'a dyn Runner,
    tcti: &'a str,
}

impl<'a> Clevis<'a> {
    /// The programs, run through `runner`; a `tpm2` pin uses the TPM
    /// `tcti`.
    pub fn new(runner: &'a dyn Runner, tcti: &'a str) -> Clevis<'a> {
        Clevis { runner, tcti }
    }

    /// Standard output of `command` with `input`, or why it gave none.
    fn filter(&self, command: Command, input: Vec<u8>) -> Result<Vec<u8>, String> {
        let program = command.program().to_string_lossy().into_owned();
        let command = command
            .stdin(Input::Bytes(input))
            .stdout(Stream::Capture)
            .stderr(Stream::Capture);
        let output = self
            .runner
            .run(&command)
            .map_err(|error| format!("{program} did not start: {error}"))?;
        if !output.status.is_success() || output.stdout.is_empty() {
            return Err(failure(&program, &output));
        }
        Ok(output.stdout)
    }

    /// The key that the clevis `token` protects, as `jose fmt -j- -Og jwe
    /// -o- | jose jwe fmt -i- -c | clevis decrypt` recovers it.
    ///
    /// # Errors
    ///
    /// Why it could not: what the program that failed said, its last line
    /// first, so that the log can say it.
    pub fn decrypt(&self, token: &Token) -> Result<Secret, String> {
        let jwe = self.filter(
            Command::new("jose").args(["fmt", "-j-", "-Og", "jwe", "-o-"]),
            format!("{}\n", token.to_json()).into_bytes(),
        )?;
        let compact = self.filter(Command::new("jose").args(["jwe", "fmt", "-i-", "-c"]), jwe)?;
        let decrypt = Command::new("clevis")
            .arg("decrypt")
            .timeout(TIMEOUT)
            .env("TPM2TOOLS_TCTI", self.tcti)
            .stdin(Input::Bytes(compact))
            .stdout(Stream::CaptureSecret)
            .stderr(Stream::Capture);
        let output = self
            .runner
            .run(&decrypt)
            .map_err(|error| format!("clevis did not start: {error}"))?;
        if !output.status.is_success() {
            return Err(failure("clevis decrypt", &output));
        }
        output
            .into_secret()
            .filter(|key| !key.is_empty())
            .ok_or_else(|| "clevis decrypt printed no key".to_owned())
    }
}

/// What `program` said when it failed: the last line of its standard
/// error, where scripts put the reason, and its exit status.
fn failure(program: &str, output: &zbm_platform::Output) -> String {
    let text = output.stderr_text();
    let said = text.lines().rev().find(|line| !line.trim().is_empty());
    let status = output.status.shell_code();
    match said {
        Some(line) => format!("{program} failed ({status}): {}", line.trim()),
        None => format!("{program} failed ({status})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_platform::{Output, Status};
    use zbm_testkit::FakeRunner;

    #[test]
    fn the_jwe_goes_through_jose_to_clevis() {
        let runner = FakeRunner::new();
        runner.on(|command| {
            Some(Ok(if command.program() == "clevis" {
                Output::with_secret(Status::exited(0), Secret::copy_from(b"key"))
            } else {
                Output::new(Status::exited(0), "step\n")
            }))
        });
        let token =
            Token::from_export("2", r#"{"type":"clevis","keyslots":["3"],"jwe":{"p":"x"}}"#)
                .unwrap();

        let key = Clevis::new(&runner, "device:/dev/tpmrm0").decrypt(&token);

        assert_eq!(key.unwrap().expose(), b"key");
        assert_eq!(
            runner.lines(),
            [
                "jose fmt -j- -Og jwe -o-",
                "jose jwe fmt -i- -c",
                "clevis decrypt"
            ]
        );
        let calls = runner.calls();
        assert_eq!(
            *calls[0].get_stdin(),
            Input::Bytes(format!("{}\n", token.to_json()).into_bytes())
        );
        assert_eq!(*calls[2].get_stdin(), Input::Bytes(b"step\n".to_vec()));
    }

    /// A runner whose `jose` prints a line and whose `clevis` gives
    /// `clevis`'s answer.
    fn with_clevis(clevis: impl Fn() -> Output + Send + Sync + 'static) -> FakeRunner {
        let runner = FakeRunner::new();
        runner.on(move |command| {
            Some(Ok(if command.program() == "clevis" {
                clevis()
            } else {
                Output::new(Status::exited(0), "step\n")
            }))
        });
        runner
    }

    #[test]
    fn a_failed_or_empty_decryption_says_why() {
        let token = Token::without_fields("0", "clevis");
        let failing = with_clevis(|| {
            let mut output = Output::new(Status::exited(1), "");
            output.stderr = b"Creating TPM2 primary key failed!\nERROR: tpm2\n\n".to_vec();
            output
        });
        assert_eq!(
            Clevis::new(&failing, "x").decrypt(&token).err().as_deref(),
            Some("clevis decrypt failed (1): ERROR: tpm2")
        );
        let empty = with_clevis(|| Output::with_secret(Status::exited(0), Secret::copy_from(b"")));
        assert_eq!(
            Clevis::new(&empty, "x").decrypt(&token).err().as_deref(),
            Some("clevis decrypt printed no key")
        );
        // jose that cannot read the token: clevis is not run.
        let bad_jose = FakeRunner::new();
        bad_jose.on(|command| {
            (command.program() == "jose").then(|| {
                let mut said = Output::new(Status::exited(1), "");
                said.stderr = b"Invalid json!\n".to_vec();
                Ok(said)
            })
        });
        assert_eq!(
            Clevis::new(&bad_jose, "x").decrypt(&token).err().as_deref(),
            Some("jose failed (1): Invalid json!")
        );
        assert!(
            !bad_jose
                .lines()
                .iter()
                .any(|line| line.starts_with("clevis"))
        );
    }

    #[test]
    fn a_program_that_does_not_start_or_says_nothing_is_named() {
        let token = Token::without_fields("0", "clevis");
        let missing = FakeRunner::new();
        missing.on(|command| {
            (command.program() == "jose").then(|| Err(std::io::Error::other("no such file")))
        });
        assert_eq!(
            Clevis::new(&missing, "x").decrypt(&token).err().as_deref(),
            Some("jose did not start: no such file")
        );
        let no_clevis = FakeRunner::new();
        no_clevis.on(|command| {
            Some(if command.program() == "clevis" {
                Err(std::io::Error::other("no such file"))
            } else {
                Ok(Output::new(Status::exited(0), "step\n"))
            })
        });
        assert_eq!(
            Clevis::new(&no_clevis, "x")
                .decrypt(&token)
                .err()
                .as_deref(),
            Some("clevis did not start: no such file")
        );
        let silent = with_clevis(|| Output::new(Status::exited(2), ""));
        assert_eq!(
            Clevis::new(&silent, "x").decrypt(&token).err().as_deref(),
            Some("clevis decrypt failed (2)")
        );
    }

    #[test]
    fn decrypting_ends_after_its_time_out() {
        let runner = with_clevis(|| Output::new(Status::exited(1), ""));
        let token = Token::without_fields("0", "clevis");
        let _ = Clevis::new(&runner, "device:/dev/tpmrm0").decrypt(&token);
        let calls = runner.calls();
        let decrypt = calls
            .iter()
            .find(|call| call.program() == "clevis")
            .unwrap();
        assert_eq!(decrypt.get_timeout(), Some(TIMEOUT));
    }
}
