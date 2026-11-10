//! The main view (`docs/dev/menu-design/apple-hig.md`, rules 2 to 4 and 14):
//! the countdown banner, the boot environments as a grouped list, and the
//! inspector, to the right from 100 columns (split view), below otherwise.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph};
use zbm_app::Encryption;

use super::{health, history, lists};
use crate::menu::format::{age, fit};
use crate::menu::keymap::Map;
use crate::menu::model::{Model, own_command_line};
use crate::menu::theme::Role;
use crate::menu::world::{Environment, Health, Origin, SnapshotInfo};
use crate::text::text;

/// From this width the inspector is to the right of the list.
const WIDE: u16 = 100;

/// The width of the inspector's labels, right-aligned.
const LABEL: usize = 12;

/// Draws the main view.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let area = render_banner(model, frame, area);
    let (list, detail) = if area.width >= WIDE {
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                .areas(area);
        (list, detail)
    } else {
        let lines = if area.height >= 17 { 9 } else { 5 };
        let [list, detail] =
            Layout::vertical([Constraint::Min(4), Constraint::Length(lines)]).areas(area);
        (list, detail)
    };
    render_list(model, frame, list);
    render_detail(model, frame, detail);
}

/// Draws the countdown banner at the top of `area`, while it runs; the rest
/// of `area`.
pub(super) fn render_banner(model: &Model, frame: &mut Frame<'_>, area: Rect) -> Rect {
    let banner = health::banner(model);
    let [top, rest] = Layout::vertical([
        Constraint::Length(u16::from(banner.is_some())),
        Constraint::Min(1),
    ])
    .areas(area);
    if let Some((forms, role)) = banner {
        let width = usize::from(top.width);
        let text = forms
            .iter()
            .find(|form| form.chars().count() <= width)
            .or(forms.last())
            .cloned()
            .unwrap_or_default();
        frame.render_widget(Paragraph::new(health::banner_line(model, &text, role)), top);
    }
    rest
}

/// The boot environments: the name, then its labels and the kernel that
/// boots, right-aligned.
pub(super) fn render_list(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let environments = &model.snapshot.environments;
    let width = lists::row_width(area);
    let rows = model
        .environments
        .matches
        .iter()
        .filter_map(|ranked| {
            let environment = environments.get(ranked.index)?;
            let facts = facts(model, environment);
            let room: usize =
                width.saturating_sub(facts.iter().map(Span::width).sum::<usize>() + 2);
            let name = fit(environment.name.as_str(), room, theme.is_ascii());
            Some(lists::row(theme, &name, &ranked.positions, width, facts))
        })
        .collect();
    let (title, hint) = (text!("environments-none"), text!("environments-none.text"));
    let empty = (title.as_str(), hint.as_str());
    let header = text!("view-main");
    let header = (header.as_str(), environments.len());
    lists::render(model, frame, area, header, &model.environments, rows, empty);
}

/// The facts of a row: problems, `next boot`, `locked`, `default`, and the
/// kernel that boots.
pub(super) fn facts(model: &Model, environment: &Environment) -> Vec<Span<'static>> {
    let mut facts = labels(model, environment);
    let kernel = environment.boot_kernel().map_or_else(
        || text!("kernel-none"),
        |k| k.version.printable().to_string(),
    );
    facts.push(Span::styled(kernel, model.theme.style(Role::Muted)));
    facts
}

/// The labels of a row, each followed by two spaces: problems, `next
/// boot`, `locked` and `default`.
pub(super) fn labels(model: &Model, environment: &Environment) -> Vec<Span<'static>> {
    let theme = model.theme;
    let mut facts = health::badges(model, environment);
    if environment.encryption == Encryption::Locked {
        facts.push(Span::styled(
            format!("{}  ", text!("label-locked")),
            theme.style(Role::Locked),
        ));
    }
    if environment.is_default {
        facts.push(Span::styled(
            format!("{}  ", text!("label-default")),
            theme.style(Role::Default),
        ));
    }
    facts
}

/// The inspector with nothing selected.
pub(super) fn render_nothing(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let block = lists::block(theme, &text!("details-title"));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let title = text!("details-none");
    lists::empty_state(theme, frame, inner, (title.as_str(), ""));
}

