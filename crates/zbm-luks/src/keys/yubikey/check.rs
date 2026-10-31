//! What `zbm debug yubikey` shows (ROADMAP.md SB11): the YubiKeys reached
//! over their OTP interface, and a slot's response to a test challenge
//! beside `ykchalresp`'s, to check the protocol on a real key.

use std::path::PathBuf;
use std::time::Duration;

use zbm_crypt::yubikey::YubiKey;
use zbm_crypt::yubikey::otp::{self, Otp};
use zbm_platform::{Context, Secret};

/// A YubiKey reached over its OTP interface.
#[derive(Debug, Eq, PartialEq)]
pub struct FoundKey {
    /// Its `hidraw` device.
    pub device: PathBuf,
    /// Its serial number, or why it gave none.
    pub serial: Result<u32, String>,
}

/// The YubiKeys among the `hidraw` devices, with their serial numbers.
pub fn found_keys(context: &Context<'_>) -> Vec<FoundKey> {
    let protocol = Otp::new(context.hid(), context.clock());
    otp::devices(context.files())
        .into_iter()
        .map(|device| FoundKey {
            serial: protocol.serial(&device).map_err(|error| error.to_string()),
            device,
        })
        .collect()
}

/// What a check of a slot's response gave.
#[derive(Debug)]
pub enum Checked {
    /// No YubiKey among the `hidraw` devices.
    NoKey,
    /// The first key gave no response, for this reason.
    Failed(PathBuf, String),
    /// The first key's response, and whether `ykchalresp` gives the same,
    /// where the image has it.
    Answered(PathBuf, Secret, Option<Agreement>),
}

/// Whether `ykchalresp` gives the response the protocol got.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Agreement {
    /// The same response.
    Same,
    /// Another one, or none.
    Different,
}

/// The response of the first YubiKey's `slot` to `challenge`, beside
/// `ykchalresp`'s.
pub fn check_response(context: &Context<'_>, slot: u8, challenge: &Secret) -> Checked {
    let Some(device) = otp::devices(context.files()).into_iter().next() else {
        return Checked::NoKey;
    };
    let protocol = Otp::new(context.hid(), context.clock());
    let native = match protocol.response(&device, slot, challenge) {
        Ok(response) => response,
        Err(error) => return Checked::Failed(device, error.to_string()),
    };
    let agreement = context.command_exists("ykchalresp").then(|| {
        // The key takes a moment to be ready again.
        context.clock().sleep(Duration::from_millis(100));
        let tool = YubiKey::new(context.runner()).response(slot, challenge);
        if tool.is_some_and(|tool| tool.ct_eq(native.expose())) {
            Agreement::Same
        } else {
            Agreement::Different
        }
    });
    Checked::Answered(device, native, agreement)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_platform::{Output, Status};
    use zbm_testkit::fakes::{FakeSystem, FakeYubiKey};

    use super::*;

    /// A key whose slots answer with 20 bytes of the challenge's length.
    fn key() -> FakeYubiKey {
        FakeYubiKey::new(|_, challenge| Some(vec![u8::try_from(challenge.len()).unwrap(); 20]))
    }

    #[test]
    fn the_keys_are_found_with_their_serial_numbers() {
        let fake = FakeSystem::new();
        assert!(found_keys(&fake.context()).is_empty());
        fake.plug_yubikey("hidraw0", key().with_serial(99));
        fake.plug_yubikey("hidraw2", key());

        let found = found_keys(&fake.context());

        assert_eq!(
            found,
            [
                FoundKey {
                    device: "/dev/hidraw0".into(),
                    serial: Ok(99)
                },
                FoundKey {
                    device: "/dev/hidraw2".into(),
                    serial: Err("the YubiKey gave no response".to_owned())
                },
            ]
        );
    }

    #[test]
    fn a_response_is_checked_against_ykchalresp_s() {
        let fake = FakeSystem::new();
        let test = Secret::copy_from(b"test");
        assert!(matches!(
            check_response(&fake.context(), 2, &test),
            Checked::NoKey
        ));
        fake.plug_yubikey("hidraw0", key());
        let expected = "04".repeat(20);
        let Checked::Answered(device, response, None) = check_response(&fake.context(), 2, &test)
        else {
            panic!("no answer");
        };
        assert_eq!(device, PathBuf::from("/dev/hidraw0"));
        assert_eq!(response.expose(), expected.as_bytes());

        fake.environment.set("PATH", "/bin");
        fake.files.file_with_mode("/bin/ykchalresp", "", 0o755);
        fake.runner.on(move |command| {
            let response = Secret::copy_from(expected.as_bytes());
            (command.program() == "ykchalresp")
                .then(|| Ok(Output::with_secret(Status::exited(0), response)))
        });
        let same = check_response(&fake.context(), 2, &test);
        assert!(
            matches!(same, Checked::Answered(_, _, Some(Agreement::Same))),
            "{same:?}"
        );
        let other = check_response(&fake.context(), 1, &Secret::copy_from(b"other"));
        assert!(matches!(
            other,
            Checked::Answered(_, _, Some(Agreement::Different))
        ));
    }

    #[test]
    fn a_key_that_gives_nothing_is_said_so() {
        let fake = FakeSystem::new();
        fake.plug_yubikey("hidraw0", FakeYubiKey::new(|_, _| None));
        let checked = check_response(&fake.context(), 1, &Secret::copy_from(b"test"));
        let Checked::Failed(device, error) = checked else {
            panic!("{checked:?}");
        };
        assert_eq!(device, PathBuf::from("/dev/hidraw0"));
        assert_eq!(error, "the YubiKey gave no response");
    }
}
