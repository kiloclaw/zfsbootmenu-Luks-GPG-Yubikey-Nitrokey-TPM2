//! The main view (`docs/dev/menu-design/apple-hig.md`, rules 2 to 4 and 14):
//! the countdown banner, the boot environments as a grouped list, and the
//! inspector, to the right from 100 columns (split view), below otherwise.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph};
use zbm_app::Encryption;

use super::{health, lists};
use crate::menu::format::{age, fit};
use crate::menu::model::Model;
use crate::menu::theme::Role;
use crate::menu::world::{Environment, Health, Origin};
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
fn facts(model: &Model, environment: &Environment) -> Vec<Span<'static>> {
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
    let kernel = environment.boot_kernel().map_or_else(
        || text!("kernel-none"),
        |k| k.version.printable().to_string(),
    );
    facts.push(Span::styled(kernel, theme.style(Role::Muted)));
    facts
}

/// The inspector of the selected environment: its name as the title,
/// `default` on the right, the facts that decide whether to boot it; in
/// the workspace, its next steps first when tall.
pub(super) fn render_detail(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let Some(environment) = model.selected_environment() else {
        let block = lists::block(theme, &text!("details-title"));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let title = text!("details-none");
        lists::empty_state(theme, frame, inner, (title.as_str(), ""));
        return;
    };
    let mut block = Block::bordered()
        .border_set(theme.border())
        .border_style(theme.style(Role::Border))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", environment.name),
            theme.style(Role::Title),
        ));
    if environment.is_default {
        let label = Span::styled(
            format!(" {} ", text!("label-default")),
            theme.style(Role::Default),
        );
        block = block.title_top(Line::from(label).right_aligned());
    }
    let width = usize::from(area.width.saturating_sub(4));
    let clip = |line| lists::clip(line, width, theme.is_ascii());
    let mut lines: Vec<Line<'static>> = detail_lines(model, environment)
        .into_iter()
        .map(clip)
        .collect();
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

/// When the pane is tall: the newest snapshots, their ages right-aligned.
fn snapshot_lines(model: &Model, environment: &Environment, width: usize) -> Vec<Line<'static>> {
    let theme = model.theme;
    let mut newest: Vec<_> = environment.snapshots.iter().collect();
    newest.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.created));
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

/// The facts of an environment, most important first: the pane shows as
/// many as fit.
fn detail_lines(model: &Model, environment: &Environment) -> Vec<Line<'static>> {
    let theme = model.theme;
    let dot = theme.dot();
    let mut lines: Vec<Line<'static>> = health::facts(model, environment)
        .into_iter()
        .map(|(label, value)| fact(model, &label, value))
        .collect();
    lines.extend(pool_fact(model, environment));
    let kernel = environment.boot_kernel().map_or_else(
        || text!("value-none"),
        |k| k.version.printable().to_string(),
    );
    let count = environment.kernels.len();
    let kernels = text!("fact-kernel.count", count = count);
    lines.push(fact(
        model,
        &text!("fact-kernel"),
        vec![Span::raw(format!("{kernel}{dot}{kernels}"))],
    ));
    let snapshots = match environment.snapshots.iter().max_by_key(|s| s.created) {
        Some(newest) => format!(
            "{}{dot}{}",
            environment.snapshots.len(),
            text!(
                "fact-snapshots.newest",
                snapshot = newest.name.name(),
                age = age(model.snapshot.now, newest.created)
            )
        ),
        None => text!("value-none"),
    };
    lines.push(fact(
        model,
        &text!("fact-snapshots"),
        vec![Span::raw(snapshots)],
    ));
    lines.push(fact(
        model,
        &text!("fact-encryption"),
        encryption(model, environment),
    ));
    lines.push(fact(
        model,
        &text!("fact-command-line"),
        command_line(model, environment),
    ));
    lines
}

/// The pool of an environment, its health and whether it is writable.
fn pool_fact(model: &Model, environment: &Environment) -> Option<Line<'static>> {
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
    Some(fact(
        model,
        &text!("fact-pool"),
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
    let origin = if line == environment.command_line.value {
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
