//! libfido2's tools: listing security keys (`fido2-token -L`) and asking one
//! for the hmac-secret of a credential (`fido2-assert -G -h`), which a
//! systemd-fido2 token turns into the LUKS passphrase.
//!
//! The secret is captured from `fido2-assert`'s output and never written to
//! a file; it is the last line of the output.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use zbm_platform::{Command, Input, Runner, Secret, Stream};

/// What a systemd-fido2 token asks of the security key.
#[derive(Clone, Copy, Debug)]
pub struct Assertion<'t> {
    /// The relying party, such as `io.systemd.cryptsetup`.
    pub rp: &'t str,
    /// The credential ID, in base64.
    pub credential: &'t str,
    /// The salt of the hmac-secret, in base64.
    pub salt: &'t str,
    /// Whether the user must touch the key (`up`).
    pub presence: bool,
    /// Whether the key must verify the user (`uv`).
    pub verification: bool,
    /// Whether the key asks for its PIN (`pin`).
    pub pin: bool,
}

/// The devices in `fido2-token -L` output: the part before the first `:` of
/// each line that names a device below `/dev/`.
pub fn listed_devices(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (device, _) = line.split_once(':')?;
            device.starts_with("/dev/").then(|| device.to_owned())
        })
        .collect()
}

/// The last line of `fido2-assert`'s output, without newlines, as `tail -n 1`
/// and `$(...)` leave it.
pub fn last_line(output: &[u8]) -> Secret {
    let body = output.strip_suffix(b"\n").unwrap_or(output);
    let start = body
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |at| at + 1);
    Secret::copy_from(body.get(start..).unwrap_or_default())
}

/// libfido2's tools, run through a [`Runner`].
#[derive(Clone, Copy)]
pub struct Fido2<'a> {
    runner: &'a dyn Runner,
}

impl<'a> Fido2<'a> {
    /// The tools, run through `runner`.
    pub fn new(runner: &'a dyn Runner) -> Fido2<'a> {
        Fido2 { runner }
    }

    /// The connected security keys (`fido2-token -L`).
    pub fn devices(&self) -> Vec<String> {
        let list = Command::new("fido2-token")
            .arg("-L")
            .stdin(Input::Null)
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        let listing = self
            .runner
            .run(&list)
            .map(|output| output.stdout_text())
            .unwrap_or_default();
        listed_devices(&listing)
    }

    /// The hmac-secret that `device` computes for `assertion` and the
    /// client data hash `challenge` (`fido2-assert -G -h`), or `None` when it
    /// produces none.
    pub fn hmac_secret(
        &self,
        device: &str,
        assertion: Assertion<'_>,
        challenge: &Secret,
    ) -> Option<Secret> {
        let flag = |on: bool| if on { "true" } else { "false" };
        let mut command = Command::new("fido2-assert").args([
            "-G".to_owned(),
            "-h".to_owned(),
            "-t".to_owned(),
            format!("up={}", flag(assertion.presence)),
            "-t".to_owned(),
            format!("uv={}", flag(assertion.verification)),
        ]);
        if assertion.pin {
            command = command.args(["-t", "pin=true"]);
        }
        let input = format!(
            "{}\n{}\n{}\n{}\n",
            String::from_utf8_lossy(challenge.expose()),
            assertion.rp,
            assertion.credential,
            assertion.salt
        );
        let assert = command
            .arg(device)
            .stdin(Input::Bytes(input.into_bytes()))
            .stdout(Stream::CaptureSecret)
            .stderr(Stream::Null);
        let output = self.runner.run(&assert).ok()?;
        let hmac = last_line(output.into_secret().unwrap_or_default().expose());
        (!hmac.is_empty()).then_some(hmac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_platform::{Output, Status};
    use zbm_testkit::FakeRunner;

    fn assertion(pin: bool) -> Assertion<'static> {
        Assertion {
            rp: "io.systemd.cryptsetup",
            credential: "Y3JlZA==",
            salt: "c2FsdA==",
            presence: true,
            verification: false,
            pin,
        }
    }

    #[test]
    fn devices_are_listed_and_asked_for_the_secret() {
        let runner = FakeRunner::new();
        runner.respond(
            "fido2-token -L",
            0,
            "/dev/hidraw3: vendor=0x1050, product=0x0407 (Yubico YubiKey)\nnoise\n",
        );
        runner.on(|command| {
            (command.program() == "fido2-assert").then(|| {
                let output = b"cdh\nrp\nAUTHDATA\nSIG\naG1hYw==\n";
                Ok(Output::with_secret(
                    Status::exited(0),
                    Secret::copy_from(output),
                ))
            })
        });
        let fido2 = Fido2::new(&runner);

        assert_eq!(fido2.devices(), ["/dev/hidraw3"]);
        let secret =
            fido2.hmac_secret("/dev/hidraw3", assertion(true), &Secret::copy_from(b"Q0RI"));
        assert_eq!(secret.unwrap().expose(), b"aG1hYw==");
        let call = &runner.calls()[1];
        assert_eq!(
            call.display(),
            "fido2-assert -G -h -t up=true -t uv=false -t pin=true /dev/hidraw3"
        );
        assert_eq!(
            *call.get_stdin(),
            Input::Bytes(b"Q0RI\nio.systemd.cryptsetup\nY3JlZA==\nc2FsdA==\n".to_vec())
        );
    }

    #[test]
    fn no_output_is_no_secret() {
        let runner = FakeRunner::new();
        runner.respond_program("fido2-assert", 1, "");
        let fido2 = Fido2::new(&runner);

        assert!(
            fido2
                .hmac_secret("/dev/hidraw0", assertion(false), &Secret::copy_from(b"x"))
                .is_none()
        );
        assert_eq!(
            runner.lines(),
            ["fido2-assert -G -h -t up=true -t uv=false /dev/hidraw0"]
        );
        assert_eq!(last_line(b"a\nb\n\n").expose(), b"");
        assert_eq!(last_line(b"only").expose(), b"only");
    }
}
