//! Alerts, action sheets and the command palette, over the dimmed view
//! (`docs/dev/menu-design/apple-hig.md`, rules 6 to 8).

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, HighlightSpacing, List, ListItem, ListState, Padding, Paragraph, Wrap,
};

use super::lists;
use crate::menu::format::{capitalised, fit};
use crate::menu::model::{Choice, Dialog, Model, Then};
use crate::menu::theme::{Glyph, Role, Theme};
use crate::menu::update::palette_items;
use crate::menu::world::Action;

/// The widest an alert or action sheet gets.
const ALERT_WIDTH: u16 = 60;

/// How a button looks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    /// An ordinary button.
    Normal,
    /// The button `Enter` presses.
    Default,
    /// A destructive button `Enter` presses.
    Destructive,
    /// A button that cannot be pressed yet.
    Disabled,
}

/// An alert: a title, a message, buttons.
pub(super) struct Alert {
    pub(super) title: String,
    pub(super) body: Vec<Line<'static>>,
    pub(super) buttons: Vec<(String, Kind)>,
    pub(super) danger: bool,
}

/// Draws the dialog or the palette, if one is open, over the dimmed view.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    if model.dialog.is_none() && model.palette.is_none() {
        return;
    }
    frame
        .buffer_mut()
        .set_style(area, Style::new().add_modifier(Modifier::DIM));
    let theme = model.theme;
    match &model.dialog {
        Some(Dialog::Unlock(prompt)) => super::unlock::render(model, frame, area, prompt),
        Some(Dialog::Choose {
            title,
            lines,
            choices,
            selected,
        }) => sheet(theme, frame, area, (title, lines, choices, *selected)),
        Some(Dialog::Confirm {
            title,
            lines,
            name,
            typed,
            action,
        }) => confirm(theme, frame, area, (title, lines, name, typed, action)),
        Some(Dialog::Error { what, why, next }) => error(theme, frame, area, (what, why, next)),
        None => palette(model, frame, area),
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

/// The width of alerts: 60 columns, or the screen less a margin.
fn alert_width(area: Rect) -> u16 {
    area.width.saturating_sub(4).min(ALERT_WIDTH)
}

/// The rows `lines` take wrapped in `width` columns.
fn wrapped(lines: &[Line<'_>], width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let rows: usize = lines
        .iter()
        .map(|line| line.width().div_ceil(width).max(1))
        .sum();
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// The frame of an alert or sheet: borders, red for danger, margins.
fn frame_block(theme: Theme, danger: bool) -> Block<'static> {
    let role = if danger { Role::Danger } else { Role::Border };
    Block::bordered()
        .border_set(theme.border())
        .border_style(theme.style(role))
        .padding(Padding::horizontal(1))
}

/// The buttons, right-aligned, drawn `[ Label ]`.
fn buttons(theme: Theme, buttons: &[(String, Kind)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (label, kind)) in buttons.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        let style = match kind {
            Kind::Normal => theme.style(Role::Text),
            Kind::Default => theme.style(Role::Selected),
            Kind::Destructive => theme.style(Role::Danger).add_modifier(Modifier::REVERSED),
            Kind::Disabled => theme.style(Role::Muted),
        };
        spans.push(Span::styled(format!("[ {label} ]"), style));
    }
    Line::from(spans).right_aligned()
}

/// Draws `alert`; returns the area of its message.
pub(super) fn alert(theme: Theme, frame: &mut Frame<'_>, area: Rect, alert: Alert) -> Rect {
    let width = alert_width(area);
    let inner_width = width.saturating_sub(4);
    let body = wrapped(&alert.body, inner_width);
    let height = body.saturating_add(6);
    let rect = centred(frame, area, width, height);
    let block = frame_block(theme, alert.danger);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [title, _, message, _, row] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(body),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let heading = fit(&alert.title, usize::from(inner.width), theme.is_ascii());
    frame.render_widget(
        Paragraph::new(Line::styled(heading, theme.style(Role::Title)).centered()),
        title,
    );
    frame.render_widget(
        Paragraph::new(alert.body).wrap(Wrap { trim: false }),
        message,
    );
    frame.render_widget(Paragraph::new(buttons(theme, &alert.buttons)), row);
    message
}

/// Lines in the body style.
fn body_lines(theme: Theme, lines: &[String]) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|line| Line::styled(line.clone(), theme.style(Role::Text)))
        .collect()
}

/// An action sheet: the item, a message, the actions, then **Cancel** set
/// apart. Destructive actions are red; those that ask first end in `…`.
fn sheet(
    theme: Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: (&String, &Vec<String>, &Vec<Choice>, usize),
) {
    let (title, lines, choices, selected) = dialog;
    let width = alert_width(area);
    let message = body_lines(theme, lines);
    let body = wrapped(&message, width.saturating_sub(4));
    let count = u16::try_from(choices.len()).unwrap_or(u16::MAX);
    let rect = centred(
        frame,
        area,
        width,
        count.saturating_add(body).saturating_add(6),
    );
    let block = frame_block(theme, false);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [head, text, _, list, rule, cancel] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(body),
        Constraint::Length(1),
        Constraint::Length(count),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let heading = fit(title, usize::from(inner.width), theme.is_ascii());
    let heading = Line::styled(heading, theme.style(Role::Title)).centered();
    frame.render_widget(Paragraph::new(heading), head);
    frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: true }), text);
    actions(theme, frame, (list, rule, cancel), (choices, selected));
}

