//! Unlocking (`docs/dev/menu-design/unlocking.md`): the unlock dialog, the
//! keyboard layout typing is translated into, and sealing a TPM2 key again
//! after the TPM kept it.

use std::sync::Arc;

use zbm_display::keys::{Key, KeyPress};
use zbm_platform::secret::Secret;
use zbm_types::Printable;

use crate::menu::keymap::{self, Input, Intent, Modifiers};
use crate::menu::model::{Dialog, Model, Prompt, Then};
use crate::menu::world::{Action, Method, Outcome, PcrChange};
use crate::text::{self, Id, id, text};

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
        // Shown, a secret would be copied into the text and cells drawn,
        // which nothing wipes, in the start-up program that lives until
        // the boot: start-up keeps it hidden.
        Input::Intent(Intent::Reveal) if !model.is_starting_up() => {
            prompt.revealed = !prompt.revealed;
        }
        Input::Intent(Intent::Primary) => {
            let secret = prompt.method.is_typed().then(|| Arc::new(prompt.secret));
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
        .find(|known| known.name.get() == volume)
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

/// `key` as the layout `Ctrl-K` chose has it, once it chose one: the
/// character of the layout the key was typed on for the key's position
/// (the console's own when the input does not say), turned into the chosen
/// layout's, with the image's keymaps. Only in the unlock dialog and for
/// action keys: the filter, the palette, a name and a typed confirmation
/// take the characters as typed. Keys with `Ctrl` or `Alt`, a terminal's
/// characters, and layouts whose keymap did not load are left as they are.
pub(super) fn translated(model: &Model, mut key: KeyPress) -> KeyPress {
    let keyboard = &model.snapshot.keyboard;
    let held = Modifiers::of(&key).has_any(Modifiers::CTRL | Modifiers::ALT);
    let is_field =
        keymap::is_text(model.context()) && !matches!(model.dialog, Some(Dialog::Unlock(_)));
    if held || is_field || model.is_typed_on_a_terminal() {
        return key;
    }
    let to = model.layout.and_then(|chosen| keyboard.layouts.get(chosen));
    let positions = |name: Option<&str>| name.and_then(|name| keyboard.positions.get(name));
    if let (Key::Char(c), Some(from), Some(to)) = (
        key.key,
        positions(model.typed_layout()),
        positions(to.map(String::as_str)),
    ) {
        key.key = Key::Char(from.translate(to, c));
    }
    key
}

/// The next keyboard layout: the one after the layout chosen last, or,
/// the first time, after the one keys are typed on; what the unlock dialog
/// had typed is cleared, as it was typed with the other one. A terminal's
/// characters are in its own layout, which the menu does not know.
pub(super) fn next_layout(model: &mut Model) {
    let layouts = &model.snapshot.keyboard.layouts;
    if model.is_typed_on_a_terminal() {
        model.notification = Some(Outcome::Warning(text!("layout-serial")));
        return;
    }
    if layouts.len() < 2 {
        model.notification = Some(Outcome::Warning(text!("layout-only-one")));
        return;
    }
    // A layout the image does not list comes before its first.
    let current = model.layout.or_else(|| {
        let typed = model.typed_layout()?;
        layouts.iter().position(|name| name == typed)
    });
    model.layout = Some(current.map_or(0, |at| (at + 1) % layouts.len()));
    if let Some(Dialog::Unlock(prompt)) = &mut model.dialog {
        prompt.secret = Secret::new();
    }
    let name = model.layout().unwrap_or_default().to_owned();
    model.notification = Some(Outcome::Done(text!("layout-typing", layout = name)));
}

/// The confirmation of sealing `volume`'s TPM2 key to the PCRs as they are
/// now; `None` when its key is current or it has none.
pub(super) fn reseal(model: &Model, volume: &str) -> Option<Then> {
    let known = model
        .snapshot
        .volumes
        .iter()
        .find(|v| v.name.get() == volume)?;
    let seal = known.tpm2.as_ref().filter(|seal| !seal.is_current())?;
    // Short, so that the field to type in stays on a 52x20 screen.
    let mut lines: Vec<String> = seal
        .changed
        .iter()
        .map(|change| format!("{}.", pcr_change(change)))
        .collect();
    lines.push(text!("reseal", pcrs = seal.pcr_list()));
    lines.push(text!("reseal.only-yours"));
    Some(Then::Confirm {
        action: Action::Reseal(volume.to_owned()),
        title: text!("reseal.title", volume = Printable(volume).to_string()),
        lines,
        name: volume.to_owned(),
    })
}

/// A PCR that changed, for people: `PCR 0 (firmware code): the firmware
/// was updated to 1.21`.
pub(crate) fn pcr_change(change: &PcrChange) -> String {
    let name = text::get(pcr_name(change.index));
    let pcr = text!("pcr", index = change.index, name = name);
    match &change.detail {
        Some(detail) => format!("{pcr}: {detail}"),
        None => pcr,
    }
}

/// What a PCR measures on a UEFI machine, as the TCG PC Client profile and
/// systemd use them.
fn pcr_name(index: u8) -> Id {
    match index {
        0 => id!("pcr-firmware-code"),
        1 => id!("pcr-firmware-settings"),
        2 => id!("pcr-option-roms"),
        3 => id!("pcr-option-rom-settings"),
        4 => id!("pcr-boot-loader"),
        5 => id!("pcr-partition-table"),
        7 => id!("pcr-secure-boot"),
        9 => id!("pcr-initramfs"),
        11 => id!("pcr-kernel-image"),
        12 => id!("pcr-command-line"),
        14 => id!("pcr-shim"),
        15 => id!("pcr-system-identity"),
        _ => id!("pcr-other"),
    }
}
