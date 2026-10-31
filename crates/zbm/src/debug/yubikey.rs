//! `zbm debug yubikey`: the YubiKeys ZFSBootMenu reaches over their OTP
//! interface (ROADMAP.md SB11), and a check of its challenge-response
//! against `ykchalresp` on a real key (`zbm_luks::keys::yubikey::check`).
//!
//! - `zbm debug yubikey` lists the keys' `hidraw` devices and their serial
//!   numbers;
//! - `zbm debug yubikey SLOT CHALLENGE` prints the response of slot 1 or 2
//!   of the first key to the test challenge `CHALLENGE`, and whether
//!   `ykchalresp`, where the image has it, gives the same. A response to a
//!   real passphrase is a LUKS key: give a challenge of no volume's.

use zbm_luks::keys::yubikey::check::{self, Agreement, Checked};
use zbm_platform::{Context, Secret};

use crate::{Io, USAGE};

/// The usage text.
pub const USAGE_TEXT: &str = "\
Usage: zbm debug yubikey                  the YubiKeys and their serial numbers
       zbm debug yubikey SLOT CHALLENGE   slot 1 or 2's response to a test challenge,
                                          beside ykchalresp's
";

/// What is said when no key is there.
const NO_KEY: &str = "zbm debug yubikey: no YubiKey among the hidraw devices";

/// `zbm debug yubikey` on `context`.
pub fn run_in(context: &Context<'_>, args: &[&str], io: &mut Io) -> i32 {
    match args {
        [] => list(context, io),
        [slot @ ("1" | "2"), challenge] => {
            let slot = u8::from(*slot == "2") + 1;
            respond(context, slot, &Secret::copy_from(challenge.as_bytes()), io)
        }
        _ => {
            let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
            USAGE
        }
    }
}

/// The keys and their serial numbers.
fn list(context: &Context<'_>, io: &mut Io) -> i32 {
    let found = check::found_keys(context);
    if found.is_empty() {
        let _ = writeln!(io.stderr, "{NO_KEY}");
        return 1;
    }
    for key in found {
        let serial = match key.serial {
            Ok(serial) => format!("serial {serial}"),
            Err(error) => format!("no serial number ({error})"),
        };
        let _ = writeln!(io.stdout, "{}  {serial}", key.device.display());
    }
    0
}

/// Slot `slot`'s response to `challenge`, beside `ykchalresp`'s.
fn respond(context: &Context<'_>, slot: u8, challenge: &Secret, io: &mut Io) -> i32 {
    match check::check_response(context, slot, challenge) {
        Checked::NoKey => {
            let _ = writeln!(io.stderr, "{NO_KEY}");
            1
        }
        Checked::Failed(device, error) => {
            let _ = writeln!(
                io.stderr,
                "zbm debug yubikey: {}: {error}",
                device.display()
            );
            1
        }
        Checked::Answered(device, response, agreement) => {
            let text = String::from_utf8_lossy(response.expose()).into_owned();
            let _ = writeln!(io.stdout, "{}  slot {slot}  {text}", device.display());
            match agreement {
                None => 0,
                Some(Agreement::Same) => {
                    let _ = writeln!(io.stdout, "ykchalresp gives the same");
                    0
                }
                Some(Agreement::Different) => {
                    let _ = writeln!(io.stdout, "ykchalresp gives another response, or none");
                    1
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_platform::{Output, Status};
    use zbm_testkit::fakes::{FakeSystem, FakeYubiKey};

    use super::*;

    fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        let code = run_in(&fake.context(), args, &mut io);
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
        (code, text(stdout), text(stderr))
    }

    /// A key whose slots answer with 20 bytes of the challenge's length.
    fn key() -> FakeYubiKey {
        FakeYubiKey::new(|_, challenge| Some(vec![u8::try_from(challenge.len()).unwrap(); 20]))
    }

    #[test]
    fn the_keys_are_listed_with_their_serial_numbers() {
        let fake = FakeSystem::new();
        assert_eq!(call(&fake, &[]).0, 1);
        fake.plug_yubikey("hidraw0", key().with_serial(99));
        fake.plug_yubikey("hidraw2", key());

        let (code, out, _) = call(&fake, &[]);

        assert_eq!(code, 0);
        let second = "/dev/hidraw2  no serial number (the YubiKey gave no response)";
        assert_eq!(out, format!("/dev/hidraw0  serial 99\n{second}\n"));
    }

    #[test]
    fn a_response_is_printed_beside_ykchalresp_s() {
        let fake = FakeSystem::new();
        assert_eq!(call(&fake, &["2", "test"]).0, 1);
        fake.plug_yubikey("hidraw0", key());
        let response = "04".repeat(20);

        let (code, out, _) = call(&fake, &["2", "test"]);
        assert_eq!(code, 0);
        assert_eq!(out, format!("/dev/hidraw0  slot 2  {response}\n"));

        fake.environment.set("PATH", "/bin");
        fake.files.file_with_mode("/bin/ykchalresp", "", 0o755);
        fake.runner.on(move |command| {
            let response = Secret::copy_from(response.as_bytes());
            (command.program() == "ykchalresp")
                .then(|| Ok(Output::with_secret(Status::exited(0), response)))
        });
        let (code, out, _) = call(&fake, &["2", "test"]);
        assert_eq!(code, 0);
        assert!(out.ends_with("ykchalresp gives the same\n"), "{out}");
        let (code, out, _) = call(&fake, &["1", "other"]);
        assert_eq!(code, 1);
        assert!(
            out.ends_with("ykchalresp gives another response, or none\n"),
            "{out}"
        );
    }

    #[test]
    fn a_key_that_gives_nothing_and_wrong_arguments_are_said_so() {
        let fake = FakeSystem::new();
        fake.plug_yubikey("hidraw0", FakeYubiKey::new(|_, _| None));
        let (code, _, err) = call(&fake, &["1", "test"]);
        assert_eq!(code, 1);
        assert_eq!(
            err,
            "zbm debug yubikey: /dev/hidraw0: the YubiKey gave no response\n"
        );
        for args in [&["3", "x"][..], &["1"], &["--slot"]] {
            let (code, _, err) = call(&fake, args);
            assert_eq!(code, USAGE, "{args:?}");
            assert!(err.starts_with("Usage: zbm debug yubikey"));
        }
    }
}
