//! The key section of `zfsbootmenu(7)`, written from the key map: the
//! workspace's keys in each place of the menu, and the classic aliases of
//! the `fzf` menu (ADR 0016, point 17). `cargo xtask gen-docs` writes them
//! into the page, between its markers, through the example `gen_keys`.
//!
//! The text is reStructuredText in the form of the option sections: the
//! names of the keys in bold, then what they do, indented. No table: the
//! legacy help viewer draws the page 52 columns wide.

use zbm_display::keys::Key;

use super::classic::{aliases as letters, sent_as, taken_with_ctrl};
use super::{
    CLASSIC, COMMAND_NAMES, Context, Input, Intent, Map, Modifiers, describe, input, key_name,
};
use crate::text::{self, Id, Language, id, text};
use Context::{
    Choose, Diff, Help, Kernels, Log, Main, Packages, Pools, Recovery, Review, Snapshots, Startup,
};

/// The views of the tabs. The keys they have in common, with one meaning,
/// are listed once, for every view; each lists only its other keys.
const VIEWS: &[Context] = &[
    Main, Kernels, Snapshots, Pools, Diff, Packages, Recovery, Log,
];

/// The places listed whole after the views: help, start-up, dialogs,
/// fields and the command line.
const OTHERS: &[Context] = &[
    Help,
    Startup,
    Choose,
    Context::Text,
    Context::Command,
    Review,
];

/// The keys of a field outside the table, which `input` reads: Backspace
/// erases, `Ctrl-U` clears.
const EDITING: &[(Key, Modifiers)] = &[
    (Key::Backspace, Modifiers::NONE),
    (Key::Char('u'), Modifiers::CTRL),
];

/// A key of a place: its name, what it does there and in which of its
/// fields or dialogs, when not in all, and what it does in the main view.
struct Bound {
    name: String,
    what: Id,
    at: Option<Id>,
    usual: Id,
}

/// The workspace's keys in each place of the menu, as definition entries
/// under a title for each place: first those every view has, then each
/// view's own, then the places listed whole.
pub fn workspace() -> String {
    let common: Vec<Bound> = keys(Main)
        .into_iter()
        .filter(|key| {
            VIEWS
                .iter()
                .all(|view| keys(*view).iter().any(|other| is_same(key, other)))
        })
        .collect();
    let mut places = vec![place(&text!("manual-every-view"), &common)];
    for view in VIEWS {
        let own: Vec<Bound> = keys(*view)
            .into_iter()
            .filter(|key| {
                key.what != key.usual
                    || key.at.is_some()
                    || !common.iter().any(|seen| is_same(key, seen))
            })
            .collect();
        places.push(place(&name(*view), &own));
    }
    for other in OTHERS {
        places.push(place(&name(*other), &keys(*other)));
        if *other == Context::Command {
            places.push(command_names());
        }
    }
    places.join("\n")
}

/// The names of the command line's commands, under their title: each in
/// bold, with its English name where the page's language has another, and
/// what it does.
fn command_names() -> String {
    let title = text!("manual-command-names");
    let underline = "~".repeat(title.chars().count());
    let entries: Vec<String> = COMMAND_NAMES
        .iter()
        .map(|(id, what)| {
            let mut names = vec![text::get(*id)];
            let english = text::in_language(Language::English, || text::get(*id));
            if !names.contains(&english) {
                names.push(english);
            }
            let what = text::get(*what);
            entry(&names, &sentence_of(&what))
        })
        .collect();
    let intro = text!("manual-command-names-intro");
    format!("{title}\n{underline}\n\n{intro}\n\n{}", entries.join("\n"))
}

/// The classic aliases of the `fzf` menu that the workspace keeps, one
/// entry for each letter: `MOD+K`, or `Alt+K` where the menu binds the
/// letter with `Ctrl` itself, as the help view says; then the letter's
/// meaning in each view, and where a terminal sends the letter with `Ctrl`
/// as another key.
pub fn aliases() -> String {
    let entries: Vec<String> = CLASSIC
        .iter()
        .map(|(letter, meanings)| {
            let mut keys: Vec<String> = Vec::new();
            let mut said = Vec::new();
            for (view, _) in *meanings {
                let Some((_, key, what)) = letters(*view, &|_| true)
                    .into_iter()
                    .find(|(other, ..)| other == letter)
                else {
                    continue;
                };
                if !keys.contains(&key) {
                    keys.push(key);
                }
                said.push(format!("{}: {}", name(*view), text::get(what)));
            }
            let mut body = format!("{}.", said.join("; "));
            if let Some((view, intent)) = meanings
                .iter()
                .find_map(|(view, _)| Some((*view, taken_with_ctrl(*letter, *view)?)))
            {
                let key = key_name(Key::Char(*letter), Modifiers::CTRL);
                let action = text::title(describe(view, intent));
                body.push(' ');
                body.push_str(&text!("manual-alt-only", key = key, action = action));
            }
            if let Some(sent) = sent_as(*letter) {
                let key = key_name(Key::Char(*letter), Modifiers::CTRL);
                let sent = key_name(sent.key, Modifiers::of(&sent));
                body.push(' ');
                body.push_str(&text!("manual-sent-as", key = key, sent = sent));
            }
            entry(&keys, &body)
        })
        .collect();
    entries.join("\n")
}

