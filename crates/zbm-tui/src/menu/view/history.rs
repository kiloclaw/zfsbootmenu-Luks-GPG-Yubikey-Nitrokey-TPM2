//! The boot history in the workspace's Log tab (ADR 0016, point 7): above
//! the log since start, the boots whose logs `zbm.save_logs` kept, newest
//! first, as many as fit in half the tab; without the option, how to keep
//! them. Every name and text of the history comes from the EFI system
//! partition, which anyone with the disk can write: each is drawn
//! printable.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use zbm_types::code::ErrorCode;
use zbm_types::{BootRecord, Printable};

use super::{lists, screens};
use crate::menu::format::{date, fit};
use crate::menu::model::Model;
use crate::menu::theme::Role;
use crate::menu::world::{History, Recorded, SavedBoot};
use crate::text::text;

/// The fewest rows the history takes, with its borders.
const LEAST: u16 = 3;

/// Draws the history, and the log since start below it.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let rows = rows(model);
    let wanted = u16::try_from(rows.len().saturating_add(2)).unwrap_or(u16::MAX);
    let height = wanted.min((area.height / 2).max(LEAST));
    let [top, log] = Layout::vertical([Constraint::Length(height), Constraint::Min(1)]).areas(area);
    let width = usize::from(top.width.saturating_sub(4));
    let lines: Vec<Line<'static>> = rows
        .into_iter()
        .map(|(role, row)| Line::styled(fit(&row, width, theme.is_ascii()), theme.style(role)))
        .collect();
    let title = text!("history-title");
    let block = match &model.history {
        Some(History::Saved(boots)) => lists::counted(theme, &title, &boots.len().to_string()),
        _ => lists::block(theme, &title),
    };
    frame.render_widget(Paragraph::new(lines).block(block), top);
    screens::log(model, frame, log);
}

/// The rows of the history, newest first, each with how it is drawn.
pub(super) fn rows(model: &Model) -> Vec<(Role, String)> {
    let muted = |row| (Role::Muted, row);
    match &model.history {
        None => vec![muted(text!("history-reading"))],
        Some(History::Off) => vec![muted(text!("history-off")), muted(text!("history-off-how"))],
        Some(History::Unavailable { why, code }) => vec![(
            Role::Warning,
            text!(
                "history-unavailable",
                why = Printable(why).to_string(),
                code = ErrorCode::new(*code).to_string()
            ),
        )],
        Some(History::Saved(boots)) if boots.is_empty() => vec![muted(text!("history-none"))],
        Some(History::Saved(boots)) => boots.iter().map(row).collect(),
    }
}

/// The row of `boot`: what its record says, or why it has none.
fn row(boot: &SavedBoot) -> (Role, String) {
    let name = boot.name.printable().to_string();
    match &boot.record {
        Recorded::Read(record) => (Role::Text, loaded(record)),
        Recorded::Missing => (Role::Muted, text!("history-no-record", name = name)),
        Recorded::Unreadable(why) => (
            Role::Warning,
            text!(
                "history-unreadable",
                name = name,
                why = Printable(why).to_string()
            ),
        ),
    }
}

/// What `record` says: when, which environment, which kernel, and whether
/// what was loaded was measured.
fn loaded(record: &BootRecord) -> String {
    let measured = if record.measured {
        text!("history-measured")
    } else {
        text!("history-not-measured")
    };
    text!(
        "history-loaded",
        when = when(record.time),
        environment = record.environment.printable().to_string(),
        kernel = kernel(record),
        measured = measured
    )
}

/// The version of the kernel `record` loaded, drawn printable.
pub(super) fn kernel(record: &BootRecord) -> String {
    zbm_app::kernel_version(&record.kernel.printable().to_string())
}

/// The time `seconds` after the epoch, in UTC, to the minute.
pub(super) fn when(seconds: u64) -> String {
    let (hour, minute) = (seconds % 86_400 / 3_600, seconds % 3_600 / 60);
    format!("{} {hour:02}:{minute:02}", date(seconds))
}
