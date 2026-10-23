//! Lists: a box with a title, rows with the filter's matches highlighted,
//! the selection, and the filter line while one is set.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState as WidgetState, Paragraph};

use crate::menu::model::{ListState, Model};
use crate::menu::theme::{Role, Theme};

/// A box around a view, with its title.
pub(super) fn block(theme: Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_set(theme.border())
        .border_style(theme.style(Role::Border))
        .title(Span::styled(format!(" {title} "), theme.style(Role::Title)))
}

/// `text` as spans, with the characters at `positions` in the match style.
pub(super) fn highlighted(theme: Theme, text: &str, positions: &[usize]) -> Vec<Span<'static>> {
    if positions.is_empty() {
        return vec![Span::raw(text.to_owned())];
    }
    text.chars()
        .enumerate()
        .map(|(index, c)| {
            let role = if positions.contains(&index) {
                Role::Match
            } else {
                Role::Text
            };
            Span::styled(c.to_string(), theme.style(role))
        })
        .collect()
}

/// A list in a box: `rows` in the order of `state.matches`, the selection
/// marked, the filter line under it while a filter is set.
pub(super) fn render(
    model: &Model,
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    state: &ListState,
    rows: Vec<Line<'static>>,
) {
    let theme = model.theme;
    let block = block(theme, title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let show_filter = state.filtering || !state.filter.is_empty();
    let [list_area, filter_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(u16::from(show_filter)),
    ])
    .areas(inner);
    if rows.is_empty() {
        let empty = if state.filter.is_empty() {
            "nothing here".to_owned()
        } else {
            format!("nothing matches \"{}\"", state.filter)
        };
        frame.render_widget(
            Paragraph::new(empty).style(theme.style(Role::Muted)),
            list_area,
        );
    } else {
        let items: Vec<ListItem<'static>> = rows.into_iter().map(ListItem::new).collect();
        let list = List::new(items)
            .highlight_symbol("> ")
            .highlight_style(theme.style(Role::Selected));
        let mut widget = WidgetState::default().with_selected(Some(state.selected));
        frame.render_stateful_widget(list, list_area, &mut widget);
    }
    if show_filter {
        let text = format!("/{}", state.filter);
        let column = u16::try_from(text.chars().count()).unwrap_or(u16::MAX);
        frame.render_widget(Paragraph::new(text), filter_area);
        if state.filtering {
            frame.set_cursor_position(Position::new(
                filter_area.x.saturating_add(column),
                filter_area.y,
            ));
        }
    }
}

/// A two-column row: a name with its matches, then facts in the muted
/// style, padded so that the facts line up.
pub(super) fn row(
    theme: Theme,
    name: &str,
    positions: &[usize],
    width: usize,
    facts: Vec<Span<'static>>,
) -> Line<'static> {
    let mut spans = highlighted(theme, name, positions);
    let length = name.chars().count();
    if !facts.is_empty() {
        spans.push(Span::raw(" ".repeat(width.saturating_sub(length).max(1))));
        spans.extend(facts);
    }
    Line::from(spans)
}

/// The width of the name column: the longest name, within `limit`.
pub(super) fn name_width<'a>(names: impl Iterator<Item = &'a str>, limit: usize) -> usize {
    names
        .map(|name| name.chars().count())
        .max()
        .unwrap_or(0)
        .min(limit)
        + 2
}
