//! Drawing the menu: the frame (status bar, notification, footer) around
//! the view of the current screen, and dialogs over it
//! (`docs/dev/menu-design/information-architecture.md`).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::format::title_case;
use super::keymap;
use super::model::{Model, Screen};
use super::theme::{Glyph, Role};
use super::world::Outcome;

mod dialogs;
mod diff;
mod health;
mod lists;
mod main_view;
mod packages;
mod screens;
mod status;
mod unlock;

/// The smallest size the menu lays out for.
pub const MINIMUM: (u16, u16) = (52, 20);

/// Draws `model` into `frame`.
pub fn view(model: &Model, frame: &mut Frame<'_>) {
    let area = frame.area();
    if let Some(why) = &model.leaving {
        frame.render_widget(Paragraph::new(why.as_str()), area);
        return;
    }
    if area.width < MINIMUM.0 || area.height < MINIMUM.1 {
        let notice = format!("The menu needs {}x{} at least.", MINIMUM.0, MINIMUM.1);
        frame.render_widget(Paragraph::new(notice), area);
        return;
    }
    // Text without a role is primary text: white on the console.
    frame
        .buffer_mut()
        .set_style(area, model.theme.style(Role::Text));
    let notification = u16::from(model.notification.is_some());
    let [top, body, note, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(notification),
        Constraint::Length(1),
    ])
    .areas(area);
    status::render(model, frame, top);
    render_screen(model, frame, body);
    if let Some(outcome) = &model.notification {
        frame.render_widget(Paragraph::new(notification_line(model, outcome)), note);
    }
    frame.render_widget(Paragraph::new(footer(model, bottom.width)), bottom);
    dialogs::render(model, frame, body);
}

/// The view of the current screen.
fn render_screen(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    match model.screen {
        Screen::Startup => screens::startup(model, frame, area),
        Screen::Main => main_view::render(model, frame, area),
        Screen::Snapshots => screens::snapshots(model, frame, area),
        Screen::Kernels => screens::kernels(model, frame, area),
        Screen::CommandLine => screens::command_line(model, frame, area),
        Screen::Pools => screens::pools(model, frame, area),
        Screen::Recovery => screens::recovery(model, frame, area),
        Screen::Log => screens::log(model, frame, area),
        Screen::Help(context) => screens::help(model, frame, area, context),
        Screen::Diff => diff::render(model, frame, area),
    }
}

/// The notification line (rule 10): the outcome of the last action, with a
/// glyph and a word.
fn notification_line<'a>(model: &Model, outcome: &'a Outcome) -> Line<'a> {
    let theme = model.theme;
    let (glyph, word, role, text) = match outcome {
        Outcome::Done(text) => (Glyph::Done, "Done", Role::Good, text.as_str()),
        Outcome::Warning(text) => (Glyph::Warning, "Warning", Role::Warning, text.as_str()),
        Outcome::Failed { what, .. } => (Glyph::Failed, "Failed", Role::Danger, what.as_str()),
    };
    Line::from(vec![
        Span::styled(
            format!(" {} {word}: ", theme.glyph(glyph)),
            theme.style(role),
        ),
        Span::styled(text, theme.style(Role::Text)),
    ])
}

/// The toolbar (rule 9): the actions of the focused element, each its key
/// and a title-case label, and `? More` on the right.
fn footer(model: &Model, width: u16) -> Line<'static> {
    let theme = model.theme;
    let width = usize::from(width).saturating_sub(2);
    let gap = " ".repeat(keymap::TOOLBAR_GAP);
    let mut spans = vec![Span::raw(" ")];
    let mut used = 0;
    for (index, (key, help)) in keymap::footer(model.context(), width)
        .into_iter()
        .enumerate()
    {
        let label = title_case(help);
        let length = key.len() + 1 + label.len();
        if key == "?" {
            spans.push(Span::raw(
                " ".repeat(width.saturating_sub(used + length).max(1)),
            ));
        } else if index > 0 {
            spans.push(Span::raw(gap.clone()));
            used += gap.len();
        }
        used += length;
        spans.push(Span::styled(key, theme.style(Role::Key)));
        spans.push(Span::styled(format!(" {label}"), theme.style(Role::Muted)));
    }
    Line::from(spans)
}

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod wireframes;
