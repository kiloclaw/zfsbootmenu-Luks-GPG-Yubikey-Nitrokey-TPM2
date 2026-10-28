//! The views opened from the main view, and start-up, as grouped lists
//! (`docs/dev/menu-design/apple-hig.md`).

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use zbm_types::Printable;

use super::lists;
use crate::menu::format::{age, date, fit, size};
use crate::menu::keymap::{self, Context};
use crate::menu::model::{Model, RECOVERY};
use crate::menu::theme::{Glyph, Role};
use crate::menu::world::{Health, Level, StageState, VolumeState};
use crate::text::{self, Id, text};

/// Start-up as a checklist (rule 10): each stage with a glyph, and the
/// step count in the header.
pub(super) fn startup(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let stages = &model.snapshot.startup;
    let lines: Vec<Line<'static>> = stages
        .iter()
        .map(|stage| {
            let (glyph, role) = match &stage.state {
                StageState::Pending => (" ", Role::Muted),
                StageState::Running => (theme.glyph(Glyph::Running), Role::Accent),
                StageState::Done => (theme.glyph(Glyph::Done), Role::Good),
                StageState::Failed(_) => (theme.glyph(Glyph::Failed), Role::Danger),
                StageState::Skipped => (theme.glyph(Glyph::Skipped), Role::Muted),
            };
            let name = if stage.state == StageState::Pending {
                Span::styled(stage.name.clone(), theme.style(Role::Muted))
            } else {
                Span::raw(stage.name.clone())
            };
            let mut spans = vec![Span::styled(format!("{glyph:<3}"), theme.style(role)), name];
            if let StageState::Failed(reason) = &stage.state {
                spans.push(Span::styled(
                    format!(": {reason}"),
                    theme.style(Role::Danger),
                ));
            }
            Line::from(spans)
        })
        .collect();
    let current = stages
        .iter()
        .position(|stage| matches!(stage.state, StageState::Pending | StageState::Running))
        .map_or(stages.len(), |index| index + 1);
    let count = text!("startup-step", current = current, total = stages.len());
    let block = lists::counted(theme, &text!("startup-title"), &count);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The snapshots of the focused environment: name, then age and size.
pub(super) fn snapshots(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let snapshots = model.focus_snapshots();
    let width = lists::row_width(area).saturating_sub(2);
    let rows = model
        .snapshots
        .matches
        .iter()
        .filter_map(|ranked| {
            let snapshot = snapshots.get(ranked.index)?;
            let marked = model.marked.contains(&snapshot.name);
            let facts = vec![Span::styled(
                format!(
                    "{:>10}  {:>9}",
                    age(model.snapshot.now, snapshot.created),
                    size(snapshot.used)
                ),
                theme.style(Role::Muted),
            )];
            let name = fit(
                snapshot.name.name(),
                width.saturating_sub(23),
                theme.is_ascii(),
            );
            let mut line = lists::row(theme, &name, &ranked.positions, width, facts);
            let mark = if marked {
                theme.glyph(Glyph::Marked)
            } else {
                " "
            };
            line.spans.insert(
                0,
                Span::styled(format!("{mark} "), theme.style(Role::Accent)),
            );
            Some(line)
        })
        .collect();
    let header = text!("snapshots-by", order = text::get(model.sort.id()));
    let (title, hint) = (text!("snapshots-none"), text!("snapshots-none.text"));
    let empty = (title.as_str(), hint.as_str());
    let total = (header.as_str(), snapshots.len());
    lists::render(model, frame, area, total, &model.snapshots, rows, empty);
}

/// The kernels of the focused environment, the default labelled.
pub(super) fn kernels(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let Some(environment) = model.focused() else {
        return;
    };
    let width = lists::row_width(area);
    let rows = model
        .kernels
        .matches
        .iter()
        .filter_map(|ranked| {
            let kernel = environment.kernels.get(ranked.index)?;
            let facts = if kernel.is_default {
                vec![Span::styled(
                    text!("label-default"),
                    theme.style(Role::Default),
                )]
            } else {
                Vec::new()
            };
            Some(lists::row(
                theme,
                &kernel.version.printable().to_string(),
                &ranked.positions,
                width,
                facts,
            ))
        })
        .collect();
    let (title, hint) = (text!("kernels-none"), text!("kernels-none.text"));
    let empty = (title.as_str(), hint.as_str());
    let header = text!("kernels-title");
    let total = (header.as_str(), environment.kernels.len());
    lists::render(model, frame, area, total, &model.kernels, rows, empty);
}

/// The command line editor.
pub(super) fn command_line(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let Some(editor) = &model.editor else {
        return;
    };
    let header = match &editor.kernel {
        Some(kernel) => text!("command-line-with", kernel = kernel.printable().to_string()),
        None => text!("command-line-title"),
    };
    let block = lists::block(theme, &header);
    let inner = block.inner(area);
    let width = usize::from(inner.width.max(1));
    // The line wraps at the last column, not between words, so that the
    // cursor is where the next character goes. The text came from the pool,
    // so it is shown escaped, the cursor after the escapes.
    let shown = Printable(&editor.text).to_string();
    let mut lines = wrapped(&shown, width);
    lines.extend([
        Line::default(),
        Line::styled(text!("command-line-hint"), theme.style(Role::Muted)),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
    let length = shown.chars().count();
    let (row, column) = (length / width, length % width);
    let row = u16::try_from(row).unwrap_or(u16::MAX);
    let column = u16::try_from(column).unwrap_or(u16::MAX);
    frame.set_cursor_position(Position::new(
        inner.x.saturating_add(column),
        inner.y.saturating_add(row),
    ));
}

/// `text` in lines of `width` characters, the last one shorter; one empty
/// line for no text.
pub(super) fn wrapped(text: &str, width: usize) -> Vec<Line<'static>> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![Line::default()];
    }
    chars
        .chunks(width.max(1))
        .map(|chunk| Line::from(chunk.iter().collect::<String>()))
        .collect()
}

/// Pools, then LUKS volumes: the name, then the state right-aligned.
pub(super) fn pools(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let snapshot = &model.snapshot;
    let width = lists::row_width(area);
    let rows: Vec<Line<'static>> = model
        .pools
        .matches
        .iter()
        .filter_map(|ranked| pool_row(model, ranked.index, width))
        .collect();
    let total = snapshot.pools.len() + snapshot.volumes.len();
    let (title, hint) = (text!("pools-none"), text!("pools-none.text"));
    let empty = (title.as_str(), hint.as_str());
    let header = text!("pools-title");
    let header = (header.as_str(), total);
    lists::render(model, frame, area, header, &model.pools, rows, empty);
}

/// The row of the pool or volume at `index` of the pools view.
fn pool_row(model: &Model, index: usize, width: usize) -> Option<Line<'static>> {
    let theme = model.theme;
    let snapshot = &model.snapshot;
    let dot = theme.dot();
    if let Some(pool) = snapshot.pools.get(index) {
        let role = if pool.health == Health::Online {
            Role::Good
        } else {
            Role::Warning
        };
        let mode = match (pool.imported, pool.writable) {
            (false, _) => text!("pool-not-imported"),
            (true, true) => text!("pool-read-write"),
            (true, false) => text!("pool-read-only"),
        };
        let mut facts = Vec::new();
        if let Some(hostid) = &pool.foreign_hostid {
            facts.push(Span::styled(
                format!("{}{dot}", text!("pool-host", hostid = hostid.as_str())),
                theme.style(Role::Warning),
            ));
        }
        facts.push(Span::styled(
            format!("{mode}{dot}"),
            theme.style(Role::Muted),
        ));
        facts.push(Span::styled(
            format!("{:<8}", pool.health.as_str()),
            theme.style(role),
        ));
        let name = format!("{:<8}{}", text!("pool-label"), pool.name);
        return Some(lists::row(theme, &name, &[], width, facts));
    }
    let volume = snapshot
        .volumes
        .get(index.checked_sub(snapshot.pools.len())?)?;
    let (word, role) = match &volume.state {
        VolumeState::Open => (text!("volume-open"), Role::Good),
        VolumeState::Locked => (text!("volume-locked"), Role::Locked),
        VolumeState::Skipped => (text!("volume-skipped"), Role::Muted),
        VolumeState::Failed(reason) => (
            text!("volume-failed", reason = Printable(reason).to_string()),
            Role::Danger,
        ),
    };
    let mut facts = Vec::new();
    if volume.tpm2.as_ref().is_some_and(|seal| !seal.is_current()) {
        facts.push(Span::styled(
            format!("{}{dot}", text!("volume-tpm2-changed")),
            theme.style(Role::Warning),
        ));
    }
    facts.push(Span::styled(
        format!("{}{dot}", volume.device.printable()),
        theme.style(Role::Muted),
    ));
    facts.push(Span::styled(format!("{word:<8}"), theme.style(role)));
    let name = format!("{:<8}{}", text!("volume-label"), volume.name.printable());
    Some(lists::row(theme, &name, &[], width, facts))
}

