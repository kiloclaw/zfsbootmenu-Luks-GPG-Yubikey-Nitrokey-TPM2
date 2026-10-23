//! The unlock alert (`docs/dev/menu-design/unlocking.md`): the volume, the
//! field, why this method is asked, the other methods and the keyboard
//! layout; **Skip** (`Esc`) and **Unlock** (`Enter`).

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;

use super::dialogs::{Alert, Kind, alert};
use crate::menu::format::capitalised;
use crate::menu::model::{Model, Prompt};
use crate::menu::theme::Role;
use crate::menu::world::Notice;

/// Draws the unlock alert of `prompt`, the cursor in its field.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, prompt: &Prompt) {
    let body = body(model, prompt);
    let buttons = vec![
        ("Skip".into(), Kind::Normal),
        ("Unlock".into(), Kind::Default),
    ];
    let title = format!("Unlock {}", prompt.volume);
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
        let column = prompt.method.as_str().len() + 2 + shown;
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
        Line::styled(format!("on {device}"), theme.style(Role::Muted)),
        Line::default(),
    ];
    let ask = if !method.is_typed() {
        format!(
            "Waiting for the {}: touch it, or plug it in.",
            method.as_str()
        )
    } else if prompt.revealed {
        format!("{}: {}", capitalised(method.as_str()), prompt.typed())
    } else {
        format!("{}:", capitalised(method.as_str()))
    };
    body.push(Line::styled(ask, theme.style(Role::Title)));
    if let Some(notice) = &prompt.notice {
        body.push(Line::styled(
            notice.text().to_owned(),
            theme.style(Role::Warning),
        ));
    }
    if let Some(left) = prompt.tries_left {
        body.push(Line::styled(
            format!("{left} tries left"),
            theme.style(Role::Warning),
        ));
    }
    let others: Vec<&str> = known
        .map(|known| {
            known
                .methods
                .iter()
                .filter(|m| **m != method)
                .map(|m| m.as_str())
                .collect()
        })
        .unwrap_or_default();
    if !others.is_empty() {
        let keys = if theme.is_ascii() {
            "Up/Down"
        } else {
            "↑/↓"
        };
        let line = format!("{keys} to use the {} instead", others.join(" or "));
        body.push(Line::styled(line, theme.style(Role::Muted)));
    }
    if method.is_typed() {
        body.push(keyboard_line(model, prompt));
    }
    body
}

/// The keyboard layout and the keys that change it or show what is typed;
/// after a wrong secret, a warning that the layout may be why.
fn keyboard_line(model: &Model, prompt: &Prompt) -> Line<'static> {
    let theme = model.theme;
    let keyboard = &model.snapshot.keyboard;
    let reveal = if prompt.revealed {
        "Ctrl-R: hide"
    } else {
        "Ctrl-R: show"
    };
    let dot = theme.dot();
    if !keyboard.local {
        let text = format!("Keyboard: the terminal's own{dot}{reveal}");
        return Line::styled(text, theme.style(Role::Muted));
    }
    let layout = model.layout().unwrap_or("us");
    if keyboard.layouts.len() < 2 {
        let text = format!("Keyboard: {layout}{dot}{reveal}");
        return Line::styled(text, theme.style(Role::Muted));
    }
    let wrong = prompt.notice.as_ref().is_some_and(Notice::is_wrong_secret);
    if wrong && model.layout == 0 {
        let text = format!("Keyboard: {layout}. Set with another layout? Ctrl-K changes it.");
        return Line::styled(text, theme.style(Role::Warning));
    }
    let text = format!("Keyboard: {layout}{dot}Ctrl-K: layout{dot}{reveal}");
    Line::styled(text, theme.style(Role::Muted))
}
