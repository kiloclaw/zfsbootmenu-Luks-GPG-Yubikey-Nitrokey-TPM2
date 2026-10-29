//! The Boot tab of the workspace (ADR 0007,
//! `docs/dev/menu-design/information-architecture.md`): from 110 columns a
//! sidebar of the pools, the volumes and what needs attention, beside the
//! boot environments and the inspector; narrower, as over a serial line at
//! 80×24, what needs attention as one line above the list, and the
//! inspector below it; at 52×20, the list and the inspector alone.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::{lists, main_view};
use crate::menu::format::fit;
use crate::menu::keymap::{self, Context, Intent, Map, Tab};
use crate::menu::model::Model;
use crate::menu::theme::{Glyph, Role};
use crate::menu::world::{Environment, Health, Level, Verdict, VolumeState};
use crate::text::{self, text};

/// From this width the sidebar shows.
const SIDEBAR_FROM: u16 = 110;

/// The width of the sidebar.
const SIDEBAR: u16 = 26;

/// From this width what needs attention shows as a line above the list.
const ATTENTION_LINE_FROM: u16 = 80;

/// How many next steps the inspector lists.
const NEXT_STEPS: usize = 4;

/// Something that needs attention, and the tab that leads to it.
struct Item {
    glyph: Glyph,
    role: Role,
    what: String,
    tab: Tab,
}

/// Draws the Boot tab.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let area = main_view::render_banner(model, frame, area);
    if area.width >= SIDEBAR_FROM {
        let [side, list, detail] = Layout::horizontal([
            Constraint::Length(SIDEBAR),
            Constraint::Fill(5),
            Constraint::Fill(4),
        ])
        .areas(area);
        render_sidebar(model, frame, side);
        main_view::render_list(model, frame, list);
        main_view::render_detail(model, frame, detail);
        return;
    }
    let items = attention(model);
    let line = u16::from(!items.is_empty() && area.width >= ATTENTION_LINE_FROM);
    let [note, rest] = Layout::vertical([Constraint::Length(line), Constraint::Min(1)]).areas(area);
    if line > 0 {
        frame.render_widget(Paragraph::new(attention_line(model, &items, note)), note);
    }
    let lines = if rest.height >= 17 { 9 } else { 5 };
    let [list, detail] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(lines)]).areas(rest);
    main_view::render_list(model, frame, list);
    main_view::render_detail(model, frame, detail);
}

/// What needs attention: pools not imported or not healthy, volumes not
/// open, environments whose checks found something, and the log's
/// problems.
fn attention(model: &Model) -> Vec<Item> {
    let snapshot = &model.snapshot;
    let mut items = pool_items(model);
    items.extend(volume_items(model));
    items.extend(snapshot.environments.iter().filter_map(environment_item));
    let problems = snapshot.log.count(Level::Warning) + snapshot.log.count(Level::Error);
    if problems > 0 {
        items.push(Item {
            glyph: Glyph::Warning,
            role: Role::Warning,
            what: text!("status-log-problems", count = problems),
            tab: Tab::Log,
        });
    }
    items
}

/// The pools not imported, or not healthy.
fn pool_items(model: &Model) -> Vec<Item> {
    model
        .snapshot
        .pools
        .iter()
        .filter_map(|pool| {
            let name = pool.name.as_str();
            let what = match (&pool.foreign_hostid, pool.imported, pool.health) {
                (Some(hostid), false, _) => {
                    text!("attention-foreign", pool = name, hostid = hostid.as_str())
                }
                (None, false, _) => text!("attention-not-imported", pool = name),
                (_, true, Health::Online) => return None,
                (_, true, health) => format!("{name} {}", health.as_str()),
            };
            Some(Item {
                glyph: Glyph::Warning,
                role: Role::Warning,
                what,
                tab: Tab::Pools,
            })
        })
        .collect()
}

