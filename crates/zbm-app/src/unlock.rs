//! The use case "open a LUKS volume with a key file".

use std::path::PathBuf;

use zbm_crypt::{Closed, Mapping, OpenOptions};
use zbm_luks::unlocker::{KeyFile, Unlocker};
use zbm_platform::Context;
use zbm_types::{DevicePath, MappingName};

use crate::{AppError, Event};

/// A volume to open, and how.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnlockRequest {
    /// The LUKS volume.
    pub device: DevicePath,
    /// The name of its mapping.
    pub name: MappingName,
    /// The key file to open it with.
    pub key_file: PathBuf,
    /// How to open it.
    pub options: OpenOptions,
}

/// Opens the volume of `request`.
///
/// The open mapping stays open: the volume is meant to be used, by a pool
/// import or by the booted system.
pub(crate) fn perform(
    context: &Context<'_>,
    request: UnlockRequest,
) -> Result<Vec<Event>, AppError> {
    let unlocker = Unlocker::new(vec![Box::new(KeyFile::new(request.key_file))]);
    let mapping = Mapping::<Closed>::new(request.device, request.name);
    let opened = unlocker.unlock(context, mapping, request.options)?;
    Ok(vec![Event::Unlocked {
        device: opened.mapping.device().clone(),
        name: opened.mapping.name().clone(),
        method: opened.method,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeSystem;

    fn request() -> UnlockRequest {
        UnlockRequest {
            device: "/dev/sda2".parse().unwrap(),
            name: "cryptroot".parse().unwrap(),
            key_file: PathBuf::from("/keys/root.key"),
            options: OpenOptions::default(),
        }
    }

    #[test]
    fn an_opened_volume_is_reported_as_an_event() {
        let fake = FakeSystem::new();
        fake.files.file("/keys/root.key", "secret");

        let events = App::new(Context::new(fake.system()))
            .perform(crate::Action::Unlock(request()))
            .unwrap();

        assert_eq!(
            events,
            [Event::Unlocked {
                device: "/dev/sda2".parse().unwrap(),
                name: "cryptroot".parse().unwrap(),
                method: "key file",
            }]
        );
    }

    #[test]
    fn a_failure_keeps_every_attempt() {
        let fake = FakeSystem::new();

        let error = App::new(Context::new(fake.system()))
            .perform(crate::Action::Unlock(request()))
            .unwrap_err();

        let AppError::Unlock(failed) = error else {
            panic!("expected an unlock failure, got {error}");
        };
        assert_eq!(failed.attempts.len(), 1);
        assert_eq!(
            failed.attempts[0].1.to_string(),
            "the key file /keys/root.key does not exist"
        );
    }
}
