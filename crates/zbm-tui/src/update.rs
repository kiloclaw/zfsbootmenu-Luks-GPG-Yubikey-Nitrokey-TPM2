//! What a message does to the menu.

use zbm_types::DatasetName;

use crate::filter;
use crate::keymap::Intent;
use crate::model::Model;

/// Something that happened to the menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Message {
    /// A bound key was pressed.
    Intent(Intent),
    /// A character was typed into the filter.
    Type(char),
}

/// What the menu asks of the program that runs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    /// Boot this environment.
    Boot(DatasetName),
    /// Leave the menu without booting.
    Quit,
}

/// The menu after `message`, and what it asks for.
///
/// This is the whole behaviour of the menu, and it is pure: no terminal, no
/// clock, no system.
pub fn update(model: Model, message: Message) -> (Model, Vec<Request>) {
    let mut model = model;
    let mut requests = Vec::new();
    match message {
        Message::Type(c) => {
            model.filter.push(c);
            refilter(&mut model);
        }
        Message::Intent(Intent::Up) => model.selected = model.selected.saturating_sub(1),
        Message::Intent(Intent::Down) => {
            let last = model.matches.len().saturating_sub(1);
            model.selected = (model.selected + 1).min(last);
        }
        Message::Intent(Intent::Boot) => {
            if let Some(entry) = model.selected() {
                requests.push(Request::Boot(entry.name.clone()));
            }
        }
        Message::Intent(Intent::DeleteChar) => {
            model.filter.pop();
            refilter(&mut model);
        }
        Message::Intent(Intent::ClearFilter) => {
            model.filter.clear();
            refilter(&mut model);
        }
        Message::Intent(Intent::Back) if !model.filter.is_empty() => {
            model.filter.clear();
            refilter(&mut model);
        }
        Message::Intent(Intent::Back | Intent::Quit) => requests.push(Request::Quit),
    }
    (model, requests)
}

/// Matches the filter again. While a filter is typed, the best match is
/// selected; when the filter is cleared, the entry that was selected stays
/// selected.
fn refilter(model: &mut Model) {
    let previous = model.matches.get(model.selected).map(|found| found.entry);
    model.matches = filter::matches(&model.entries, &model.filter);
    model.selected = previous
        .and_then(|entry| model.matches.iter().position(|found| found.entry == entry))
        .filter(|_| model.filter.is_empty())
        .unwrap_or(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_app::Snapshot;

    fn menu() -> Model {
        Model::new(&Snapshot::example(), false)
    }

    /// The menu after `messages`, and the requests of the last one.
    fn after(messages: &[Message]) -> (Model, Vec<Request>) {
        messages
            .iter()
            .fold((menu(), Vec::new()), |(model, _), message| {
                update(model, *message)
            })
    }

    fn typed(text: &str) -> Vec<Message> {
        text.chars().map(Message::Type).collect()
    }

    fn selected(model: &Model) -> Option<&str> {
        model.selected().map(|entry| entry.name.as_str())
    }

    #[test]
    fn enter_boots_the_default_when_nothing_was_typed() {
        let (_, requests) = after(&[Message::Intent(Intent::Boot)]);

        assert_eq!(
            requests,
            [Request::Boot("zroot/ROOT/void".parse().unwrap())]
        );
    }

    #[test]
    fn up_and_down_move_within_the_list_and_stop_at_its_ends() {
        let cases: [(&str, &[Intent], &str); 3] = [
            ("down once", &[Intent::Down], "tank/ROOT/arch"),
            (
                "past the end",
                &[Intent::Down, Intent::Down, Intent::Down],
                "zroot/ROOT/ubuntu",
            ),
            ("above the top", &[Intent::Up], "zroot/ROOT/void"),
        ];

        for (name, intents, expected) in cases {
            let messages: Vec<Message> = intents.iter().map(|i| Message::Intent(*i)).collect();
            let (model, _) = after(&messages);
            assert_eq!(selected(&model), Some(expected), "case {name}");
        }
    }

    #[test]
    fn typing_filters_and_selects_the_best_match() {
        let (model, _) = after(&typed("arch"));

        assert_eq!(model.matches().len(), 1);
        assert_eq!(selected(&model), Some("tank/ROOT/arch"));
    }

    #[test]
    fn a_filter_that_matches_nothing_boots_nothing() {
        let mut messages = typed("nothing");
        messages.push(Message::Intent(Intent::Boot));

        let (model, requests) = after(&messages);

        assert_eq!(selected(&model), None);
        assert_eq!(requests, []);
    }

    #[test]
    fn backspace_and_ctrl_u_widen_the_filter_again() {
        let mut messages = typed("archx");
        messages.push(Message::Intent(Intent::DeleteChar));
        let (model, _) = after(&messages);
        assert_eq!(model.filter(), "arch");

        messages.push(Message::Intent(Intent::ClearFilter));
        let (model, _) = after(&messages);
        assert_eq!(model.filter(), "");
        assert_eq!(model.matches().len(), 3);
    }

    #[test]
    fn esc_clears_the_filter_first_and_quits_second() {
        let mut messages = typed("void");
        messages.push(Message::Intent(Intent::Back));
        let (model, requests) = after(&messages);
        assert_eq!(model.filter(), "");
        assert_eq!(requests, []);

        messages.push(Message::Intent(Intent::Back));
        let (_, requests) = after(&messages);
        assert_eq!(requests, [Request::Quit]);
    }

    #[test]
    fn ctrl_c_quits_even_with_a_filter() {
        let mut messages = typed("void");
        messages.push(Message::Intent(Intent::Quit));

        let (_, requests) = after(&messages);

        assert_eq!(requests, [Request::Quit]);
    }

    #[test]
    fn clearing_the_filter_keeps_the_selected_entry() {
        let mut messages = vec![Message::Intent(Intent::Down), Message::Intent(Intent::Down)];
        messages.push(Message::Intent(Intent::ClearFilter));

        let (model, _) = after(&messages);

        assert_eq!(selected(&model), Some("zroot/ROOT/ubuntu"));
    }
}
