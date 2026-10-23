//! The differences view: what changed since a snapshot, as a grouped list,
//! and a preview of the selected change; marked paths can be restored.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::lists;
use crate::menu::format::fit;
use crate::menu::model::{DiffView, Model};
use crate::menu::theme::{Glyph, Role, Theme};
use crate::menu::world::{Change, ChangeKind};

/// From this width the preview is to the right of the list.
const WIDE: u16 = 100;

/// Draws the differences view.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let Some(diff) = &model.diff else {
        return;
    };
    let (list, preview) = if area.width >= WIDE {
        let [list, preview] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(area);
        (list, preview)
    } else {
        let lines = if area.height >= 17 { 8 } else { 6 };
        let [list, preview] =
            Layout::vertical([Constraint::Min(4), Constraint::Length(lines)]).areas(area);
        (list, preview)
    };
    render_list(model, frame, list, diff);
    render_preview(model.theme, frame, preview, diff);
}

/// The style of a kind of change.
fn role(kind: ChangeKind) -> Role {
    match kind {
        ChangeKind::Removed => Role::Danger,
        ChangeKind::Created => Role::Good,
        ChangeKind::Modified => Role::Warning,
        ChangeKind::Renamed => Role::Accent,
    }
}

/// The changes: a mark, the kind, the path and where it went.
fn render_list(model: &Model, frame: &mut Frame<'_>, area: Rect, diff: &DiffView) {
    let theme = model.theme;
    let width = lists::row_width(area).saturating_sub(4);
    let rows = diff
        .list
        .matches
        .iter()
        .filter_map(|ranked| {
            let change = diff.changes.get(ranked.index)?;
            let marked = diff.marked.contains(&change.path);
            let mark = if marked {
                theme.glyph(Glyph::Marked)
            } else {
                " "
            };
            let mut spans = vec![
                Span::styled(format!("{mark} "), theme.style(Role::Accent)),
                Span::styled(
                    format!("{} ", change.kind.mark()),
                    theme.style(role(change.kind)),
                ),
            ];
            let path = fit(&change.path, width, theme.is_ascii());
            spans.extend(lists::highlighted(theme, &path, &ranked.positions));
            if let Some(to) = &change.renamed_to {
                spans.push(Span::styled(format!(" → {to}"), theme.style(Role::Muted)));
            }
            Some(lists::clip(Line::from(spans), width + 4, theme.is_ascii()))
        })
        .collect();
    let since = diff.from.name();
    let header = format!("Changes since {since}");
    let empty = ("No Changes", "Nothing changed since the snapshot.");
    let total = (header.as_str(), diff.changes.len());
    lists::render(model, frame, area, total, &diff.list, rows, empty);
}

/// The preview of the selected change: its text diff, or why there is none.
fn render_preview(theme: Theme, frame: &mut Frame<'_>, area: Rect, diff: &DiffView) {
    let Some(change) = diff.selected() else {
        let block = lists::block(theme, "Preview");
        frame.render_widget(block, area);
        return;
    };
    let block = lists::counted(theme, "Preview", kind_word(change.kind));
    let lines = preview_lines(theme, change);
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    frame.render_widget(paragraph, area);
}

/// What happened, in a word.
fn kind_word(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Removed => "removed",
        ChangeKind::Created => "created",
        ChangeKind::Modified => "modified",
        ChangeKind::Renamed => "renamed",
    }
}

/// The lines of the preview of `change`.
fn preview_lines(theme: Theme, change: &Change) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(change.path.clone(), theme.style(Role::Title))];
    let note = match (change.kind, &change.renamed_to) {
        (ChangeKind::Created, _) => {
            Some("Created after the snapshot: there is nothing to restore.".to_owned())
        }
        (ChangeKind::Removed, _) => {
            Some("Removed since the snapshot: restoring brings it back.".to_owned())
        }
        (ChangeKind::Renamed, Some(to)) => Some(format!(
            "Renamed to {to}: restoring brings back the old name."
        )),
        _ if change.preview.is_empty() => Some("A binary file: no text to show.".to_owned()),
        _ => None,
    };
    if let Some(note) = note {
        lines.push(Line::styled(note, theme.style(Role::Muted)));
    }
    lines.extend(change.preview.iter().map(|line| {
        let role = match line.chars().next() {
            Some('-') => Role::Danger,
            Some('+') => Role::Good,
            _ => Role::Muted,
        };
        Line::styled(line.clone(), theme.style(role))
    }));
    lines
}