/// The inspector's box: `title`, and `label` on the right.
pub(super) fn inspector(model: &Model, title: &str, label: Option<String>) -> Block<'static> {
    let theme = model.theme;
    let block = Block::bordered()
        .border_set(theme.border())
        .border_style(theme.style(Role::Border))
        .padding(Padding::horizontal(1))
        .title(Span::styled(format!(" {title} "), theme.style(Role::Title)));
    match label {
        Some(label) => {
            let label = Span::styled(format!(" {label} "), theme.style(Role::Default));
            block.title_top(Line::from(label).right_aligned())
        }
        None => block,
    }
}

/// The facts as the inspector draws them in lines `width` columns wide:
/// in the workspace their values wrap, else they are cut.
pub(super) fn fact_lines(model: &Model, facts: Vec<Fact>, width: usize) -> Vec<Line<'static>> {
    let ascii = model.theme.is_ascii();
    if model.map == Map::Workspace {
        return facts
            .into_iter()
            .flat_map(|(label, value)| wrapped_fact(model, &label, &value, width))
            .collect();
    }
    facts
        .into_iter()
        .map(|(label, value)| lists::clip(fact(model, &label, value), width, ascii))
        .collect()
}

/// The inspector of the selected environment: its name as the title,
/// `default` on the right, the facts that decide whether to boot it; in
/// the workspace, its next steps first when tall.
pub(super) fn render_detail(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let Some(environment) = model.selected_environment() else {
        render_nothing(model, frame, area);
        return;
    };
    let default = environment.is_default.then(|| text!("label-default"));
    let block = inspector(model, environment.name.as_str(), default);
    let width = usize::from(area.width.saturating_sub(4));
    let mut lines = fact_lines(model, detail_facts(model, environment), width);
    let room = usize::from(area.height.saturating_sub(2));
    for section in [
        super::workspace::next_steps(model, width),
        health::check_lines(model, environment, width),
        kernel_lines(model, environment, width),
        snapshot_lines(model, environment, width),
    ] {
        if !section.is_empty() && lines.len() + section.len() < room {
            lines.push(Line::default());
            lines.extend(section);
        }
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// When the pane is tall: the kernels, the one that boots labelled.
fn kernel_lines(model: &Model, environment: &Environment, width: usize) -> Vec<Line<'static>> {
    let theme = model.theme;
    let boots = environment.boot_kernel().map(|k| k.version.clone());
    let mut lines = vec![Line::styled(
        text!("details-kernels"),
        theme.style(Role::Header),
    )];
    lines.extend(environment.kernels.iter().map(|kernel| {
        let label = if boots.as_ref() == Some(&kernel.version) {
            vec![Span::styled(
                text!("label-boots"),
                theme.style(Role::Default),
            )]
        } else {
            Vec::new()
        };
        let version = kernel.version.printable().to_string();
        let version = fit(&version, width.saturating_sub(9), theme.is_ascii());
        let mut line = lists::row(theme, &version, &[], width.saturating_sub(2), label);
        line.spans.insert(0, Span::raw("  "));
        line
    }));
    lines
}

/// How many snapshots the tall pane lists.
const NEWEST: usize = 5;

/// The [`NEWEST`] snapshots of `snapshots`, newest first, without
/// sorting them all.
fn newest_of(snapshots: &[SnapshotInfo]) -> Vec<&SnapshotInfo> {
    let mut newest: Vec<&SnapshotInfo> = Vec::with_capacity(NEWEST + 1);
    for snapshot in snapshots {
        let at = newest.partition_point(|kept| kept.created >= snapshot.created);
        if at < NEWEST {
            newest.insert(at, snapshot);
            newest.truncate(NEWEST);
        }
    }
    newest
}

/// When the pane is tall: the newest snapshots, their ages right-aligned:
/// those read, else the newest alone, which every snapshot of the machine
/// holds.
fn snapshot_lines(model: &Model, environment: &Environment, width: usize) -> Vec<Line<'static>> {
    let theme = model.theme;
    let newest = match model.loaded_snapshots(&environment.name) {
        Some(loaded) => newest_of(loaded),
        None => environment.newest_snapshot.iter().collect(),
    };
    let mut lines = vec![Line::styled(
        text!("details-newest-snapshots"),
        theme.style(Role::Header),
    )];
    lines.extend(newest.iter().take(NEWEST).map(|snapshot| {
        let when = age(model.snapshot.now, snapshot.created);
        let room = width.saturating_sub(when.chars().count() + 4);
        let name = fit(snapshot.name.name(), room, theme.is_ascii());
        let facts = vec![Span::styled(when, theme.style(Role::Muted))];
        let mut line = lists::row(theme, &name, &[], width.saturating_sub(2), facts);
        line.spans.insert(0, Span::raw("  "));
        line
    }));
    lines
}

