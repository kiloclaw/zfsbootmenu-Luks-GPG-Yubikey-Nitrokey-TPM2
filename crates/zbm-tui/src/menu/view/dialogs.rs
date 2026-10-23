//! Dialogs and the command palette, drawn over the view.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};

use super::lists;
use crate::menu::model::{Choice, Dialog, Model};
use crate::menu::theme::{Role, Theme};
use crate::menu::update::palette_items;
use crate::menu::world::Method;

/// Draws the dialog or the palette, if one is open.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    if let Some(dialog) = &model.dialog {
        match dialog {
            Dialog::Unlock {
                volume,
                method,
                tries_left,
                ..
            } => {
                unlock(model, frame, area, (volume, *method, *tries_left));
            }
            Dialog::Choose {
                title,
                lines,
                choices,
                selected,
            } => {
                choose(model.theme, frame, area, (title, lines, choices, *selected));
            }
            Dialog::Confirm {
                title,
                lines,
                name,
                typed,
                ..
            } => {
                confirm(model.theme, frame, area, (title, lines, name, typed));
            }
            Dialog::Error { what, why, next } => error(model.theme, frame, area, (what, why, next)),
        }
    } else if model.palette.is_some() {
        palette(model, frame, area);
    }
}

/// A box of `width` by `height` in the middle of `area`, cleared.
fn centred(frame: &mut Frame<'_>, area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [rect] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    frame.render_widget(Clear, rect);
    rect
}

/// The width of dialogs: 70 columns, or the screen less a margin.
fn dialog_width(area: Rect) -> u16 {
    area.width.saturating_sub(4).min(70)
}

/// The height of a bordered box of `width` holding `lines` wrapped.
fn wrapped_height(lines: &[Line<'_>], width: u16) -> u16 {
    let inner = usize::from(width.saturating_sub(2).max(1));
    let rows: usize = lines
        .iter()
        .map(|line| line.width().div_ceil(inner).max(1))
        .sum();
    u16::try_from(rows).unwrap_or(u16::MAX).saturating_add(2)
}

/// Text lines in the muted style.
fn muted(theme: Theme, lines: &[String]) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|line| Line::styled(line.clone(), theme.style(Role::Text)))
        .collect()
}

/// The unlock dialog: the volume, the method, the hidden field, the other
/// methods. The field shows nothing, not even the length of the secret.
fn unlock(
    model: &Model,
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: (&String, Method, Option<u32>),
) {
    let (_, method, _) = dialog;
    let lines = unlock_lines(model, dialog);
    let width = dialog_width(area);
    let rect = centred(frame, area, width, wrapped_height(&lines, width));
    let block = lists::block(model.theme, "Unlock");
    let inner = block.inner(rect);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(block),
        rect,
    );
    if method.is_typed() {
        let column = u16::try_from(method.as_str().len() + 2).unwrap_or(u16::MAX);
        frame.set_cursor_position(Position::new(inner.x.saturating_add(column), inner.y + 2));
    }
}

/// The text of the unlock dialog.
fn unlock_lines(model: &Model, dialog: (&String, Method, Option<u32>)) -> Vec<Line<'static>> {
    let (volume, method, tries_left) = dialog;
    let theme = model.theme;
    let known = model
        .snapshot
        .volumes
        .iter()
        .find(|known| &known.name == volume);
    let device = known.map(|known| known.device.clone()).unwrap_or_default();
    let others: Vec<&str> = known
        .map(|known| {
            let others = known.methods.iter().filter(|m| **m != method);
            others.map(|m| m.as_str()).collect()
        })
        .unwrap_or_default();
    let ask = if method.is_typed() {
        format!("{}:", capitalised(method.as_str()))
    } else {
        format!(
            "Waiting for the {}: touch it, or plug it in and press Enter",
            method.as_str()
        )
    };
    let mut lines = vec![
        Line::from(format!("{volume} on {device}")),
        Line::default(),
        Line::styled(ask, theme.style(Role::Title)),
    ];
    if let Some(left) = tries_left {
        let warning = format!("{left} tries left");
        lines.push(Line::styled(warning, theme.style(Role::Warning)));
    }
    lines.push(Line::default());
    if !others.is_empty() {
        let switch = format!("Up/Down: {}", others.join(", "));
        lines.push(Line::styled(switch, theme.style(Role::Muted)));
    }
    lines.push(Line::styled(
        "Esc: leave it locked",
        theme.style(Role::Muted),
    ));
    lines
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A choice among actions.
fn choose(
    theme: Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: (&String, &Vec<String>, &Vec<Choice>, usize),
) {
    let (title, lines, choices, selected) = dialog;
    let mut text = muted(theme, lines);
    if !text.is_empty() {
        text.push(Line::default());
    }
    let width = dialog_width(area);
    let extra = wrapped_height(&text, width).saturating_sub(2) * u16::from(!text.is_empty());
    let height = u16::try_from(choices.len()).unwrap_or(u16::MAX) + extra + 2;
    let rect = centred(frame, area, width, height);
    let block = lists::block(theme, title);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [top, list] =
        Layout::vertical([Constraint::Length(extra), Constraint::Min(1)]).areas(inner);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), top);
    let items: Vec<ListItem<'static>> = choices
        .iter()
        .map(|c| ListItem::new(c.label.clone()))
        .collect();
    let widget = List::new(items)
        .highlight_symbol("> ")
        .highlight_style(theme.style(Role::Selected));
    frame.render_stateful_widget(
        widget,
        list,
        &mut ListState::default().with_selected(Some(selected)),
    );
}

