//! The questions of start-up, as alerts: what happened, the time left
//! before start-up goes on by itself, and the two answers, **Esc** to stop
//! and **Enter** to go on.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::dialogs::{Alert, Kind, alert};
use crate::menu::model::Asking;
use crate::menu::theme::{Role, Theme};
use crate::menu::world::Question;
use crate::text::text;

/// What a question says: its title, its message, how it goes on and what
/// stops it, if anything does, and how the countdown reads.
struct Wording {
    title: String,
    lines: Vec<String>,
    go: String,
    stop: Option<String>,
    countdown: fn(u64) -> String,
}

/// The countdown of a question that tries again.
fn retrying(seconds: u64) -> String {
    text!("question-retrying", seconds = seconds)
}

/// The countdown of a question that looks for devices again.
fn looking(seconds: u64) -> String {
    text!("question-looking", seconds = seconds)
}

/// The countdown of a question that goes on.
fn continuing(seconds: u64) -> String {
    text!("question-continuing", seconds = seconds)
}

/// The words of `question`.
fn wording(question: &Question) -> Wording {
    match question {
        Question::RetryUnlock { volume } => Wording {
            title: text!("question-unlock.title", volume = volume.as_str()),
            lines: vec![text!("question-unlock")],
            go: text!("button-try-again"),
            stop: Some(text!("button-skip")),
            countdown: retrying,
        },
        Question::MissingVolume { device } => Wording {
            title: text!("question-missing-volume.title", device = device.as_str()),
            lines: vec![text!("question-missing-volume", device = device.as_str())],
            go: text!("button-look-again"),
            stop: Some(text!("button-skip")),
            countdown: looking,
        },
        Question::MissingDevices => Wording {
            title: text!("question-missing-devices.title"),
            lines: vec![text!("question-missing-devices")],
            go: text!("button-look-again"),
            stop: Some(text!("button-stop-waiting")),
            countdown: looking,
        },
        Question::ImportFailed { pool } => Wording {
            title: match pool {
                Some(pool) => text!("import-failed.title", pool = pool.as_str()),
                None => text!("question-import-none.title"),
            },
            lines: vec![text!("question-import")],
            go: text!("button-try-again"),
            stop: Some(text!("import-shell")),
            countdown: retrying,
        },
        Question::Unsupported { advice } => Wording {
            title: text!("question-unsupported.title"),
            lines: vec![text!("question-unsupported"), format!("{advice}.")],
            go: text!("button-continue"),
            stop: None,
            countdown: continuing,
        },
    }
}

/// Draws the alert of `asking`.
pub(super) fn render(theme: Theme, frame: &mut Frame<'_>, area: Rect, asking: &Asking) {
    let wording = wording(&asking.question);
    let mut body: Vec<Line<'static>> = wording.lines.into_iter().map(Line::from).collect();
    if let Some(left) = asking.left {
        let seconds = left.div_ceil(1000);
        body.push(Line::default());
        body.push(Line::styled(
            (wording.countdown)(seconds),
            theme.style(Role::Muted),
        ));
    }
    let mut buttons = Vec::new();
    if let Some(stop) = wording.stop {
        buttons.push((stop, Kind::Normal));
    }
    buttons.push((wording.go, Kind::Default));
    alert(
        theme,
        frame,
        area,
        Alert {
            title: wording.title,
            body,
            buttons,
            danger: false,
        },
    );
}
