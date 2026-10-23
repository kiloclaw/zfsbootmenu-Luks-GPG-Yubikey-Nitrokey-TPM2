//! The main view: the boot environments and the detail pane, to the right
//! from 100 columns, below the list otherwise.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use zbm_app::Encryption;

use super::lists;
use crate::menu::format::{age, fit};
use crate::menu::model::Model;
use crate::menu::theme::Role;
use crate::menu::world::{Environment, Origin};

/// From this width the detail pane is to the right of the list.
const WIDE: u16 = 100;

/// Draws the main view.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let (list, detail) = if area.width >= WIDE {
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(area);
        (list, detail)
    } else {
        let lines = if area.height >= 18 { 8 } else { 5 };
        let [list, detail] =
            Layout::vertical([Constraint::Min(4), Constraint::Length(lines)]).areas(area);
        (list, detail)
    };
    render_list(model, frame, list);
    render_detail(model, frame, detail);
}

/// The list of boot environments: default marked, the kernel that boots,
/// and the encryption.
fn render_list(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let environments = &model.snapshot.environments;
    let limit = usize::from(area.width).saturating_sub(24);
    let width = lists::name_width(environments.iter().map(|e| e.name.as_str()), limit);
    let rows = model
        .environments
        .matches
        .iter()
        .filter_map(|ranked| {
            let environment = environments.get(ranked.index)?;
            let name = fit(environment.name.as_str(), limit, theme.is_ascii());
            let mut line = lists::row(
                theme,
                &name,
                &ranked.positions,
                width,
                facts(model, environment),
            );
            let mark = if environment.is_default {
                Span::styled("* ", theme.style(Role::Default))
            } else {
                Span::raw("  ")
            };
            line.spans.insert(0, mark);
            Some(line)
        })
        .collect();
    let title = format!(
        "Boot environments {}/{}",
        model.environments.matches.len(),
        environments.len()
    );
    lists::render(model, frame, area, &title, &model.environments, rows);
}

/// The facts of a row: the kernel that boots, and `locked`.
fn facts(model: &Model, environment: &Environment) -> Vec<Span<'static>> {
    let theme = model.theme;
    let kernel = environment
        .boot_kernel()
        .map(|k| k.version.clone())
        .unwrap_or_default();
    let mut facts = vec![Span::styled(kernel, theme.style(Role::Muted))];
    if environment.encryption == Encryption::Locked {
        facts.push(Span::raw(" "));
        facts.push(Span::styled("locked", theme.style(Role::Locked)));
    }
    facts
}

/// The detail pane: what decides whether to boot the selection.
fn render_detail(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let Some(environment) = model.selected_environment() else {
        let block = lists::block(theme, "Details");
        frame.render_widget(Paragraph::new("no boot environment").block(block), area);
        return;
    };
    let mut title = environment.name.to_string();
    if environment.is_default {
        title.push_str(" (default)");
    }
    let block = lists::block(theme, &title);
    let mut lines = detail_lines(model, environment);
    let room = usize::from(area.height.saturating_sub(2));
    let width = usize::from(area.width.saturating_sub(4));
    for section in [
        kernel_lines(model, environment, width),
        snapshot_lines(model, environment, width),
    ] {
        if lines.len() + section.len() < room {
            lines.push(Line::default());
            lines.extend(section);
        }
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// When the pane is tall: the kernels, the one that boots marked.
fn kernel_lines(model: &Model, environment: &Environment, width: usize) -> Vec<Line<'static>> {
    let theme = model.theme;
    let boots = environment.boot_kernel().map(|k| k.version.clone());
    let mut lines = vec![Line::styled("Kernels", theme.style(Role::Title))];
    lines.extend(environment.kernels.iter().map(|kernel| {
        let version = fit(&kernel.version, width, theme.is_ascii());
        if boots.as_ref() == Some(&kernel.version) {
            Line::from(vec![
                Span::styled("* ", theme.style(Role::Default)),
                Span::raw(version),
            ])
        } else {
            Line::from(format!("  {version}"))
        }
    }));
    lines
}

/// How many snapshots the tall pane lists.
const NEWEST: usize = 5;

/// When the pane is tall: the newest snapshots and their ages.
fn snapshot_lines(model: &Model, environment: &Environment, width: usize) -> Vec<Line<'static>> {
    let theme = model.theme;
    let mut newest: Vec<_> = environment.snapshots.iter().collect();
    newest.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.created));
    let mut lines = vec![Line::styled("Newest snapshots", theme.style(Role::Title))];
    lines.extend(newest.iter().take(NEWEST).map(|snapshot| {
        let when = age(model.snapshot.now, snapshot.created);
        let name = fit(
            snapshot.name.name(),
            width.saturating_sub(when.len() + 4),
            theme.is_ascii(),
        );
        Line::from(vec![
            Span::raw(format!("  {name}  ")),
            Span::styled(when, theme.style(Role::Muted)),
        ])
    }));
    lines
}

