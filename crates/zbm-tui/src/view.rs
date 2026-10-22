//! Drawing the menu.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use zbm_app::Encryption;

use crate::keymap;
use crate::model::{Match, Model};

/// Borders of ASCII characters, for terminals without line drawing.
const ASCII_BORDER: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// The prompt in front of the filter text.
const PROMPT: &str = "Filter: ";

/// Draws `model` into `frame`: a title, the list in a box, the filter line
/// and the help line.
pub fn view(model: &Model, frame: &mut Frame<'_>) {
    let [title, list, filter, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let counts = format!(
        "ZFSBootMenu: {} of {} boot environments",
        model.matches().len(),
        model.entries().len()
    );
    frame.render_widget(Paragraph::new(counts), title);
    render_list(model, frame, list);
    frame.render_widget(
        Paragraph::new(format!("{PROMPT}{}", model.filter())),
        filter,
    );
    let column = u16::try_from(PROMPT.len() + model.filter().chars().count()).unwrap_or(u16::MAX);
    frame.set_cursor_position(Position::new(filter.x.saturating_add(column), filter.y));
    let help_text = keymap::help_line(usize::from(help.width));
    frame.render_widget(Paragraph::new(help_text), help);
}

/// The boxed list of matching entries, the selected one reversed.
fn render_list(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let border = if model.is_ascii() {
        ASCII_BORDER
    } else {
        border::PLAIN
    };
    let block = Block::bordered()
        .border_set(border)
        .title(" Boot environments ");
    if model.matches().is_empty() {
        let message = format!("no boot environment matches {:?}", model.filter());
        frame.render_widget(Paragraph::new(message).block(block), area);
        return;
    }
    let items: Vec<ListItem<'_>> = model
        .matches()
        .iter()
        .filter_map(|found| item(model, found))
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_symbol("> ")
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(model.selected_index()));
    frame.render_stateful_widget(list, area, &mut state);
}

/// One line of the list: `*` for a default, the name with the matched
/// characters underlined, and the encryption.
fn item<'a>(model: &'a Model, found: &Match) -> Option<ListItem<'a>> {
    let entry = model.entries().get(found.entry)?;
    let marker = if entry.is_default { "* " } else { "  " };
    let mut spans = vec![Span::raw(marker)];
    let matched = Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    for (index, c) in entry.name.as_str().chars().enumerate() {
        let style = if found.positions.contains(&index) {
            matched
        } else {
            Style::new()
        };
        spans.push(Span::styled(c.to_string(), style));
    }
    let tag = match entry.encryption {
        Encryption::None => "",
        Encryption::Locked => "  [locked]",
        Encryption::Unlocked => "  [unlocked]",
    };
    spans.push(Span::raw(tag));
    Some(ListItem::new(Line::from(spans)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::{Message, update};
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use zbm_app::Snapshot;

    /// The screen of `model` on a terminal of `columns` by `rows`.
    fn screen(model: &Model, columns: u16, rows: u16) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(columns, rows)).unwrap();
        terminal.draw(|frame| view(model, frame)).unwrap();
        terminal.backend().clone()
    }

    fn typed(text: &str) -> Model {
        text.chars()
            .fold(Model::new(&Snapshot::example(), false), |model, c| {
                update(model, Message::Type(c)).0
            })
    }

    #[test]
    fn the_list_at_80_by_25() {
        assert_snapshot!(screen(&Model::new(&Snapshot::example(), false), 80, 25));
    }

    #[test]
    fn the_list_at_120_by_40() {
        assert_snapshot!(screen(&Model::new(&Snapshot::example(), false), 120, 40));
    }

    #[test]
    fn the_list_at_52_by_20() {
        assert_snapshot!(screen(&Model::new(&Snapshot::example(), false), 52, 20));
    }

    #[test]
    fn the_list_in_ascii_for_simple_terminals() {
        assert_snapshot!(screen(&Model::new(&Snapshot::example(), true), 52, 20));
    }

    #[test]
    fn a_filtered_list_shows_the_matches_only() {
        assert_snapshot!(screen(&typed("ubu"), 80, 25));
    }

    #[test]
    fn a_filter_without_matches_says_so() {
        assert_snapshot!(screen(&typed("nothing"), 52, 20));
    }

    #[test]
    fn the_cursor_follows_the_filter_text() {
        let mut backend = screen(&typed("ub"), 80, 25);

        let cursor = ratatui::backend::Backend::get_cursor_position(&mut backend).unwrap();

        assert_eq!(cursor, Position::new(10, 23));
    }
}
