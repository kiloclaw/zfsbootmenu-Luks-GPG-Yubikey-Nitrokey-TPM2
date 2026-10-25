//! The state of start-up's dialogs: the unlock dialog, with what is asked,
//! why, and what was typed (`docs/dev/menu-design/unlocking.md`), and the
//! questions start-up asks, with the time left before it goes on.

use zbm_platform::secret::Secret;

use crate::menu::world::{Method, Notice, Question};

/// Start-up asks whether to go on; when the time runs out, it does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Asking {
    /// What it asks.
    pub question: Question,
    /// The milliseconds left before it goes on by itself; `None`: it waits.
    pub left: Option<u64>,
}

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
    /// For a method with nothing to type: the milliseconds left to wait
    /// for its device.
    pub wait: Option<u64>,
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
            wait: None,
        }
    }

    /// The prompt, waiting `seconds` for the method's device.
    #[must_use]
    pub fn waiting(mut self, seconds: Option<u64>) -> Prompt {
        self.wait = seconds.map(|seconds| seconds.saturating_mul(1000));
        self
    }

    /// What was typed, as the dialog shows it once revealed.
    pub fn typed(&self) -> String {
        String::from_utf8_lossy(self.secret.expose()).into_owned()
    }
}
