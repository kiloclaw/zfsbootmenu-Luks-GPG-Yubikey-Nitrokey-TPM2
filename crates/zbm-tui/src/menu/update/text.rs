//! Text fields: the filter of a list and the command line editor, and the
//! typing the command palette shares with them.

use zbm_types::Untrusted;

use crate::menu::keymap::{self, Input, Intent};
use crate::menu::model::{Editor, Model, Screen};
use crate::menu::world::{Action, CommandLine, Outcome};
use crate::text::text;

/// Applies a typing input to `text`; returns the intent when the input is
/// not typing, and leaves `text` alone.
pub(super) fn edit(text: &mut String, input: Input) -> Option<Intent> {
    match input {
        Input::Type(c) => text.push(c),
        Input::Erase => {
            text.pop();
        }
        Input::Clear => text.clear(),
        Input::Intent(intent) => return Some(intent),
    }
    None
}

/// The filter of the current list.
pub(super) fn filter(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(list) = model.list_mut() else {
        return Vec::new();
    };
    if edit(&mut list.filter, input).is_none() {
        model.refresh();
        if let Some(list) = model.list_mut() {
            list.selected = 0;
        }
        return Vec::new();
    }
    match input {
        Input::Intent(Intent::Back) => {
            list.filter.clear();
            list.filtering = false;
            model.refresh();
            Vec::new()
        }
        Input::Intent(Intent::Primary) => {
            list.filtering = false;
            if !keymap::is_offered(model.context(), Intent::Primary, model) {
                return super::offers::not_offered(model);
            }
            super::screens::intent(model, Intent::Primary)
        }
        Input::Intent(intent @ (Intent::Up | Intent::Down | Intent::PageUp | Intent::PageDown)) => {
            super::lists::move_selection(model, intent);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// The command line editor: `Enter` boots with the edited line, for this
/// boot only; in the save editor, it opens the dry run of the line saved
/// for every boot ([`save`]).
pub(super) fn editor(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(mut editor) = model.editor.take() else {
        model.screen = Screen::Main;
        return Vec::new();
    };
    if edit(&mut editor.text, input).is_none() {
        model.editor = Some(editor);
        return Vec::new();
    }
    match input {
        Input::Intent(Intent::Back) => {
            model.screen = Screen::Main;
            Vec::new()
        }
        Input::Intent(Intent::Primary) => {
            if let Some(problem) = check(&editor.text) {
                model.notification = Some(Outcome::Warning(problem));
                model.editor = Some(editor);
                return Vec::new();
            }
            if editor.save {
                return save(model, editor);
            }
            // The editor gives its line to the boot and closes: when the boot
            // fails, or the environment has gone, the main view says so.
            model.screen = Screen::Main;
            let Some(environment) = model
                .snapshot
                .environments
                .iter()
                .find(|e| e.name == editor.environment)
                .cloned()
            else {
                return Vec::new();
            };
            model.edited.retain(|(name, _)| *name != editor.environment);
            // This edit is newer than a save sent before: it stays.
            let saving = model.saving.as_ref().and_then(Action::changed_environment);
            if saving == Some(&editor.environment) {
                model.saving = None;
            }
            model.edited.push((
                editor.environment.clone(),
                Untrusted::new(editor.text.clone()),
            ));
            let action = super::screens::boot(model, &environment, editor.kernel.clone());
            super::screens::guarded(model, action)
        }
        _ => {
            model.editor = Some(editor);
            Vec::new()
        }
    }
}

/// The save editor's `Enter`: a line the machine would refuse to save is
/// refused here first ([`unsaved`]); else its dry run opens over the
/// editor, which stays open under it, so that `Esc` goes back to the
/// line. An empty line lets the environment inherit its command line.
fn save(model: &mut Model, editor: Editor) -> Vec<Action> {
    if let Some(problem) = unsaved(&editor.text) {
        model.notification = Some(Outcome::Warning(problem));
        model.editor = Some(editor);
        return Vec::new();
    }
    let environment = editor.environment.clone();
    let action = if editor.text.is_empty() {
        Action::InheritCommandLine(environment)
    } else {
        Action::SetCommandLine(environment, Untrusted::new(editor.text.clone()))
    };
    model.editor = Some(editor);
    super::dry_run::open(model, action)
}

/// Why the machine would refuse to save `line`, if it would, by its own
/// checks: a control character, or more bytes than ZFS keeps of a
/// property's value.
fn unsaved(line: &str) -> Option<String> {
    if CommandLine::has_control(line) {
        return Some(text!("command-line-control"));
    }
    CommandLine::is_too_long(line)
        .then(|| text!("command-line-too-long", longest = CommandLine::LONGEST))
}

/// What is wrong with a command line, if anything: an unclosed quote.
pub fn check(line: &str) -> Option<String> {
    let quotes = line.chars().filter(|c| *c == '"').count();
    (quotes % 2 == 1).then(|| text!("command-line-unclosed-quote"))
}
