//! The YubiKey personalization tools: whether a YubiKey is connected
//! (`ykinfo -s`) and its HMAC-SHA1 response to a challenge
//! (`ykchalresp -<slot> -i-`). The challenge goes to standard input and the
//! response is captured as a secret.

use zbm_platform::{Command, Input, Runner, Secret, Stream};

/// `ykinfo` and `ykchalresp`, run through a [`Runner`].
#[derive(Clone, Copy)]
pub struct YubiKey<'a> {
    runner: &'a dyn Runner,
}

impl<'a> YubiKey<'a> {
    /// The tools, run through `runner`.
    pub fn new(runner: &'a dyn Runner) -> YubiKey<'a> {
        YubiKey { runner }
    }

    /// Whether `ykinfo -s` finds a YubiKey.
    pub fn is_present(&self) -> bool {
        let command = Command::new("ykinfo")
            .arg("-s")
            .stdin(Input::Null)
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        self.runner
            .run(&command)
            .is_ok_and(|output| output.status.is_success())
    }

    /// The response of the YubiKey's `slot` to `challenge`, without final
    /// newlines; `None` when it gives none.
    pub fn response(&self, slot: u8, challenge: &Secret) -> Option<Secret> {
        let command = Command::new("ykchalresp")
            .args([format!("-{slot}"), "-i-".to_owned()])
            .stdin(Input::Secret(challenge.clone()))
            .stdout(Stream::CaptureSecret)
            .stderr(Stream::Null);
        self.runner
            .run(&command)
            .ok()
            .filter(|output| output.status.is_success())
            .and_then(|output| output.secret)
            .map(|mut response| {
                while response.expose().last() == Some(&b'\n') {
                    response.pop();
                }
                response
            })
            .filter(|response| !response.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_platform::{Output, Status};
    use zbm_testkit::FakeRunner;

    #[test]
    fn the_challenge_goes_in_and_the_response_comes_out_as_secrets() {
        let runner = FakeRunner::new();
        runner.on(|command| {
            let challenge =
                matches!(command.get_stdin(), Input::Secret(c) if c.expose() == b"pass");
            (command.program() == "ykchalresp").then(|| {
                Ok(if challenge {
                    Output::with_secret(Status::exited(0), Secret::copy_from(b"3f2a\n\n"))
                } else {
                    Output::new(Status::exited(1), "")
                })
            })
        });
        let yubikey = YubiKey::new(&runner);

        assert!(yubikey.is_present());
        assert_eq!(
            yubikey
                .response(2, &Secret::copy_from(b"pass"))
                .unwrap()
                .expose(),
            b"3f2a"
        );
        assert!(yubikey.response(2, &Secret::copy_from(b"other")).is_none());
        assert_eq!(
            runner.lines(),
            ["ykinfo -s", "ykchalresp -2 -i-", "ykchalresp -2 -i-"]
        );
        let absent = FakeRunner::new();
        absent.respond_program("ykinfo", 1, "");
        assert!(!YubiKey::new(&absent).is_present());
    }
}