/// The volumes not open.
fn volume_items(model: &Model) -> Vec<Item> {
    model
        .snapshot
        .volumes
        .iter()
        .filter_map(|volume| {
            let (glyph, role, state) = match &volume.state {
                VolumeState::Open => return None,
                VolumeState::Failed(_) => (Glyph::Failed, Role::Danger, text!("attention-failed")),
                VolumeState::Locked => (Glyph::Warning, Role::Locked, text!("volume-locked")),
                VolumeState::Skipped => (Glyph::Warning, Role::Warning, text!("volume-skipped")),
            };
            let what = format!("{}: {state}", volume.name.printable());
            Some(Item {
                glyph,
                role,
                what,
                tab: Tab::Pools,
            })
        })
        .collect()
}

/// An environment whose checks found problems or warnings, by its last
/// name: `debian: 1 problem`.
fn environment_item(environment: &Environment) -> Option<Item> {
    let count = |verdict| {
        environment
            .checks
            .iter()
            .filter(|check| check.verdict == verdict)
            .count()
    };
    let (failures, warnings) = (count(Verdict::Failure), count(Verdict::Warning));
    let (glyph, role, badge) = if failures > 0 {
        let badge = text!("badge-problems", count = failures);
        (Glyph::Failed, Role::Danger, badge)
    } else if warnings > 0 {
        let badge = text!("badge-warnings", count = warnings);
        (Glyph::Warning, Role::Warning, badge)
    } else {
        return None;
    };
    let name = environment.name.to_string();
    let short = name.rsplit('/').next().unwrap_or(&name);
    Some(Item {
        glyph,
        role,
        what: format!("{short}: {badge}"),
        tab: Tab::Boot,
    })
}

/// How many things need attention, for the tab bar of narrow screens.
pub(super) fn attention_count(model: &Model) -> usize {
    attention(model).len()
}

/// The key that opens `tab`, with its name: `3 Pools`.
fn tab_key(tab: Tab) -> String {
    let what = keymap::describe(Context::Main, Intent::Tab(tab));
    format!("{} {}", tab.digit(), text::title(what))
}

/// What needs attention as one line, as wide as `area`: each item with its
/// glyph, and the tab of the first on the right.
fn attention_line(model: &Model, items: &[Item], area: Rect) -> Line<'static> {
    let theme = model.theme;
    let width = usize::from(area.width).saturating_sub(2);
    let key = items
        .first()
        .map(|item| tab_key(item.tab))
        .unwrap_or_default();
    let mut room = width.saturating_sub(key.chars().count() + 3);
    let mut spans = vec![Span::raw(" ")];
    for item in items {
        let text = format!("{} {}", theme.glyph(item.glyph), item.what);
        let length = text.chars().count() + 3;
        if length > room {
            break;
        }
        room -= length;
        spans.push(Span::styled(text, theme.style(item.role)));
        spans.push(Span::raw("   "));
    }
    spans.push(Span::raw(" ".repeat(room)));
    spans.push(Span::styled(key, theme.style(Role::Muted)));
    Line::from(spans)
}

/// The sidebar: the pools, the volumes, and what needs attention.
fn render_sidebar(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let theme = model.theme;
    let width = lists::row_width(area) + 2;
    let pools = pool_lines(model, width);
    let volumes = volume_lines(model, width);
    let open = model
        .snapshot
        .volumes
        .iter()
        .filter(|volume| volume.state == VolumeState::Open)
        .count();
    let height = |lines: &[Line<'_>]| u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let [top, middle, bottom] = Layout::vertical([
        Constraint::Length(height(&pools)),
        Constraint::Length(height(&volumes)),
        Constraint::Min(3),
    ])
    .areas(area);
    let count = model.snapshot.pools.len().to_string();
    let block = lists::counted(theme, &text!("sidebar-pools"), &count);
    frame.render_widget(Paragraph::new(pools).block(block), top);
    let count = text!(
        "sidebar-open",
        open = open,
        total = model.snapshot.volumes.len()
    );
    let block = lists::counted(theme, &text!("sidebar-volumes"), &count);
    frame.render_widget(Paragraph::new(volumes).block(block), middle);
    let items = attention(model);
    let count = items.len().to_string();
    let block = lists::counted(theme, &text!("sidebar-attention"), &count);
    let lines = attention_lines(model, &items, width);
    frame.render_widget(Paragraph::new(lines).block(block), bottom);
}

