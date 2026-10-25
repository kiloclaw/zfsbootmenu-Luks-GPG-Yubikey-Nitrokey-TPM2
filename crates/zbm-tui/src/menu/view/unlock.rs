//! The unlock alert (`docs/dev/menu-design/unlocking.md`): the volume, the
//! field, why this method is asked, the other methods and the keyboard
//! layout; **Skip** (`Esc`) and **Unlock** (`Enter`).

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;

use super::dialogs::{Alert, Kind, alert};
use crate::menu::model::{Model, Prompt};
use crate::menu::theme::Role;
use crate::menu::world::{Method, Notice};
use crate::text::{self, Id, id, text};

/// The name of a key method: the message is how a sentence names it, its
/// `.label` the label of its field.
pub(crate) fn method_name(method: Method) -> Id {
    match method {
        Method::Passphrase => id!("method-passphrase"),
        Method::Tpm2Pin => id!("method-tpm2-pin"),
        Method::Tpm2 => id!("method-tpm2"),
        Method::Clevis => id!("method-clevis"),
        Method::Fido2 => id!("method-fido2"),
        Method::YubiKey => id!("method-yubikey"),
        Method::GnupgKeyFile => id!("method-gnupg"),
        Method::KeyDevice => id!("method-key-device"),
        Method::Fido2Pin => id!("method-fido2-pin"),
        Method::YubiKeyPassphrase => id!("method-yubikey-passphrase"),
        Method::OpenPgpCard => id!("method-openpgp-card"),
        Method::OpenPgpCardPin => id!("method-openpgp-card-pin"),
    }
}

/// The label of the field of a typed method.
fn field_label(method: Method) -> String {
    text::message(method_label(method), &[])
}

/// The `.label` of a method's name.
fn method_label(method: Method) -> Id {
    match method {
        Method::Passphrase => id!("method-passphrase.label"),
        Method::Tpm2Pin => id!("method-tpm2-pin.label"),
        Method::GnupgKeyFile => id!("method-gnupg.label"),
        Method::Fido2Pin => id!("method-fido2-pin.label"),
        Method::YubiKeyPassphrase => id!("method-yubikey-passphrase.label"),
        Method::OpenPgpCardPin => id!("method-openpgp-card-pin.label"),
        other => method_name(other),
    }
}

/// Draws the unlock alert of `prompt`, the cursor in its field.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, prompt: &Prompt) {
    let body = body(model, prompt);
    let buttons = vec![
        (text!("button-skip"), Kind::Normal),
        (text!("button-unlock"), Kind::Default),
    ];
    let title = text!("unlock-title", volume = prompt.volume.as_str());
    let message = alert(
        model.theme,
        frame,
        area,
        Alert {
            title,
            body,
            buttons,
            danger: false,
        },
    );
    if prompt.method.is_typed() {
        let shown = if prompt.revealed {
            prompt.typed().chars().count()
        } else {
            0
        };
        let column = field_label(prompt.method).chars().count() + 2 + shown;
        let column = u16::try_from(column).unwrap_or(u16::MAX);
        let row = message.y.saturating_add(2);
        frame.set_cursor_position(Position::new(message.x.saturating_add(column), row));
    }
}

/// The message: the device, the field or what is awaited, why, the tries
/// left, the other methods, the keyboard.
fn body(model: &Model, prompt: &Prompt) -> Vec<Line<'static>> {
    let theme = model.theme;
    let method = prompt.method;
    let known = model
        .snapshot
        .volumes
        .iter()
        .find(|known| known.name == prompt.volume);
    let device = known.map(|known| known.device.clone()).unwrap_or_default();
    let mut body = vec![
        Line::styled(
            text!("unlock-device", device = device.as_str()),
            theme.style(Role::Muted),
        ),
        Line::default(),
    ];
    let ask = if !method.is_typed() {
        text!("unlock-waiting", method = text::get(method_name(method)))
    } else if prompt.revealed {
        format!("{}: {}", field_label(method), prompt.typed())
    } else {
        format!("{}:", field_label(method))
    };
    body.push(Line::styled(ask, theme.style(Role::Title)));
    if let Some(notice) = &prompt.notice {
        body.push(Line::styled(
            notice.text().to_owned(),
            theme.style(Role::Warning),
        ));
    }
    if let Some(wait) = prompt.wait {
        let seconds = wait.div_ceil(1000);
        body.push(Line::styled(
            text!("unlock-wait-left", seconds = seconds),
            theme.style(Role::Muted),
        ));
    }
    if let Some(left) = prompt.tries_left {
        body.push(Line::styled(
            text!("unlock-tries-left", count = left),
            theme.style(Role::Warning),
        ));
    }
    if let Some(line) = known.and_then(|known| other_methods(model, &known.methods, method)) {
        body.push(line);
    }
    if method.is_typed() {
        body.push(keyboard_line(model, prompt));
    }
    body
}

/// The line that names the methods of `methods` other than `method`, and
/// the keys that choose them; `None` when there is none.
fn other_methods(model: &Model, methods: &[Method], method: Method) -> Option<Line<'static>> {
    let theme = model.theme;
    let others: Vec<String> = methods
        .iter()
        .filter(|m| **m != method)
        .map(|m| text::get(method_name(*m)))
        .collect();
    if others.is_empty() {
        return None;
    }
    let keys = if theme.is_ascii() {
        text!("keys-up-down")
    } else {
        "↑/↓".to_owned()
    };
    let line = text!(
        "unlock-other-methods",
        keys = keys,
        methods = others.join(&text!("unlock-or"))
    );
    Some(Line::styled(line, theme.style(Role::Muted)))
}

/// The keyboard layout and the keys that change it or show what is typed;
/// after a wrong secret, a warning that the layout may be why.
fn keyboard_line(model: &Model, prompt: &Prompt) -> Line<'static> {
    let theme = model.theme;
    let keyboard = &model.snapshot.keyboard;
    let reveal = if prompt.revealed {
        text!("unlock-hide")
    } else {
        text!("unlock-show")
    };
    let dot = theme.dot();
    if !keyboard.local {
        let line = format!("{}{dot}{reveal}", text!("keyboard-terminal"));
        return Line::styled(line, theme.style(Role::Muted));
    }
    let layout = model.layout().unwrap_or(crate::menu::keyboard::CONSOLE);
    if keyboard.layouts.len() < 2 {
        let line = format!("{}{dot}{reveal}", text!("keyboard-layout", layout = layout));
        return Line::styled(line, theme.style(Role::Muted));
    }
    let wrong = prompt.notice.as_ref().is_some_and(Notice::is_wrong_secret);
    if wrong && model.layout == 0 {
        let line = text!("keyboard-wrong", layout = layout);
        return Line::styled(line, theme.style(Role::Warning));
    }
    let line = format!(
        "{}{dot}{}{dot}{reveal}",
        text!("keyboard-layout", layout = layout),
        text!("keyboard-change")
    );
    Line::styled(line, theme.style(Role::Muted))
}
