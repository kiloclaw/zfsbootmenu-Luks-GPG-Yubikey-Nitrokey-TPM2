//! Unlocking (`docs/dev/menu-design/unlocking.md`): the unlock dialog, the
//! keyboard layout typing is translated into, and sealing a TPM2 key again
//! after the TPM kept it.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use zbm_platform::secret::Secret;

use crate::menu::keyboard;
use crate::menu::keymap::{Input, Intent};
use crate::menu::model::{Dialog, Model, Prompt, Then};
use crate::menu::world::{Action, Method, Outcome};

/// The unlock dialog: the secret never leaves its [`Secret`].
pub(super) fn prompt(model: &mut Model, input: Input, mut prompt: Prompt) -> Vec<Action> {
    match input {
        Input::Type(c) => {
            let mut bytes = [0_u8; 4];
            prompt
                .secret
                .push_bytes(c.encode_utf8(&mut bytes).as_bytes());
        }
        Input::Erase => erase_char(&mut prompt.secret),
        Input::Clear => prompt.secret = Secret::new(),
        Input::Intent(Intent::Back) => return vec![Action::SkipUnlock(prompt.volume)],
        Input::Intent(Intent::Up | Intent::Down) => {
            prompt.method = other_method(model, &prompt.volume, prompt.method, input);
            prompt.secret = Secret::new();
            prompt.notice = None;
            prompt.tries_left = None;
        }
        Input::Intent(Intent::Reveal) => prompt.revealed = !prompt.revealed,
        Input::Intent(Intent::Primary) => {
            let secret = prompt.method.is_typed().then_some(prompt.secret);
            return vec![Action::Unlock {
                volume: prompt.volume,
                method: prompt.method,
                secret,
            }];
        }
        Input::Intent(_) => {}
    }
    model.dialog = Some(Dialog::Unlock(prompt));
    Vec::new()
}

/// Removes the last character of a secret typed as UTF-8.
fn erase_char(secret: &mut Secret) {
    while let Some(byte) = secret.pop() {
        if byte & 0xC0 != 0x80 {
            break;
        }
    }
}

/// The method before or after `method` among those of `volume`.
fn other_method(model: &Model, volume: &str, method: Method, input: Input) -> Method {
    let methods = model
        .snapshot
        .volumes
        .iter()
        .find(|known| known.name == volume)
        .map(|known| known.methods.clone())
        .unwrap_or_default();
    let Some(position) = methods.iter().position(|known| *known == method) else {
        return method;
    };
    let next = if input == Input::Intent(Intent::Up) {
        position.checked_sub(1).unwrap_or(methods.len() - 1)
    } else {
        (position + 1) % methods.len()
    };
    methods.get(next).copied().unwrap_or(method)
}

/// `key` as the chosen layout has it: the console's character for the key's
/// position, turned into the chosen layout's. Keys with `Ctrl` or `Alt`,
/// and keys from a serial terminal, are left as they are.
pub(super) fn translated(model: &Model, mut key: KeyEvent) -> KeyEvent {
    let keyboard = &model.snapshot.keyboard;
    let held = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    if let (KeyCode::Char(c), Some(from), Some(to)) =
        (key.code, keyboard.layouts.first(), model.layout())
        && keyboard.local
        && !held
    {
        key.code = KeyCode::Char(keyboard::translate(from, to, c));
    }
    key
}

/// The next keyboard layout; what the unlock dialog had typed is cleared,
/// as it was typed with the other one.
pub(super) fn next_layout(model: &mut Model) {
    let keyboard = &model.snapshot.keyboard;
    if !keyboard.local {
        model.notification = Some(Outcome::Warning(
            "keys come from a serial terminal: its own layout applies".to_owned(),
        ));
        return;
    }
    if keyboard.layouts.len() < 2 {
        model.notification = Some(Outcome::Warning(
            "this image knows one keyboard layout".to_owned(),
        ));
        return;
    }
    model.layout = (model.layout + 1) % keyboard.layouts.len();
    if let Some(Dialog::Unlock(prompt)) = &mut model.dialog {
        prompt.secret = Secret::new();
    }
    let name = model.layout().unwrap_or_default().to_owned();
    model.notification = Some(Outcome::Done(format!("typing with the {name} layout")));
}

/// The confirmation of sealing `volume`'s TPM2 key to the PCRs as they are
/// now; `None` when its key is current or it has none.
pub(super) fn reseal(model: &Model, volume: &str) -> Option<Then> {
    let known = model.snapshot.volumes.iter().find(|v| v.name == volume)?;
    let seal = known.tpm2.as_ref().filter(|seal| !seal.is_current())?;
    // Short, so that the field to type in stays on a 52x20 screen.
    let mut lines: Vec<String> = seal
        .changed
        .iter()
        .map(|change| format!("{}.", change.describe()))
        .collect();
    lines.push(format!(
        "Re-sealing trusts the machine as it is now (PCRs {}).",
        seal.pcr_list()
    ));
    lines.push("Do it only for changes you made; else keep the passphrase.".to_owned());
    Some(Then::Confirm {
        action: Action::Reseal(volume.to_owned()),
        title: format!("Re-seal the TPM2 Key of {volume}"),
        lines,
        name: volume.to_owned(),
    })
}
