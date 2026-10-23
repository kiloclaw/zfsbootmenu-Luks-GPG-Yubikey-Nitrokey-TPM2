//! The views opened from the main view, and start-up.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::lists;
use crate::menu::format::{age, date, fit, size};
use crate::menu::keymap::{self, Context};
use crate::menu::model::{Model, RECOVERY};
use crate::menu::theme::Role;
use crate::menu::world::{Health, Level, StageState, VolumeState};

/// Start-up: each stage with its state as a word.
pub(super) fn startup(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let lines: Vec<Line<'static>> = model
        .snapshot
        .startup
        .iter()
        .map(|stage| {
            let (word, role) = match &stage.state {
                StageState::Pending => ("      ".to_owned(), Role::Muted),
                StageState::Running => ("  ... ".to_owned(), Role::Title),
                StageState::Done => ("  ok  ".to_owned(), Role::Good),
                StageState::Failed(_) => ("failed".to_owned(), Role::Danger),
                StageState::Skipped => ("skip  ".to_owned(), Role::Muted),
            };
            let mut spans = vec![
                Span::raw(" ["),
                Span::styled(word, theme.style(role)),
                Span::raw("] "),
                Span::raw(stage.name.clone()),
            ];
            if let StageState::Failed(reason) = &stage.state {
                spans.push(Span::styled(
                    format!(": {reason}"),
                    theme.style(Role::Danger),
                ));
            }
            Line::from(spans)
        })
        .collect();
    let block = lists::block(theme, "Starting ZFSBootMenu");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The snapshots of the focused environment: name, age, size, marks.
pub(super) fn snapshots(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let snapshots = model.focus_snapshots();
    let limit = usize::from(area.width).saturating_sub(28);
    let width = lists::name_width(snapshots.iter().map(|s| s.name.name()), limit);
    let rows = model
        .snapshots
        .matches
        .iter()
        .filter_map(|ranked| {
            let snapshot = snapshots.get(ranked.index)?;
            let marked = model.marked.contains(&snapshot.name);
            let name = fit(snapshot.name.name(), limit, theme.is_ascii());
            let facts = vec![Span::styled(
                format!(
                    "{:>10}  {:>9}",
                    age(model.snapshot.now, snapshot.created),
                    size(snapshot.used)
                ),
                theme.style(Role::Muted),
            )];
            let mut line = lists::row(theme, &name, &ranked.positions, width, facts);
            line.spans.insert(
                0,
                Span::styled(if marked { "+ " } else { "  " }, theme.style(Role::Match)),
            );
            Some(line)
        })
        .collect();
    let title = format!(
        "Snapshots {}/{} by {}",
        model.snapshots.matches.len(),
        snapshots.len(),
        model.sort.as_str()
    );
    lists::render(model, frame, area, &title, &model.snapshots, rows);
}

/// The kernels of the focused environment, the default marked.
pub(super) fn kernels(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let Some(environment) = model.focused() else {
        return;
    };
    let rows = model
        .kernels
        .matches
        .iter()
        .filter_map(|ranked| {
            let kernel = environment.kernels.get(ranked.index)?;
            let mut spans = vec![Span::raw("  ")];
            spans.extend(lists::highlighted(
                theme,
                &kernel.version,
                &ranked.positions,
            ));
            if kernel.is_default {
                if let Some(first) = spans.first_mut() {
                    *first = Span::styled("* ", theme.style(Role::Default));
                }
                spans.push(Span::styled("  default", theme.style(Role::Default)));
            }
            Some(Line::from(spans))
        })
        .collect();
    let title = format!("Kernels of {}", environment.name);
    lists::render(model, frame, area, &title, &model.kernels, rows);
}

/// The command line editor.
pub(super) fn command_line(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let Some(editor) = &model.editor else {
        return;
    };
    let kernel = editor
        .kernel
        .as_deref()
        .map(|k| format!(" with {k}"))
        .unwrap_or_default();
    let block = lists::block(
        theme,
        &format!("Command line of {}{kernel}", editor.environment),
    );
    let inner = block.inner(area);
    let lines = vec![
        Line::from(editor.text.clone()),
        Line::default(),
        Line::styled(
            "For this boot only: Enter boots with it, Esc keeps the saved one.",
            theme.style(Role::Muted),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
    let width = usize::from(inner.width.max(1));
    let length = editor.text.chars().count();
    let (row, column) = (length / width, length % width);
    let row = u16::try_from(row).unwrap_or(u16::MAX);
    let column = u16::try_from(column).unwrap_or(u16::MAX);
    frame.set_cursor_position(Position::new(
        inner.x.saturating_add(column),
        inner.y.saturating_add(row),
    ));
}

/// Pools, then LUKS volumes.
pub(super) fn pools(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let snapshot = &model.snapshot;
    let rows: Vec<Line<'static>> = model
        .pools
        .matches
        .iter()
        .filter_map(|ranked| {
            if let Some(pool) = snapshot.pools.get(ranked.index) {
                let role = if pool.health == Health::Online {
                    Role::Good
                } else {
                    Role::Warning
                };
                let mode = match (pool.imported, pool.writable) {
                    (false, _) => "not imported",
                    (true, true) => "read/write",
                    (true, false) => "read-only",
                };
                let mut spans = vec![Span::raw(format!("pool    {:<14} ", pool.name.as_str()))];
                spans.push(Span::styled(
                    format!("{:<9}", pool.health.as_str()),
                    theme.style(role),
                ));
                spans.push(Span::raw(format!(" {mode}")));
                if let Some(hostid) = &pool.foreign_hostid {
                    spans.push(Span::styled(
                        format!(", host {hostid}"),
                        theme.style(Role::Warning),
                    ));
                }
                return Some(Line::from(spans));
            }
            let volume = snapshot.volumes.get(ranked.index - snapshot.pools.len())?;
            let (word, role) = match &volume.state {
                VolumeState::Open => ("open".to_owned(), Role::Good),
                VolumeState::Locked => ("locked".to_owned(), Role::Locked),
                VolumeState::Skipped => ("skipped".to_owned(), Role::Muted),
                VolumeState::Failed(reason) => (format!("failed: {reason}"), Role::Danger),
            };
            let methods: Vec<&str> = volume.methods.iter().map(|m| m.as_str()).collect();
            Some(Line::from(vec![
                Span::raw(format!("volume  {:<14} ", volume.name)),
                Span::styled(format!("{word:<9}"), theme.style(role)),
                Span::styled(
                    format!(" {}, {}", volume.device, methods.join(" or ")),
                    theme.style(Role::Muted),
                ),
            ]))
        })
        .collect();
    lists::render(
        model,
        frame,
        area,
        "Pools and LUKS volumes",
        &model.pools,
        rows,
    );
}

/// Recovery tools.
pub(super) fn recovery(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let rows = model
        .recovery
        .matches
        .iter()
        .filter_map(|ranked| RECOVERY.get(ranked.index))
        .map(|(label, _)| Line::from(format!("  {label}")))
        .collect();
    lists::render(model, frame, area, "Recovery tools", &model.recovery, rows);
}

/// The log, newest last.
pub(super) fn log(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let lines: Vec<Line<'static>> = model
        .snapshot
        .log
        .entries
        .iter()
        .map(|entry| {
            let (word, role) = match entry.level {
                Level::Info => ("info   ", Role::Muted),
                Level::Warning => ("warning", Role::Warning),
                Level::Error => ("error  ", Role::Danger),
            };
            Line::from(vec![
                Span::styled(format!(" {word} "), theme.style(role)),
                Span::raw(entry.text.clone()),
            ])
        })
        .collect();
    let title = format!("Log since start, {}", date(model.snapshot.now));
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(lists::block(theme, &title));
    frame.render_widget(paragraph.scroll((scrolled(model.scroll), 0)), area);
}

/// Help: the keys of the view it was opened from, the keys of every view,
/// the classic bindings.
pub(super) fn help(model: &Model, frame: &mut Frame<'_>, area: Rect, context: Context) {
    let theme = model.theme;
    let mut lines = Vec::new();
    let mut section = |title: &str, rows: Vec<(String, &str)>| {
        if rows.is_empty() {
            return;
        }
        lines.push(Line::styled(title.to_owned(), theme.style(Role::Title)));
        for (keys, what) in rows {
            lines.push(Line::from(vec![
                Span::styled(format!("  {keys:<18}"), theme.style(Role::Key)),
                Span::raw(what.to_owned()),
            ]));
        }
    };
    section("This view", keymap::help(context));
    let classic: Vec<(String, &str)> = keymap::classic_bindings(context);
    section("Classic bindings (MOD is Alt or Ctrl)", classic);
    let shown = usize::from(area.height.saturating_sub(2));
    let scroll = model.scroll.min(lines.len().saturating_sub(shown));
    let title = if lines.len() > shown {
        "Help (Up/Down scroll)"
    } else {
        "Help"
    };
    let block = lists::block(theme, title);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((scrolled(scroll), 0)),
        area,
    );
}

/// A scroll offset as ratatui takes it.
fn scrolled(lines: usize) -> u16 {
    u16::try_from(lines).unwrap_or(u16::MAX)
}
