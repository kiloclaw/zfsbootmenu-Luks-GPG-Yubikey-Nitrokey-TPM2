//! The path above the columns of the Boot tab (ADR 0007, decision 2): what
//! each column selects, `zroot › zroot/ROOT/void › 6.12.9_1`, the deepest
//! last, cut at the right when it does not fit.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::super::lists;
use crate::menu::model::Model;
use crate::menu::theme::{Glyph, Role};

/// The rows the path takes: one, none without a selection.
pub(super) fn height(model: &Model) -> u16 {
    u16::from(!model.path().is_empty())
}

/// Draws the path in `area`.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Paragraph::new(line(model, usize::from(area.width))), area);
}

/// The path as a line `width` columns wide at most: each part after a
/// margin, the parts apart by the path glyph, the deepest in bold.
fn line(model: &Model, width: usize) -> Line<'static> {
    let theme = model.theme;
    let path = model.path();
    let separator = format!(" {} ", theme.glyph(Glyph::Path));
    let last = path.len().saturating_sub(1);
    let mut spans = vec![Span::raw(" ")];
    for (index, part) in path.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(separator.clone(), theme.style(Role::Muted)));
        }
        let role = if index == last {
            Role::Title
        } else {
            Role::Text
        };
        spans.push(Span::styled(part, theme.style(role)));
    }
    lists::clip(Line::from(spans), width.saturating_sub(1), theme.is_ascii())
}
