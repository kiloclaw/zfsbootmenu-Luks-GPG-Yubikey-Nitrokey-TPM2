//! The views opened from the main view, and start-up, as grouped lists
//! (`docs/dev/menu-design/apple-hig.md`).

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use zbm_types::Printable;

use super::lists;
use crate::menu::format::{age, date, fit, size};
use crate::menu::keymap::{self, Context, Map};
use crate::menu::model::{Column, Model, RECOVERY, recovery_tools};
use crate::menu::theme::{Glyph, Role};
use crate::menu::world::{Health, Kernel, Level, PoolHost, StageState, VolumeState};
use crate::text::{self, text};

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

/// What the snapshots column says while its snapshots are not there: its
/// title and what to do; `None` once they are read.
pub(super) fn unread(model: &Model) -> Option<(String, String)> {
    match model.column() {
        Column::Loaded => None,
        Column::Reading => Some((text!("snapshots-reading"), text!("snapshots-reading.text"))),
        Column::NotRead => Some((
            text!("snapshots-not-read"),
            text!("snapshots-not-read.text"),
        )),
    }
}

/// The snapshots of the focused environment: name, then age and size;
/// while they are read, or when they are not, the column says so.
pub(super) fn snapshots(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let snapshots = model.focus_snapshots();
    let width = lists::row_width(area).saturating_sub(2);
    // Ten thousand snapshots: the rows of those shown alone.
    let window = lists::window(&model.snapshots, area);
    let first = window.start;
    let rows = model
        .snapshots
        .matches
        .get(window)
        .unwrap_or_default()
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
    let (title, hint) =
        unread(model).unwrap_or_else(|| (text!("snapshots-none"), text!("snapshots-none.text")));
    let empty = (title.as_str(), hint.as_str());
    // Until they come, the count is the one the machine read with it.
    let count = match model.column() {
        Column::Loaded => snapshots.len(),
        _ => model.focused().map_or(0, |e| e.snapshot_count),
    };
    let total = (header.as_str(), count);
    let rows = (first, rows);
    lists::render_window(model, frame, area, total, &model.snapshots, rows, empty);
}

/// The kernels of the focused environment, the default labelled.
pub(super) fn kernels(model: &Model, frame: &mut Frame<'_>, area: Rect) {
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
            Some(kernel_row(model, kernel, &ranked.positions, width))
        })
        .collect();
    let (title, hint) = (text!("kernels-none"), text!("kernels-none.text"));
    let empty = (title.as_str(), hint.as_str());
    let header = text!("kernels-title");
    let total = (header.as_str(), environment.kernels.len());
    lists::render(model, frame, area, total, &model.kernels, rows, empty);
}

/// The row of `kernel`, `width` columns wide: its version, with the
/// characters at `positions` matched, and `default` when it is. In the
/// workspace the version is cut when both do not fit.
pub(super) fn kernel_row(
    model: &Model,
    kernel: &Kernel,
    positions: &[usize],
    width: usize,
) -> Line<'static> {
    let theme = model.theme;
    let facts = if kernel.is_default {
        vec![Span::styled(
            text!("label-default"),
            theme.style(Role::Default),
        )]
    } else {
        Vec::new()
    };
    let mut version = kernel.version.printable().to_string();
    if model.map == Map::Workspace {
        let labels = facts.iter().map(Span::width).sum::<usize>();
        let room = if labels == 0 {
            width
        } else {
            width.saturating_sub(labels + 2)
        };
        version = fit(&version, room.max(1), theme.is_ascii());
    }
    lists::row(theme, &version, positions, width, facts)
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

/// The host that used a pool last as its row says it: `host deadbeef`, or
/// whatever else is known of it.
fn host_id(host: &PoolHost) -> String {
    match &host.hostid {
        Some(hostid) => text!("pool-host", hostid = hostid.to_string()),
        None => crate::menu::model::host_text(host),
    }
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
        let mode = crate::menu::model::pool_mode(pool);
        let mut facts = Vec::new();
        if let Some(host) = &pool.host {
            facts.push(Span::styled(
                format!("{}{dot}", host_id(host)),
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
        let name = crate::menu::model::pool_name(pool, model.import_id(pool));
        let name = format!("{:<8}{name}", text!("pool-label"));
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
    let total = (header.as_str(), recovery_tools(&model.snapshot).len());
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
/// bindings, each section under its header; in the workspace, the version
/// of the ZFS module running on the right of the header, and the classic
/// bindings named as the aliases they are.
pub(super) fn help(model: &Model, frame: &mut Frame<'_>, area: Rect, context: Context) {
    let theme = model.theme;
    let workspace = model.map == Map::Workspace;
    let lines = help_lines(model, context);
    let shown = usize::from(area.height.saturating_sub(2));
    let scroll = model.scroll.min(lines.len().saturating_sub(shown));
    let header = if lines.len() > shown {
        text!("help-keys-scroll")
    } else {
        text!("help-keys")
    };
    let block = match model.snapshot.zfs_version.as_ref().filter(|_| workspace) {
        Some(version) => {
            // The header keeps its room: the corners, the spaces around
            // both and one column of border between them.
            let room =
                usize::from(area.width).saturating_sub(header.to_uppercase().chars().count() + 7);
            let version = text!(
                "help-zfs-version",
                version = version.printable().to_string()
            );
            lists::counted(theme, &header, &fit(&version, room, theme.is_ascii()))
        }
        None => lists::block(theme, &header),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((scrolled(scroll), 0)),
        area,
    );
}

/// The lines of the help view: the bindings of `context`, then the
/// aliases or the classic bindings, each section under its header. In the
/// workspace the keys' column is as wide as its widest keys.
fn help_lines(model: &Model, context: Context) -> Vec<Line<'static>> {
    let theme = model.theme;
    let workspace = model.map == Map::Workspace;
    let offered = |intent| keymap::is_offered(context, intent, model);
    let aliases = if workspace {
        text!("help-aliases")
    } else {
        text!("help-classic")
    };
    let sections = [
        (
            text!("help-this-view"),
            keymap::help(model.map, context, &offered),
        ),
        (aliases, keymap::classic_bindings(context, &offered)),
    ];
    let longest = sections.iter().flat_map(|(_, rows)| rows);
    let longest = longest.map(|(keys, _)| keys.chars().count()).max();
    let column = longest.filter(|_| workspace).unwrap_or(0).max(16);
    let mut lines = Vec::new();
    for (title, rows) in sections.into_iter().filter(|(_, rows)| !rows.is_empty()) {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::styled(title, theme.style(Role::Header)));
        for (keys, what) in rows {
            lines.push(Line::from(vec![
                Span::styled(format!("{keys:>column$}  "), theme.style(Role::Key)),
                Span::raw(text::title(what)),
            ]));
        }
    }
    lines
}

/// A scroll offset as ratatui takes it.
fn scrolled(lines: usize) -> u16 {
    u16::try_from(lines).unwrap_or(u16::MAX)
}