/// A pool per line: its name, then its health and mode, or why it is not
/// imported.
fn pool_lines(model: &Model, width: usize) -> Vec<Line<'static>> {
    let theme = model.theme;
    model
        .snapshot
        .pools
        .iter()
        .map(|pool| {
            let facts = if pool.imported {
                let role = if pool.health == Health::Online {
                    Role::Good
                } else {
                    Role::Warning
                };
                let mode = if pool.writable {
                    text!("sidebar-rw")
                } else {
                    text!("sidebar-ro")
                };
                vec![
                    Span::styled(pool.health.as_str(), theme.style(role)),
                    Span::styled(format!(" {mode}"), theme.style(Role::Muted)),
                ]
            } else {
                vec![Span::styled(
                    text!("sidebar-not-imported"),
                    theme.style(Role::Warning),
                )]
            };
            let room = width.saturating_sub(facts.iter().map(Span::width).sum::<usize>() + 1);
            let name = fit(pool.name.as_str(), room, theme.is_ascii());
            lists::row(theme, &name, &[], width, facts)
        })
        .collect()
}

/// A volume per line: its name, then whether it is open.
fn volume_lines(model: &Model, width: usize) -> Vec<Line<'static>> {
    let theme = model.theme;
    model
        .snapshot
        .volumes
        .iter()
        .map(|volume| {
            let (state, role) = match volume.state {
                VolumeState::Open => (text!("volume-open"), Role::Good),
                VolumeState::Locked => (text!("volume-locked"), Role::Locked),
                VolumeState::Skipped => (text!("volume-skipped"), Role::Warning),
                VolumeState::Failed(_) => (text!("attention-failed"), Role::Danger),
            };
            let facts = vec![Span::styled(state, theme.style(role))];
            let room = width.saturating_sub(facts.iter().map(Span::width).sum::<usize>() + 1);
            let name = volume.name.printable().to_string();
            let name = fit(&name, room, theme.is_ascii());
            lists::row(theme, &name, &[], width, facts)
        })
        .collect()
}

/// What needs attention in the sidebar: each item, then the tab that leads
/// to it, indented.
fn attention_lines(model: &Model, items: &[Item], width: usize) -> Vec<Line<'static>> {
    let theme = model.theme;
    if items.is_empty() {
        return vec![Line::styled(
            text!("attention-none"),
            theme.style(Role::Muted),
        )];
    }
    items
        .iter()
        .flat_map(|item| {
            let what = format!("{} {}", theme.glyph(item.glyph), item.what);
            [
                Line::styled(fit(&what, width, theme.is_ascii()), theme.style(item.role)),
                Line::styled(format!("  {}", tab_key(item.tab)), theme.style(Role::Muted)),
            ]
        })
        .collect()
}

/// The inspector's next steps in the workspace: the keys of the Boot tab
/// after `Enter`, two to a line; none with the current map.
pub(super) fn next_steps(model: &Model, width: usize) -> Vec<Line<'static>> {
    if model.map != Map::Workspace {
        return Vec::new();
    }
    let theme = model.theme;
    let steps: Vec<(String, String)> = keymap::bindings(model.map, Context::Main)
        .into_iter()
        .filter(|(intent, ..)| !matches!(intent, Intent::Primary | Intent::Help))
        .take(NEXT_STEPS)
        .map(|(_, key, what)| (key, text::get(what)))
        .collect();
    let column = width.saturating_sub(2) / 2;
    let mut lines = vec![Line::styled(
        text!("details-next-steps"),
        theme.style(Role::Header),
    )];
    for pair in steps.chunks(2) {
        let mut spans = vec![Span::raw("  ")];
        for (key, what) in pair {
            let what = fit(
                what,
                column.saturating_sub(key.chars().count() + 2),
                theme.is_ascii(),
            );
            let used = key.chars().count() + 1 + what.chars().count();
            spans.push(Span::styled(key.clone(), theme.style(Role::Key)));
            spans.push(Span::styled(format!(" {what}"), theme.style(Role::Text)));
            spans.push(Span::raw(" ".repeat(column.saturating_sub(used))));
        }
        lines.push(Line::from(spans));
    }
    lines
}
