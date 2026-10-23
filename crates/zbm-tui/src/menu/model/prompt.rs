//! The unlock dialog's state: what is asked, why, and what was typed
//! (`docs/dev/menu-design/unlocking.md`).

use zbm_platform::secret::Secret;

use crate::menu::world::{Method, Notice};

/// A volume asks for a secret, or waits for a device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prompt {
    /// The volume.
    pub volume: String,
    /// The method being tried.
    pub method: Method,
    /// Tries left, if limited.
    pub tries_left: Option<u32>,
    /// Why this method is asked: another one failed, or the last try was
    /// wrong.
    pub notice: Option<Notice>,
    /// What was typed; wiped when dropped.
    pub secret: Secret,
    /// Whether what is typed is shown, as the user asked.
    pub revealed: bool,
}

impl Prompt {
    /// A prompt for `volume` with `method`, nothing typed yet.
    pub fn new(
        volume: String,
        method: Method,
        tries_left: Option<u32>,
        notice: Option<Notice>,
    ) -> Prompt {
        Prompt {
            volume,
            method,
            tries_left,
            notice,
            secret: Secret::new(),
            revealed: false,
        }
    }

    /// What was typed, as the dialog shows it once revealed.
    pub fn typed(&self) -> String {
        String::from_utf8_lossy(self.secret.expose()).into_owned()
    }
}