/// Recovery tools.
pub(super) fn recovery(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let rows = model
        .recovery
        .matches
        .iter()
        .filter_map(|ranked| RECOVERY.get(ranked.index))
        .map(|(label, _)| Line::from(text::title(*label)))
        .collect();
    let title = text!("recovery-none");
    let empty = (title.as_str(), "");
    let header = text!("recovery-title");
    let total = (header.as_str(), RECOVERY.len());
    lists::render(model, frame, area, total, &model.recovery, rows, empty);
}

/// The log, newest last.
pub(super) fn log(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let lines: Vec<Line<'static>> = model
        .snapshot
        .log
        .entries
        .iter()
        // The scroll counts entries, which may wrap to several rows: the
        // first shown is the entry scrolled to, so the last can be reached.
        .skip(model.scroll)
        .map(|entry| {
            let (glyph, word, role) = match entry.level {
                Level::Info => (" ", text!("log-info"), Role::Muted),
                Level::Warning => (
                    theme.glyph(Glyph::Warning),
                    text!("log-warning"),
                    Role::Warning,
                ),
                Level::Error => (theme.glyph(Glyph::Failed), text!("log-error"), Role::Danger),
            };
            Line::from(vec![
                Span::styled(format!("{glyph} {word:<8}"), theme.style(role)),
                Span::raw(entry.text.printable().to_string()),
            ])
        })
        .collect();
    let count = date(model.snapshot.now);
    let block = lists::counted(theme, &text!("log-title"), &count);
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    frame.render_widget(paragraph, area);
}

/// Help: the keys of the view it was opened from, then the classic
/// bindings, each section under its header.
pub(super) fn help(model: &Model, frame: &mut Frame<'_>, area: Rect, context: Context) {
    let theme = model.theme;
    let mut lines = Vec::new();
    let mut section = |title: String, rows: Vec<(String, Id)>| {
        if rows.is_empty() {
            return;
        }
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::styled(title, theme.style(Role::Header)));
        for (keys, what) in rows {
            lines.push(Line::from(vec![
                Span::styled(format!("{keys:>16}  "), theme.style(Role::Key)),
                Span::raw(text::title(what)),
            ]));
        }
    };
    section(text!("help-this-view"), keymap::help(context));
    section(text!("help-classic"), keymap::classic_bindings(context));
    let shown = usize::from(area.height.saturating_sub(2));
    let scroll = model.scroll.min(lines.len().saturating_sub(shown));
    let header = if lines.len() > shown {
        text!("help-keys-scroll")
    } else {
        text!("help-keys")
    };
    let block = lists::block(theme, &header);
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
