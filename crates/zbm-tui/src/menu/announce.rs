//! What the menu tells a screen reader (ROADMAP.md UI8, ADR 0011): what
//! changes without the person moving, as lines the console on the display
//! shows on its status line and writes for `brltty` (`Announce`). A dialog
//! or a question that opens, the outcome of an action, the countdown that
//! starts or stops, and the menu that is back after the programs had the
//! screen; what a review page asks, when its plan comes; not the
//! selection, which the screen shows already.

use super::model::{Dialog, Model};
use super::view::words;
use crate::text::text;

/// What was said last, to say only what changed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Announcer {
    /// The title of the dialog over the screen, and what a review page
    /// asks.
    dialog: Option<(String, Option<String>)>,
    /// The notification line.
    notification: Option<String>,
    /// Whether the countdown ran.
    counting: bool,
    /// Whether the menu is back after the programs had the screen.
    back: bool,
}

impl Announcer {
    /// Says, at the next frame, that the menu has the screen again.
    pub fn back(&mut self) {
        self.back = true;
    }

    /// What to say of `model`, the next frame, since the last one.
    pub fn next(&mut self, model: &Model) -> Vec<String> {
        let mut said = Vec::new();
        if std::mem::take(&mut self.back) {
            said.push(words::title(model));
        }
        let dialog = words::dialog(model);
        // A review page is said again when what it asks changes: its plan
        // came, or refuses the change.
        let review = matches!(model.dialog, Some(Dialog::Review(_)));
        let title = dialog.as_ref().map(|said| {
            let asks = said.lines.first().filter(|_| review).cloned();
            (said.title.clone(), asks)
        });
        if let Some(dialog) = dialog.as_ref().filter(|_| title != self.dialog) {
            said.push(match dialog.lines.first() {
                Some(first) => format!("{}: {first}", dialog.title),
                None => dialog.title.clone(),
            });
        }
        self.dialog = title;
        let notification = words::notification(model).filter(|_| model.working.is_none());
        if notification.is_some() && notification != self.notification {
            said.extend(notification.clone());
        }
        self.notification = notification;
        let countdown = words::countdown(model);
        match (&countdown, self.counting) {
            (Some(text), false) => said.push(text.clone()),
            (None, true) if model.leaving.is_none() => said.push(text!("lines-countdown-stopped")),
            _ => {}
        }
        self.counting = countdown.is_some();
        said
    }
}

#[cfg(test)]
mod tests;
