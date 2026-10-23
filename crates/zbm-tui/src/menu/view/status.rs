//! The status bar: the countdown or the view's name, then the pools, the
//! volumes and the log, in short forms on narrow screens.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::menu::format::fit;
use crate::menu::model::{Model, Screen};
use crate::menu::theme::Role;
use crate::menu::world::{Level, VolumeState, worst_pool};

/// Below this width the status bar uses short forms.
const NARROW: u16 = 100;

/// Draws the status bar.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let width = usize::from(area.width);
    let facts = facts(model, area.width < NARROW);
    let room = width.saturating_sub(facts.chars().count() + 3);
    let title = titles(model)
        .into_iter()
        .find(|title| title.chars().count() <= room)
        .unwrap_or_else(|| {
            fit(
                &titles(model).pop().unwrap_or_default(),
                room,
                model.theme.is_ascii(),
            )
        });
    let gap = width.saturating_sub(title.chars().count() + facts.chars().count() + 2);
    let line = Line::from(vec![
        Span::raw(" "),
        Span::raw(title),
        Span::raw(" ".repeat(gap)),
        Span::raw(facts),
        Span::raw(" "),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(model.theme.style(Role::StatusBar)),
        area,
    );
}

/// The left part, longest first: the countdown while it runs, else the
/// view's name.
fn titles(model: &Model) -> Vec<String> {
    let dot = model.theme.dot();
    if let (Some(seconds), Some(environment)) =
        (model.countdown_seconds(), model.selected_environment())
    {
        let name = &environment.name;
        return vec![
            format!("Booting {name} in {seconds} s{dot}any key stops"),
            format!("Booting {name} in {seconds} s"),
            format!("Boot in {seconds} s"),
        ];
    }
    let name = match model.screen {
        Screen::Startup => "Starting".to_owned(),
        Screen::Main => "Boot environments".to_owned(),
        Screen::Snapshots => format!("Snapshots of {}", focus(model)),
        Screen::Kernels => format!("Kernels of {}", focus(model)),
        Screen::CommandLine => format!("Command line of {}", focus(model)),
        Screen::Pools => "Pools and volumes".to_owned(),
        Screen::Recovery => "Recovery".to_owned(),
        Screen::Log => "Log".to_owned(),
        Screen::Help(_) => "Help".to_owned(),
    };
    vec![format!("ZFSBootMenu{dot}{name}"), name]
}

fn focus(model: &Model) -> String {
    model
        .focus
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default()
}

/// The right part: pools, volumes, log.
fn facts(model: &Model, short: bool) -> String {
    let snapshot = &model.snapshot;
    let pools = snapshot.pools.len();
    let mut parts = Vec::new();
    match worst_pool(snapshot) {
        Some(pool) if short => parts.push(format!("{} {}", pool.name, pool.health.as_str())),
        Some(pool) => parts.push(format!(
            "pools {pools}{}{} {}",
            model.theme.dot(),
            pool.name,
            pool.health.as_str()
        )),
        None if short => parts.push("pools ok".to_owned()),
        None => parts.push(format!("pools {pools} ok")),
    }
    let volumes = &snapshot.volumes;
    if !volumes.is_empty() {
        let open = volumes
            .iter()
            .filter(|v| v.state == VolumeState::Open)
            .count();
        if short {
            parts.push(format!("{open}/{} open", volumes.len()));
        } else {
            parts.push(format!("volumes {open}/{} open", volumes.len()));
        }
    }
    let warnings = snapshot.log.count(Level::Warning);
    let errors = snapshot.log.count(Level::Error);
    if short && errors + warnings > 0 {
        parts.push(format!("[!] {}", errors + warnings));
    } else if errors > 0 {
        parts.push(format!(
            "[!] {errors} error{}",
            if errors == 1 { "" } else { "s" }
        ));
    } else if warnings > 0 {
        parts.push(format!(
            "[!] {warnings} warning{}",
            if warnings == 1 { "" } else { "s" }
        ));
    }
    parts.join(model.theme.dot())
}
