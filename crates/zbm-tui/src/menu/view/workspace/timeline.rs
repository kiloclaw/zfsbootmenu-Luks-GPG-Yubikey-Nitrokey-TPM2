//! The Snapshots tab of the workspace (ADR 0007, decision 2; ADR 0016,
//! point 9): under the path, the boot environments beside the snapshots
//! of the one open, as a timeline of groups under headers that are no
//! stops, the newest first, and the inspector of the snapshot selected.
//! From 120 columns the inspector is to their right; narrower it is
//! below them, and under 80 columns the snapshots stand alone. While a
//! filter ranks the snapshots by how well each name matches, they show
//! without groups, as the Boot tab's environments do.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::super::{lists, main_view, screens};
use super::columns::{Facts, render_environments};
use crate::menu::format::{age, date, fit, size};
use crate::menu::model::{Group, Model, Row};
use crate::menu::theme::{Glyph, Role};
use crate::menu::world::SnapshotInfo;
use crate::text::text;

/// From this width the environments show beside the snapshots.
const TWO_FROM: u16 = 80;

/// The width of the environments column.
const ENVIRONMENTS: u16 = 24;

/// From this width the inspector shows to the right of the snapshots.
const BESIDE_FROM: u16 = 120;

/// The width of the inspector beside the snapshots.
const INSPECTOR: u16 = 36;

/// The height of the inspector below the snapshots: its three facts.
const BELOW: u16 = 5;

/// Draws the Snapshots tab in `area`.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let path = super::path::height(model);
    let [top, area] = Layout::vertical([Constraint::Length(path), Constraint::Min(1)]).areas(area);
    super::path::render(model, frame, top);
    if area.width >= BESIDE_FROM {
        let [environments, snapshots, inspector] = Layout::horizontal([
            Constraint::Length(ENVIRONMENTS),
            Constraint::Fill(1),
            Constraint::Length(INSPECTOR),
        ])
        .areas(area);
        render_environments(model, frame, environments, Facts::Nothing);
        render_snapshots(model, frame, snapshots);
        render_inspector(model, frame, inspector);
        return;
    }
    let [columns, inspector] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(BELOW)]).areas(area);
    if columns.width >= TWO_FROM {
        let [environments, snapshots] =
            Layout::horizontal([Constraint::Length(ENVIRONMENTS), Constraint::Fill(1)])
                .areas(columns);
        render_environments(model, frame, environments, Facts::Nothing);
        render_snapshots(model, frame, snapshots);
    } else {
        render_snapshots(model, frame, columns);
    }
    render_inspector(model, frame, inspector);
}

/// The snapshots of the environment open: the timeline, or while a filter
/// ranks them, the list of the matches.
fn render_snapshots(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let Some(timeline) = model.timeline() else {
        screens::snapshots(model, frame, area);
        return;
    };
    let width = lists::row_width(area);
    // Ten thousand snapshots: the rows of those shown alone, which the
    // groups' offsets find without walking the others.
    let height = lists::height(&model.snapshots, area);
    let selected = timeline.row_of(model.snapshots.selected);
    let first = (selected + 1).saturating_sub(height);
    let rows = timeline
        .window(first, height)
        .into_iter()
        .filter_map(|row| match row {
            Row::Header(group) => Some(header(model, group, width)),
            Row::Snapshot(position) => {
                let ranked = model.snapshots.matches.get(position)?;
                screens::snapshot_row(model, ranked, width.saturating_sub(2))
            }
        })
        .collect();
    let (header, count) = screens::snapshots_total(model);
    let (title, hint) = screens::snapshots_empty(model);
    let empty = (title.as_str(), hint.as_str());
    let total = (header.as_str(), count);
    let rows = (Some(selected - first), rows);
    lists::render_rows(model, frame, area, total, &model.snapshots, rows, empty);
}

/// The header of `group`, `width` columns wide: its period, then how many
/// snapshots it holds and the space they hold, right-aligned, and whether
/// it is folded.
fn header(model: &Model, group: &Group, width: usize) -> Line<'static> {
    let theme = model.theme;
    let facts = group.facts().join(theme.dot());
    let room = width.saturating_sub(facts.chars().count() + 2);
    let label = fit(&group.period.label(), room, theme.is_ascii());
    let gap = width.saturating_sub(label.chars().count() + facts.chars().count());
    Line::from(vec![
        Span::styled(label, theme.style(Role::Header)),
        Span::raw(" ".repeat(gap.max(2))),
        Span::styled(facts, theme.style(Role::Muted)),
    ])
}

/// The inspector of the snapshot selected, or of the folded group whose
/// header is.
fn render_inspector(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let width = usize::from(area.width.saturating_sub(4));
    let (title, facts) = if let Some(group) = model.folded_group() {
        (group.period.label(), group_facts(group))
    } else if let Some(snapshot) = model.selected_snapshot() {
        (
            snapshot.name.name().to_owned(),
            snapshot_facts(model, snapshot),
        )
    } else {
        main_view::render_nothing(model, frame, area);
        return;
    };
    let label = model
        .selected_snapshot()
        .filter(|snapshot| model.marked.contains(&snapshot.name))
        .map(|_| model.theme.glyph(Glyph::Marked).to_owned());
    let block = main_view::inspector(model, &title, label);
    let mut lines = main_view::fact_lines(model, facts, width);
    let steps = super::next_steps(model, width);
    let room = usize::from(area.height.saturating_sub(2));
    if !steps.is_empty() && lines.len() + steps.len() < room {
        lines.push(Line::default());
        lines.extend(steps);
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// What the inspector says of `snapshot`: its environment, when it was
/// taken, in UTC as every time of the menu, and the space it holds.
fn snapshot_facts(model: &Model, snapshot: &SnapshotInfo) -> Vec<main_view::Fact> {
    let minutes = snapshot.created / 60 % (24 * 60);
    let time = format!("{:02}:{:02}", minutes / 60, minutes % 60);
    let taken = text!(
        "fact-taken.when",
        date = date(snapshot.created),
        time = time,
        age = age(model.snapshot.now, snapshot.created)
    );
    vec![
        (
            text!("fact-environment"),
            vec![Span::raw(snapshot.name.dataset().to_string())],
        ),
        (text!("fact-taken"), vec![Span::raw(taken)]),
        (text!("fact-used"), vec![Span::raw(size(snapshot.used))]),
    ]
}

/// What the inspector says of a folded `group`: how many snapshots it
/// holds and the space they hold.
fn group_facts(group: &Group) -> Vec<main_view::Fact> {
    vec![
        (
            text!("fact-snapshots"),
            vec![Span::raw(text!("timeline-count", count = group.count))],
        ),
        (text!("fact-used"), vec![Span::raw(size(group.used))]),
    ]
}

#[cfg(test)]
mod tests;
