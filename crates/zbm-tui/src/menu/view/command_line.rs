//! The command palette, over the dimmed view: the query, then the best
//! matches.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::{HighlightSpacing, List, ListItem, ListState, Paragraph};

use super::dialogs::centred;
use super::lists;
use crate::menu::format::fit;
use crate::menu::model::Model;
use crate::menu::theme::{Glyph, Role};
use crate::text::text;

/// The command palette: the query, then the best matches.
pub(super) fn palette(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let Some(palette) = &model.palette else {
        return;
    };
    let theme = model.theme;
    let items = &palette.items;
    let height = area.height.saturating_sub(2).min(16);
    let width = area.width.saturating_sub(4).min(70);
    let rect = centred(theme, frame, area, (width, height));
    let count = text!(
        "list-count-of",
        shown = palette.matches.len(),
        total = items.len()
    );
    let block = lists::counted(theme, &text!("palette-title"), &count);
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
