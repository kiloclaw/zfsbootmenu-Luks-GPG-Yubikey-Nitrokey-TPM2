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

    /// Standard output of `command` with `input`, or nothing.
    fn filter(&self, command: Command, input: Vec<u8>) -> Vec<u8> {
        let command = command
            .stdin(Input::Bytes(input))
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        self.runner
            .run(&command)
            .map(|output| output.stdout)
            .unwrap_or_default()
    }

    /// The key that the clevis `token` protects, as `jose fmt -j- -Og jwe
    /// -o- | jose jwe fmt -i- -c | clevis decrypt` recovers it; `None` when
    /// it cannot.
    pub fn decrypt(&self, token: &Token) -> Option<Secret> {
        let jwe = self.filter(
            Command::new("jose").args(["fmt", "-j-", "-Og", "jwe", "-o-"]),
            format!("{}\n", token.to_json()).into_bytes(),
        );
        let compact = self.filter(Command::new("jose").args(["jwe", "fmt", "-i-", "-c"]), jwe);
        let decrypt = Command::new("clevis")
            .arg("decrypt")
            .timeout(TIMEOUT)
            .env("TPM2TOOLS_TCTI", self.tcti)
            .stdin(Input::Bytes(compact))
            .stdout(Stream::CaptureSecret)
            .stderr(Stream::Null);
        self.runner
            .run(&decrypt)
            .ok()
            .filter(|output| output.status.is_success())
            .and_then(zbm_platform::Output::into_secret)
            .filter(|key| !key.is_empty())
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

    #[test]
    fn a_failed_or_empty_decryption_is_no_key() {
        let token = Token::without_fields("0", "clevis");
        let failing = FakeRunner::new();
        failing.respond_program("clevis", 1, "");
        assert!(Clevis::new(&failing, "x").decrypt(&token).is_none());
        let empty = FakeRunner::new();
        empty.on(|command| {
            (command.program() == "clevis").then(|| {
                Ok(Output::with_secret(
                    Status::exited(0),
                    Secret::copy_from(b""),
                ))
            })
        });
        assert!(Clevis::new(&empty, "x").decrypt(&token).is_none());
    }

    #[test]
    fn decrypting_ends_after_its_time_out() {
        let runner = FakeRunner::new();
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