/// One line of facts: a label and its value.
fn fact(model: &Model, label: &str, value: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{label:<13}"),
        model.theme.style(Role::Muted),
    )];
    spans.extend(value);
    Line::from(spans)
}

/// The facts of an environment, most important first: the pane shows as
/// many as fit.
fn detail_lines(model: &Model, environment: &Environment) -> Vec<Line<'static>> {
    let theme = model.theme;
    let pool = model
        .snapshot
        .pools
        .iter()
        .find(|pool| pool.name == environment.name.pool());
    let mut lines = Vec::new();
    if let Some(pool) = pool {
        let role = if pool.health == crate::menu::world::Health::Online {
            Role::Good
        } else {
            Role::Warning
        };
        let mode = if pool.writable {
            "read/write"
        } else {
            "read-only"
        };
        lines.push(fact(
            model,
            "pool",
            vec![
                Span::raw(format!("{} ", pool.name)),
                Span::styled(pool.health.as_str(), theme.style(role)),
                Span::raw(format!(", {mode}")),
            ],
        ));
    }
    let kernel = environment
        .boot_kernel()
        .map_or_else(|| "none".to_owned(), |k| k.version.clone());
    let count = environment.kernels.len();
    lines.push(fact(
        model,
        "kernel",
        vec![Span::raw(format!("{kernel} ({count} kernels)"))],
    ));
    let snapshots = match environment.snapshots.iter().max_by_key(|s| s.created) {
        Some(newest) => format!(
            "{}, newest {} {}",
            environment.snapshots.len(),
            newest.name.name(),
            age(model.snapshot.now, newest.created)
        ),
        None => "none".to_owned(),
    };
    lines.push(fact(model, "snapshots", vec![Span::raw(snapshots)]));
    lines.push(fact(model, "encryption", encryption(model, environment)));
    lines.push(fact(
        model,
        "command line",
        command_line(model, environment),
    ));
    lines
}

fn encryption(model: &Model, environment: &Environment) -> Vec<Span<'static>> {
    let theme = model.theme;
    match environment.encryption {
        Encryption::None => vec![Span::raw("none")],
        Encryption::Unlocked => vec![Span::raw("key loaded")],
        Encryption::Locked => vec![
            Span::styled("locked", theme.style(Role::Locked)),
            Span::raw(": the key is asked at boot"),
        ],
    }
}

fn command_line(model: &Model, environment: &Environment) -> Vec<Span<'static>> {
    let line = model.command_line(environment);
    let origin = if line == environment.command_line.value {
        match &environment.command_line.origin {
            Origin::Local => "set here".to_owned(),
            Origin::Inherited(from) => format!("from {from}"),
            Origin::Default => "default".to_owned(),
        }
    } else {
        "edited for this boot".to_owned()
    };
    let shown = if line.is_empty() {
        "(empty)".to_owned()
    } else {
        line
    };
    vec![
        Span::raw(shown),
        Span::styled(format!("  {origin}"), model.theme.style(Role::Muted)),
    ]
}