/// The inspector of the kernel the workspace's kernels column selects:
/// its version as the title, `default` or `boots` on the right, its
/// environment and the command line it boots with, then its next steps
/// when they fit.
pub(super) fn render_kernel_detail(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let (Some(environment), Some(kernel)) = (model.focused(), model.selected_kernel()) else {
        render_nothing(model, frame, area);
        return;
    };
    let boots = environment.boot_kernel() == Some(kernel);
    let label = if kernel.is_default {
        Some(text!("label-default"))
    } else {
        boots.then(|| text!("label-boots"))
    };
    let version = kernel.version.printable().to_string();
    let block = inspector(model, &version, label);
    let width = usize::from(area.width.saturating_sub(4));
    let mut facts = vec![(
        text!("fact-environment"),
        vec![Span::raw(environment.name.to_string())],
    )];
    facts.extend(pool_fact(model, environment));
    facts.push((text!("fact-command-line"), command_line(model, environment)));
    let mut lines = fact_lines(model, facts, width);
    let steps = super::workspace::next_steps(model, width);
    let room = usize::from(area.height.saturating_sub(2));
    if !steps.is_empty() && lines.len() + steps.len() < room {
        lines.push(Line::default());
        lines.extend(steps);
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// A fact of the inspector: its label, and its value.
pub(super) type Fact = (String, Vec<Span<'static>>);

/// One row of the inspector: the label right-aligned and secondary, then
/// the value.
fn fact(model: &Model, label: &str, value: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{label:>LABEL$}  "),
        model.theme.style(Role::Muted),
    )];
    spans.extend(value);
    Line::from(spans)
}

/// The rows of a fact of the workspace's inspector, `width` columns wide:
/// the label as [`fact`] draws it, then the value, which wraps between
/// words, or within one too long for a line, each line after the first
/// under the start of the value, however wide the label.
fn wrapped_fact(
    model: &Model,
    label: &str,
    value: &[Span<'static>],
    width: usize,
) -> Vec<Line<'static>> {
    let lead = Span::styled(format!("{label:>LABEL$}  "), model.theme.style(Role::Muted));
    let indent = lead.width();
    let mut lead = Some(lead);
    wrapped(value, width.saturating_sub(indent))
        .into_iter()
        .map(|spans| {
            let indent = lead.take().unwrap_or_else(|| Span::raw(" ".repeat(indent)));
            Line::from([vec![indent], spans].concat())
        })
        .collect()
}

/// `spans` in lines of `width` columns, broken at a space where one is,
/// which the break drops, else within a word; each line's characters keep
/// their style.
fn wrapped(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let mut left: Vec<(char, ratatui::style::Style)> = spans
        .iter()
        .flat_map(|span| span.content.chars().map(|c| (c, span.style)))
        .collect();
    let width = width.max(1);
    let mut lines = Vec::new();
    loop {
        if left.len() <= width {
            lines.push(styled(&left));
            return lines;
        }
        let space = left
            .get(..=width)
            .unwrap_or_default()
            .iter()
            .rposition(|(c, _)| *c == ' ')
            .filter(|at| *at > 0);
        let (end, next) = space.map_or((width, width), |at| (at, at + 1));
        lines.push(styled(left.get(..end).unwrap_or_default()));
        left.drain(..next.min(left.len()));
    }
}

/// `chars` as spans, one for each run of one style.
fn styled(chars: &[(char, ratatui::style::Style)]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (c, style) in chars {
        match spans.last_mut() {
            Some(span) if span.style == *style => span.content.to_mut().push(*c),
            _ => spans.push(Span::styled(c.to_string(), *style)),
        }
    }
    spans
}

/// The facts of an environment, most important first: the pane shows as
/// many as fit.
fn detail_facts(model: &Model, environment: &Environment) -> Vec<Fact> {
    let dot = model.theme.dot();
    let mut lines = health::facts(model, environment);
    lines.extend(pool_fact(model, environment));
    let kernel = environment.boot_kernel().map_or_else(
        || text!("value-none"),
        |k| k.version.printable().to_string(),
    );
    let count = environment.kernels.len();
    let kernels = text!("fact-kernel.count", count = count);
    lines.push((
        text!("fact-kernel"),
        vec![Span::raw(format!("{kernel}{dot}{kernels}"))],
    ));
    lines.extend(last_boot(model, environment));
    let snapshots = match &environment.newest_snapshot {
        Some(newest) => format!(
            "{}{dot}{}",
            environment.snapshot_count,
            text!(
                "fact-snapshots.newest",
                snapshot = newest.name.name(),
                age = age(model.snapshot.now, newest.created)
            )
        ),
        None => text!("value-none"),
    };
    lines.push((text!("fact-snapshots"), vec![Span::raw(snapshots)]));
    lines.push((text!("fact-encryption"), encryption(model, environment)));
    lines.push((text!("fact-command-line"), command_line(model, environment)));
    lines
}

/// When the workspace's boot history last saw `environment` loaded, and
/// with which kernel, from its newest record of it.
fn last_boot(model: &Model, environment: &Environment) -> Option<Fact> {
    if model.map != Map::Workspace {
        return None;
    }
    let record = model.history.as_ref()?.last_boot_of(&environment.name)?;
    let loaded = text!(
        "fact-last-boot.loaded",
        when = history::when(record.time),
        kernel = history::kernel(record)
    );
    Some((text!("fact-last-boot"), vec![Span::raw(loaded)]))
}

/// The pool of an environment, its health and whether it is writable.
fn pool_fact(model: &Model, environment: &Environment) -> Option<Fact> {
    let theme = model.theme;
    let dot = theme.dot();
    let pool = model
        .snapshot
        .pools
        .iter()
        .find(|pool| pool.name == environment.name.pool())?;
    let role = if pool.health == Health::Online {
        Role::Good
    } else {
        Role::Warning
    };
    let mode = if pool.writable {
        text!("pool-read-write")
    } else {
        text!("pool-read-only")
    };
    Some((
        text!("fact-pool"),
        vec![
            Span::raw(format!("{}{dot}", pool.name)),
            Span::styled(pool.health.as_str(), theme.style(role)),
            Span::raw(format!("{dot}{mode}")),
        ],
    ))
}

fn encryption(model: &Model, environment: &Environment) -> Vec<Span<'static>> {
    let theme = model.theme;
    match environment.encryption {
        Encryption::None => vec![Span::raw(text!("value-none"))],
        Encryption::Unlocked => vec![Span::raw(text!("encryption-unlocked"))],
        Encryption::Locked => vec![
            Span::styled(text!("label-locked"), theme.style(Role::Locked)),
            Span::raw(text!("encryption-locked")),
        ],
    }
}

fn command_line(model: &Model, environment: &Environment) -> Vec<Span<'static>> {
    let line = model.command_line(environment);
    let origin = if line == *own_command_line(environment) {
        match &environment.command_line.origin {
            Origin::Local => text!("origin-local"),
            Origin::Inherited(from) => text!("origin-inherited", from = from.to_string()),
            Origin::Default => text!("label-default"),
        }
    } else {
        text!("origin-edited")
    };
    let shown = if line.get().is_empty() {
        text!("value-empty")
    } else {
        line.printable().to_string()
    };
    let dot = model.theme.dot();
    vec![
        Span::raw(shown),
        Span::styled(format!("{dot}{origin}"), model.theme.style(Role::Muted)),
    ]
}
