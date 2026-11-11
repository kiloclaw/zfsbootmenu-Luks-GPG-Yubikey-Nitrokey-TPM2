//! The review page of a change (ADR 0016, points 12 to 15): it opens and
//! asks the machine for the change's plan, takes the plan of that change
//! when it comes, and asks for the change only with that plan, nothing that
//! stops it, and the target's name typed as it is.

use super::dialogs;
use crate::menu::keymap::{Input, Intent};
use crate::menu::model::{Dialog, Focus, Model, Reviewing, Then};
use crate::menu::world::{Action, Outcome, Plan, Safer};
use crate::text::text;

/// How many rows `PgUp` and `PgDn` scroll what is lost.
const PAGE: usize = 5;

/// Opens the review page of `action`, and asks the machine for its plan;
/// an action without a target to name is a programming error, which
/// yields an error.
pub(super) fn open(model: &mut Model, action: Action) -> Vec<Action> {
    let Some(reviewing) = Reviewing::new(action.clone()) else {
        model.dialog = Some(dialogs::confirm(Then::Review(action)));
        return Vec::new();
    };
    model.dialog = Some(Dialog::Review(Box::new(reviewing)));
    vec![Action::Plan(Box::new(action))]
}

/// The plan of `action` came: the page of that change takes it. Another
/// plan, as one a page closed since asked for, is dropped.
pub(super) fn planned(model: &mut Model, action: &Action, plan: Plan) {
    if let Some(Dialog::Review(reviewing)) = &mut model.dialog
        && reviewing.action == *action
    {
        reviewing.plan = Some(plan);
        if reviewing.is_refused() {
            reviewing.focus = Focus::Field;
        }
    }
}

/// The machine changed while a page shows a plan: what it planned may no
/// longer hold, so the page plans the change again. A plan only reads the
/// machine, which is not read again after it: this asks once.
pub(super) fn again(model: &mut Model) -> Vec<Action> {
    match &mut model.dialog {
        Some(Dialog::Review(reviewing)) if reviewing.plan.is_some() => {
            reviewing.plan = None;
            vec![Action::Plan(Box::new(reviewing.action.clone()))]
        }
        _ => Vec::new(),
    }
}

/// An input on the page: the name typed, the focus moved, what is lost
/// scrolled; `Esc` closes it and asks for nothing.
pub(super) fn handle(
    model: &mut Model,
    input: Input,
    mut reviewing: Box<Reviewing>,
) -> Vec<Action> {
    let has_safer = reviewing.safer(&model.snapshot.offers).is_some();
    let refused = reviewing.is_refused();
    match input {
        Input::Intent(Intent::Back) => return Vec::new(),
        Input::Intent(Intent::Primary) => return primary(model, reviewing, has_safer),
        Input::Intent(intent) => scroll_or_move(&mut reviewing, intent, has_safer),
        Input::Type(_) | Input::Erase | Input::Clear => {
            if let Some(typed) = reviewing.typed.as_mut().filter(|_| !refused) {
                edit(typed, input);
            }
            reviewing.focus = Focus::Field;
        }
    }
    model.dialog = Some(Dialog::Review(reviewing));
    Vec::new()
}

/// What `input` types into the field holding `typed`.
fn edit(typed: &mut String, input: Input) {
    match input {
        Input::Type(c) => typed.push(c),
        Input::Erase => {
            typed.pop();
        }
        Input::Clear => typed.clear(),
        Input::Intent(_) => {}
    }
}

/// `intent` on the page: the arrows and `PgUp` `PgDn` scroll what is
/// lost, within its rows; `Tab` and `Shift-Tab` move the focus.
fn scroll_or_move(reviewing: &mut Reviewing, intent: Intent, has_safer: bool) {
    let last = reviewing.lost().len().saturating_sub(1);
    let scroll = reviewing.scroll;
    reviewing.scroll = match intent {
        Intent::Up => scroll.saturating_sub(1),
        Intent::Down => scroll.saturating_add(1),
        Intent::PageUp => scroll.saturating_sub(PAGE),
        Intent::PageDown => scroll.saturating_add(PAGE),
        _ => scroll,
    }
    .min(last);
    match intent {
        Intent::NextTab => reviewing.focus = reviewing.moved(true, has_safer),
        Intent::PreviousTab => reviewing.focus = reviewing.moved(false, has_safer),
        _ => {}
    }
}

/// `Enter`: on a page that refuses the change, it closes it; on the safer
/// action, it takes it; else it asks for the change once it is planned and
/// named, and the status line says what is missing until then.
fn primary(model: &mut Model, reviewing: Box<Reviewing>, has_safer: bool) -> Vec<Action> {
    if reviewing.is_refused() {
        return Vec::new();
    }
    if has_safer && reviewing.focus == Focus::Safer {
        return safer(model, &reviewing);
    }
    if reviewing.is_ready() {
        super::screens::sent(model, &reviewing.action);
        return vec![reviewing.action];
    }
    let missing = if reviewing.plan.is_none() {
        text!("review-wait")
    } else {
        text!("confirm-type-first", name = reviewing.name.as_str())
    };
    model.notification = Some(Outcome::Warning(missing));
    model.dialog = Some(Dialog::Review(reviewing));
    Vec::new()
}

/// The safer action of the page, in place of the change: a clone of the
/// snapshot, named first, or the import that writes nothing.
fn safer(model: &mut Model, reviewing: &Reviewing) -> Vec<Action> {
    let then = match reviewing.safer(&model.snapshot.offers) {
        Some(Safer::Clone(snapshot)) => super::snapshots::clone(model, snapshot),
        Some(Safer::Import(action)) => Then::Act((**action).clone()),
        None => return Vec::new(),
    };
    dialogs::follow(model, then)
}