/// The keys of `context` one by one, in the order of the help view, and
/// the keys of a field that `input` reads; not those the menu does nothing
/// with there ([`said`]).
fn keys(context: Context) -> Vec<Bound> {
    let map = Map::Workspace;
    let mut keys: Vec<Bound> = map
        .table()
        .into_iter()
        .filter(|binding| map.is_bound(binding, context))
        .filter_map(|binding| {
            let (what, at) = said(context, binding.intent)?;
            Some(Bound {
                name: key_name(binding.key, binding.modifiers),
                what,
                at,
                usual: toward(binding.intent, describe(Main, binding.intent)),
            })
        })
        .collect();
    keys.extend(EDITING.iter().filter_map(|(key, modifiers)| {
        let what = match input(map, context, super::press(*key, *modifiers))? {
            Input::Erase => id!("action-erase"),
            Input::Clear => id!("action-clear"),
            Input::Intent(_) | Input::Type(_) => return None,
        };
        Some(Bound {
            name: key_name(*key, *modifiers),
            what,
            at: None,
            usual: what,
        })
    }));
    keys
}

/// What a key bound to `intent` does in `context`, as the page says it,
/// and where, when only some of the place's fields or dialogs read it; or
/// `None` where the menu does nothing with it though the table binds it:
/// the primary action of a view with nothing to choose, the moves of
/// start-up, which has nothing to select, `Right` in the kernels column,
/// already open, help outside the views and the command line, the first
/// and last row of a dialog, and a page of the command line.
fn said(context: Context, intent: Intent) -> Option<(Id, Option<Id>)> {
    use Intent::{Down, First, Last, PageDown, PageUp, Primary, Reveal, Up};
    let ignored = match context {
        Startup => matches!(intent, Primary | Up | Down),
        Help | Log => intent == Primary,
        Kernels => intent == Intent::Kernels,
        Choose => matches!(intent, Intent::Help | First | Last),
        Context::Text | Review => intent == Intent::Help,
        Context::Command => matches!(intent, PageUp | PageDown),
        _ => false,
    };
    if ignored {
        return None;
    }
    let at = match (context, intent) {
        (Choose, Up | Down) => Some(id!("manual-in-choices")),
        (Choose, PageUp | PageDown) => Some(id!("manual-on-review-pages")),
        (Context::Text, Up | Down) => Some(id!("manual-in-lists")),
        (Context::Text, PageUp | PageDown) => Some(id!("manual-in-filters")),
        (Context::Text, Reveal) => Some(id!("manual-in-unlock")),
        _ => None,
    };
    Some((toward(intent, describe(context, intent)), at))
}

/// `what`, said with the way `intent` goes, when it moves: up or down, a
/// page, or to an end. The help view says "Select" for both ways; the
/// page, which merges keys by what they do, says which.
fn toward(intent: Intent, what: Id) -> Id {
    let scrolls = what == id!("action-scroll");
    match intent {
        Intent::Up if scrolls => id!("manual-scroll-up"),
        Intent::Down if scrolls => id!("manual-scroll-down"),
        Intent::Up => id!("manual-select-up"),
        Intent::Down => id!("manual-select-down"),
        Intent::PageUp => id!("manual-page-up"),
        Intent::PageDown => id!("manual-page-down"),
        Intent::First => id!("manual-top"),
        Intent::Last => id!("manual-bottom"),
        _ => what,
    }
}

/// Whether two places bind a key to one meaning, as the main view says it.
fn is_same(key: &Bound, other: &Bound) -> bool {
    key.name == other.name && key.usual == other.usual
}

/// A place: its title, underlined, and an entry for each meaning, with the
/// keys that have it.
fn place(title: &str, keys: &[Bound]) -> String {
    let mut meanings: Vec<(Vec<String>, Id, Option<Id>)> = Vec::new();
    for key in keys {
        match meanings
            .iter_mut()
            .find(|(_, what, at)| *what == key.what && *at == key.at)
        {
            Some((names, ..)) if names.contains(&key.name) => {}
            Some((names, ..)) => names.push(key.name.clone()),
            None => meanings.push((vec![key.name.clone()], key.what, key.at)),
        }
    }
    let underline = "~".repeat(title.chars().count());
    let entries: Vec<String> = meanings
        .iter()
        .map(|(names, what, at)| entry(names, &sentence(*what, *at)))
        .collect();
    format!("{title}\n{underline}\n\n{}", entries.join("\n"))
}

/// One definition entry: the keys in bold, a blank line and what they do,
/// indented, as the option sections write theirs.
fn entry(keys: &[String], body: &str) -> String {
    let keys: Vec<String> = keys.iter().map(|key| format!("**{key}**")).collect();
    format!("{}\n\n  {body}\n", keys.join(", "))
}

/// What a key does as a sentence: the message, its first letter capital,
/// then where, if not everywhere in its place.
fn sentence(what: Id, at: Option<Id>) -> String {
    let said = match at {
        Some(at) => format!("{}, {}", text::get(what), text::get(at)),
        None => text::get(what),
    };
    sentence_of(&said)
}

/// `said` as a sentence: its first letter capital, and a full stop.
fn sentence_of(said: &str) -> String {
    let mut chars = said.chars();
    let first: String = chars
        .next()
        .into_iter()
        .flat_map(char::to_uppercase)
        .collect();
    format!("{first}{}.", chars.as_str())
}

/// The name of a place: its view's title, or the name of a dialog or a
/// field.
fn name(context: Context) -> String {
    match context {
        Startup => text!("view-startup"),
        Main => text!("view-main"),
        Snapshots => text!("view-snapshots"),
        Kernels => text!("view-kernels"),
        Pools => text!("view-pools"),
        Recovery => text!("view-recovery"),
        Log => text!("view-log"),
        Help => text!("view-help"),
        Diff => format!("{}: {}", text!("view-changes"), text!("segment-files")),
        Packages => format!("{}: {}", text!("view-changes"), text!("segment-packages")),
        Choose => text!("manual-sheets"),
        Context::Text => text!("manual-fields"),
        Context::Command => text!("manual-command-line"),
        Review => text!("review-title"),
    }
}

#[cfg(test)]
mod tests;