/// A typed confirmation.
fn confirm(
    theme: Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: (&String, &Vec<String>, &String, &String),
) {
    let (title, lines, name, typed) = dialog;
    let mut text = muted(theme, lines);
    text.push(Line::default());
    text.push(Line::from(vec![
        Span::raw("Type "),
        Span::styled(name.clone(), theme.style(Role::Danger)),
        Span::raw(" to confirm: "),
        Span::raw(typed.clone()),
    ]));
    text.push(Line::styled("Esc: cancel", theme.style(Role::Muted)));
    let width = dialog_width(area);
    let rect = centred(frame, area, width, wrapped_height(&text, width));
    let block = lists::block(theme, title).border_style(theme.style(Role::Danger));
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(block),
        rect,
    );
}

/// What failed, why, and the next steps.
fn error(
    theme: Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: (&String, &Vec<String>, &Vec<String>),
) {
    let (what, why, next) = dialog;
    let mut text = vec![Line::styled(what.clone(), theme.style(Role::Danger))];
    text.extend(
        why.iter()
            .map(|reason| Line::from(format!("because {reason}"))),
    );
    if !next.is_empty() {
        text.push(Line::default());
        text.push(Line::styled("You can:", theme.style(Role::Title)));
        text.extend(next.iter().map(|step| Line::from(format!("  - {step}"))));
    }
    text.push(Line::default());
    text.push(Line::styled(
        "Enter or Esc: close",
        theme.style(Role::Muted),
    ));
    let width = dialog_width(area);
    let rect = centred(frame, area, width, wrapped_height(&text, width));
    let block = lists::block(theme, "Failed").border_style(theme.style(Role::Danger));
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).block(block),
        rect,
    );
}

/// The command palette: the query, then the best matches.
fn palette(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let Some(palette) = &model.palette else {
        return;
    };
    let theme = model.theme;
    let items = palette_items(model);
    let height = area.height.saturating_sub(2).min(16);
    let rect = centred(frame, area, dialog_width(area), height);
    let block = lists::block(
        theme,
        &format!("Commands {}/{}", palette.matches.len(), items.len()),
    );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [query, list] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    frame.render_widget(Paragraph::new(format!(": {}", palette.query)), query);
    let column = u16::try_from(palette.query.chars().count() + 2).unwrap_or(u16::MAX);
    frame.set_cursor_position(Position::new(query.x.saturating_add(column), query.y));
    let rows: Vec<ListItem<'static>> = palette
        .matches
        .iter()
        .take(usize::from(list.height).max(palette.selected + 1))
        .filter_map(|ranked| {
            let item = items.get(ranked.index)?;
            let label = crate::menu::format::fit(
                &item.label,
                usize::from(list.width).saturating_sub(2),
                theme.is_ascii(),
            );
            Some(ListItem::new(Line::from(lists::highlighted(
                theme,
                &label,
                &ranked.positions,
            ))))
        })
        .collect();
    let widget = List::new(rows)
        .highlight_symbol("> ")
        .highlight_style(theme.style(Role::Selected));
    frame.render_stateful_widget(
        widget,
        list,
        &mut ListState::default().with_selected(Some(palette.selected)),
    );
}
