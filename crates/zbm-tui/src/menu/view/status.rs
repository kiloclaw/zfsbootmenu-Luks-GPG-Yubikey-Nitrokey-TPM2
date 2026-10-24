//! The navigation bar (`docs/dev/menu-design/apple-hig.md`, rule 1): the
//! back label on the left, the title in the middle, the status of the
//! machine on the right; shorter forms on narrow screens.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::menu::format::fit;
use crate::menu::keymap::Context;
use crate::menu::model::{Model, Screen};
use crate::menu::theme::{Glyph, Role};
use crate::menu::world::{Health, Level, VolumeState, worst_pool};
use crate::text::text;

/// Below this width the status takes its short forms.
const NARROW: u16 = 100;

/// Text with its role.
type Part = (String, Role);

/// Draws the navigation bar.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let width = usize::from(area.width);
    let status = facts(model, area.width < NARROW);
    let right = joined(model, &status);
    let back = back(model).map(|label| format!(" {} {label}", model.theme.glyph(Glyph::Back)));
    let short_back = back
        .as_ref()
        .map(|_| format!(" {}", model.theme.glyph(Glyph::Back)));
    let mut left = back.unwrap_or_default();
    let free = |left: &str| width.saturating_sub(left.chars().count() + right.chars().count() + 4);
    if titles(model)
        .last()
        .is_some_and(|title| title.chars().count() > free(&left))
    {
        left = short_back.unwrap_or_default();
    }
    let room = free(&left);
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
    let line = bar(model, (&left, &title, &status), width);
    frame.render_widget(Paragraph::new(line), area);
}

/// The bar: `left` at the start, `title` centred if it can be, else just
/// after `left`, and `status` at the end.
fn bar(model: &Model, (left, title, status): (&str, &str, &[Part]), width: usize) -> Line<'static> {
    let theme = model.theme;
    let right = joined(model, status);
    let (left_len, title_len, right_len) = (
        left.chars().count(),
        title.chars().count(),
        right.chars().count() + 1,
    );
    let centred = width.saturating_sub(title_len) / 2;
    let latest = width.saturating_sub(right_len + title_len + 2);
    let start = centred.min(latest).max(left_len + 2);
    let mut spans = vec![
        Span::styled(left.to_owned(), theme.style(Role::Accent)),
        Span::raw(" ".repeat(start.saturating_sub(left_len))),
        Span::styled(title.to_owned(), theme.style(Role::Title)),
    ];
    let gap = width.saturating_sub(start + title_len + right_len);
    spans.push(Span::raw(" ".repeat(gap.max(1))));
    for (index, (text, role)) in status.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(theme.dot(), theme.style(Role::Muted)));
        }
        spans.push(Span::styled(text.clone(), theme.style(*role)));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// The status as one string, for measuring.
fn joined(model: &Model, status: &[Part]) -> String {
    let texts: Vec<&str> = status.iter().map(|(text, _)| text.as_str()).collect();
    texts.join(model.theme.dot())
}

/// The label of the view `Esc` returns to, if any.
fn back(model: &Model) -> Option<String> {
    match model.screen {
        Screen::Startup | Screen::Main => None,
        Screen::Help(context) => Some(context_name(context)),
        Screen::Diff => Some(text!("view-snapshots")),
        _ => Some(text!("view-main")),
    }
}

/// The name of the view of `context`, for the back label of help.
fn context_name(context: Context) -> String {
    match context {
        Context::Startup => text!("view-startup.short"),
        Context::Snapshots => text!("view-snapshots"),
        Context::Kernels => text!("view-kernels"),
        Context::Pools => text!("view-pools.short"),
        Context::Recovery => text!("view-recovery"),
        Context::Log => text!("view-log"),
        Context::Diff | Context::Packages => text!("view-changes"),
        _ => text!("view-main"),
    }
}

/// The title of the view, longest first.
fn titles(model: &Model) -> Vec<String> {
    let focus = model
        .focus
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    match model.screen {
        Screen::Startup => vec![text!("view-startup"), text!("view-startup.short")],
        Screen::Main => vec![text!("view-main"), text!("view-main.short")],
        Screen::Snapshots => vec![
            text!("view-snapshots.of", environment = focus.as_str()),
            text!("view-snapshots"),
        ],
        Screen::Kernels => vec![
            text!("view-kernels.of", environment = focus.as_str()),
            text!("view-kernels"),
        ],
        Screen::CommandLine => vec![
            text!("view-command-line.of", environment = focus.as_str()),
            text!("view-command-line"),
        ],
        Screen::Pools => vec![text!("view-pools"), text!("view-pools.short")],
        Screen::Recovery => vec![text!("view-recovery")],
        Screen::Log => vec![text!("view-log")],
        Screen::Help(_) => vec![text!("view-help")],
        Screen::Diff => {
            let since = model.diff.as_ref().map(|diff| diff.from.to_string());
            let mut titles = vec![text!("view-changes")];
            if let Some(since) = since {
                titles.insert(0, text!("view-changes.since", snapshot = since));
            }
            titles
        }
    }
}

/// The status: the worst pool, the volumes, the log; each with the role of
/// what it says.
fn facts(model: &Model, short: bool) -> Vec<Part> {
    let snapshot = &model.snapshot;
    let mut parts = Vec::new();
    match worst_pool(snapshot) {
        Some(pool) => {
            let role = if pool.health == Health::Online {
                Role::Muted
            } else {
                Role::Warning
            };
            parts.push((format!("{} {}", pool.name, pool.health.as_str()), role));
        }
        None if short => parts.push((text!("status-pools-ok.short"), Role::Muted)),
        None => parts.push((
            text!("status-pools-ok", count = snapshot.pools.len()),
            Role::Muted,
        )),
    }
    let volumes = &snapshot.volumes;
    if !volumes.is_empty() {
        let open = volumes
            .iter()
            .filter(|v| v.state == VolumeState::Open)
            .count();
        let role = if open == volumes.len() {
            Role::Muted
        } else {
            Role::Locked
        };
        let text = if short {
            text!(
                "status-volumes-open.short",
                open = open,
                total = volumes.len()
            )
        } else {
            text!("status-volumes-open", open = open, total = volumes.len())
        };
        parts.push((text, role));
    }
    let problems = snapshot.log.count(Level::Warning) + snapshot.log.count(Level::Error);
    if problems > 0 {
        let glyph = model.theme.glyph(Glyph::Warning);
        let text = if short {
            format!("{glyph} {problems}")
        } else {
            format!("{glyph} {}", text!("status-log-problems", count = problems))
        };
        parts.push((text, Role::Warning));
    }
    parts
}