/// The actions of a sheet in `list`, the separator in `rule`, and Cancel,
/// which is selected after the last action.
fn actions(
    theme: Theme,
    frame: &mut Frame<'_>,
    (list, rule, cancel): (Rect, Rect, Rect),
    (choices, selected): (&[Choice], usize),
) {
    let items: Vec<ListItem<'static>> = choices
        .iter()
        .map(|choice| ListItem::new(action_line(theme, choice)))
        .collect();
    let marker = format!("{} ", theme.glyph(Glyph::Selected));
    let widget = List::new(items)
        .highlight_symbol(Line::from(marker.clone()))
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(theme.style(Role::Selected));
    let on_list = (selected < choices.len()).then_some(selected);
    let mut state = ListState::default().with_selected(on_list);
    frame.render_stateful_widget(widget, list, &mut state);
    let line = if theme.is_ascii() { "-" } else { "─" };
    let separator = Paragraph::new(line.repeat(usize::from(rule.width)));
    frame.render_widget(separator.style(theme.style(Role::Border)), rule);
    let (prefix, style) = if on_list.is_none() {
        (marker, theme.style(Role::Selected))
    } else {
        ("  ".to_owned(), theme.style(Role::Text))
    };
    let row = Paragraph::new(format!("{prefix}Cancel")).style(style);
    frame.render_widget(row, cancel);
}

/// The line of an action: title case, `…` and red when it asks first.
fn action_line(theme: Theme, choice: &Choice) -> Line<'static> {
    let label = choice.label.clone();
    if matches!(choice.then, Then::Confirm { .. }) {
        let text = format!("{label}{}", theme.glyph(Glyph::Ellipsis));
        Line::styled(text, theme.style(Role::Danger))
    } else {
        Line::from(label)
    }
}

/// The verb of the button that does `action`.
fn verb(action: &Action) -> &'static str {
    match action {
        Action::Rollback { .. } => "Roll Back",
        Action::CloneAndPromote(_) => "Promote",
        Action::RewindCheckpoint(_) => "Rewind",
        Action::Import(..) => "Force Import",
        Action::RestoreFiles { .. } => "Restore",
        Action::Reseal(_) => "Re-seal",
        _ => "Continue",
    }
}

/// A destructive alert: what happens, the name to type, then **Cancel**
/// and the action, which stays disabled until the name is right.
fn confirm(
    theme: Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: (&String, &Vec<String>, &String, &String, &Action),
) {
    let (title, lines, name, typed, action) = dialog;
    let mut body = body_lines(theme, lines);
    body.push(Line::default());
    body.push(Line::from(vec![
        Span::raw("Type "),
        Span::styled(name.clone(), theme.style(Role::Danger)),
        Span::raw(" to confirm: "),
        Span::styled(typed.clone(), theme.style(Role::Title)),
    ]));
    let ready = typed == name;
    let kind = if ready {
        Kind::Destructive
    } else {
        Kind::Disabled
    };
    let cancel = if ready { Kind::Normal } else { Kind::Default };
    let buttons = vec![
        ("Cancel".to_owned(), cancel),
        (verb(action).to_owned(), kind),
    ];
    let title = title.clone();
    alert(
        theme,
        frame,
        area,
        Alert {
            title,
            body,
            buttons,
            danger: true,
        },
    );
}

/// An error alert: what failed, why, what can be done; **OK**.
fn error(
    theme: Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: (&String, &Vec<String>, &Vec<String>),
) {
    let (what, why, next) = dialog;
    let mut body: Vec<Line<'static>> = why
        .iter()
        .map(|reason| Line::from(format!("{}.", capitalised(reason))))
        .collect();
    if !next.is_empty() {
        body.push(Line::default());
        body.push(Line::styled("You can:", theme.style(Role::Title)));
        let bullet = theme.glyph(Glyph::Bullet);
        let steps = next
            .iter()
            .map(|step| Line::from(format!("{bullet} {}", capitalised(step))));
        body.extend(steps);
    }
    let title = format!("{} {what}", theme.glyph(Glyph::Failed));
    let buttons = vec![("OK".to_owned(), Kind::Default)];
    alert(
        theme,
        frame,
        area,
        Alert {
            title,
            body,
            buttons,
            danger: true,
        },
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
    let rect = centred(frame, area, area.width.saturating_sub(4).min(70), height);
    let count = format!("{} of {}", palette.matches.len(), items.len());
    let block = lists::counted(theme, "Commands", &count);
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
            let room = usize::from(list.width).saturating_sub(2);
            let label = fit(&item.label, room, theme.is_ascii());
            let line = Line::from(lists::highlighted(theme, &label, &ranked.positions));
            Some(ListItem::new(line))
        })
        .collect();
    let widget = List::new(rows)
        .highlight_symbol(Line::from(format!("{} ", theme.glyph(Glyph::Selected))))
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(theme.style(Role::Selected));
    let mut state = ListState::default().with_selected(Some(palette.selected));
    frame.render_stateful_widget(widget, list, &mut state);
}
