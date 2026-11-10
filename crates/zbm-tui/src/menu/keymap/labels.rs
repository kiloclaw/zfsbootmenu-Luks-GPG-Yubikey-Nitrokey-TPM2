//! What keys say: the toolbar, the help view and the names of keys, from
//! the catalogue.

use zbm_display::keys::Key;

use super::{Context, Footer, Intent, Map, Modifiers, Tab};
use crate::text::{self, Id, id, text};
use Context::{Command, Diff, Kernels, Main, Packages, Pools, Recovery, Snapshots, Startup, Text};

/// The name of the view of `context`, as help says it, and the command
/// line's entry of that help.
pub(crate) fn view_name(context: Context) -> Id {
    match context {
        Startup => id!("view-startup"),
        Snapshots => id!("view-snapshots"),
        Kernels => id!("view-kernels"),
        Pools => id!("view-pools"),
        Recovery => id!("view-recovery"),
        Context::Log => id!("view-log"),
        Diff | Packages => id!("view-changes"),
        Command => id!("view-commands"),
        _ => id!("view-main"),
    }
}

/// What a key does for an intent in a context, as the toolbar and the
/// help view say it: a message of the catalogue with a `.title`.
pub fn describe(context: Context, intent: Intent) -> Id {
    if context == Context::Review {
        return on_review_page(intent);
    }
    match (context, intent) {
        (Main | Kernels, Intent::Primary) | (_, Intent::Boot) => id!("action-boot"),
        (Snapshots | Context::Choose, Intent::Primary) => id!("action-choose"),
        (Pools, Intent::Primary) => id!("action-details"),
        (Diff, Intent::Primary) => id!("action-restore"),
        (Packages, Intent::Primary) => id!("action-show-files"),
        (Recovery, Intent::Primary) => id!("action-run"),
        (Startup | Text | Command, Intent::Primary) => id!("action-confirm"),
        (Context::Choose | Text | Command, Intent::Back) => id!("action-cancel"),
        (_, Intent::Back) => id!("action-back"),
        (_, Intent::Kernels) => id!("action-kernels"),
        (_, Intent::Snapshots) => id!("action-snapshots"),
        (_, Intent::Edit) => id!("action-edit"),
        (_, Intent::SetDefault) => id!("action-default"),
        (_, Intent::Pools) => id!("action-pools"),
        (_, Intent::ReadWrite) => id!("action-read-write"),
        (_, Intent::Chroot) => id!("action-chroot"),
        (_, Intent::Sort) => id!("action-sort"),
        (_, Intent::Log) => id!("action-log"),
        (_, Intent::Recovery) => id!("action-recovery"),
        (_, Intent::Power) => id!("action-power"),
        (_, Intent::Rollback) => id!("action-roll-back"),
        (_, Intent::Destroy) => id!("action-destroy"),
        (_, Intent::Clone) => id!("action-clone"),
        (_, Intent::CloneAndPromote) => id!("action-clone-promote"),
        (_, Intent::Duplicate) => id!("action-duplicate"),
        (_, Intent::NewSnapshot) => id!("action-snapshot"),
        (_, Intent::Diff) => id!("action-diff"),
        (_, Intent::Mark) => id!("action-mark"),
        (_, Intent::Fold) => id!("action-fold"),
        (_, Intent::Unlock) => id!("action-unlock"),
        (_, Intent::Import) => id!("action-import"),
        (_, Intent::Rewind) => id!("action-rewind"),
        (_, Intent::Filter) => id!("action-filter"),
        (_, Intent::Palette) => id!("action-commands"),
        (_, Intent::Help) => id!("action-help"),
        (Context::Help | Context::Log, Intent::Up | Intent::Down) => id!("action-scroll"),
        (_, Intent::Primary | Intent::Up | Intent::Down) => id!("action-select"),
        (_, Intent::PageUp | Intent::PageDown | Intent::First | Intent::Last) => {
            id!("action-move")
        }
        (_, Intent::Redraw) => id!("action-redraw"),
        (_, Intent::RevertCommandLine) => id!("action-revert"),
        (_, Intent::Shell) => id!("action-shell"),
        (_, Intent::InheritDefault) => id!("action-inherit"),
        (_, Intent::Layout) => id!("action-keyboard-layout"),
        (_, Intent::Reveal) => id!("action-show-typed"),
        (_, Intent::Segment) => id!("action-segment"),
        (_, Intent::Tab(tab)) => to_tab(tab),
        (_, Intent::NextTab) => id!("action-next-tab"),
        (_, Intent::PreviousTab) => id!("action-previous-tab"),
        (_, Intent::Complete) => id!("action-complete"),
        (_, Intent::DeleteWord) => id!("action-delete-word"),
        (_, Intent::History) => id!("action-recent"),
    }
}

/// What the key of `tab` does: it chooses that tab.
fn to_tab(tab: Tab) -> Id {
    match tab {
        Tab::Boot => id!("action-tab-boot"),
        Tab::Snapshots => id!("action-tab-snapshots"),
        Tab::Pools => id!("action-tab-pools"),
        Tab::Log => id!("action-tab-log"),
    }
}

/// What a key does for an intent on a review page: it confirms the
/// change, cancels it, scrolls what it loses, or moves among its buttons.
fn on_review_page(intent: Intent) -> Id {
    match intent {
        Intent::Primary => id!("action-confirm"),
        Intent::Back => id!("action-cancel"),
        Intent::Up | Intent::Down | Intent::PageUp | Intent::PageDown => id!("action-scroll"),
        Intent::NextTab => id!("action-next-place"),
        Intent::PreviousTab => id!("action-previous-place"),
        other => describe(Context::Choose, other),
    }
}

/// The bindings of `context` in the key map `map` shown in the toolbar, in
/// order: (intent, key, what it does, as `said` says it). Only the intents
/// `offered` says the machine offers are shown; not the arrows of the
/// workspace's start-up, which has nothing to select.
pub fn bindings(
    map: Map,
    context: Context,
    offered: &dyn Fn(Intent) -> bool,
    said: &dyn Fn(Intent) -> Id,
) -> Vec<(Intent, String, Id)> {
    let idle = matches!(context, Context::Startup | Context::Help | Context::Log);
    let still = map == Map::Workspace && context == Startup;
    map.table()
        .into_iter()
        .filter(|binding| map.is_bound(binding, context) && offered(binding.intent))
        .filter(|binding| !(idle && binding.intent == Intent::Primary))
        .filter(|binding| !(still && matches!(binding.intent, Intent::Up | Intent::Down)))
        .filter_map(|binding| {
            let key = match binding.footer {
                Footer::Hidden => return None,
                Footer::Key => key_name(binding.key, binding.modifiers),
                Footer::Keys(id) => text::get(id),
            };
            Some((binding.intent, key, said(binding.intent)))
        })
        .collect()
}

/// The most actions the toolbar shows (`apple-hig.md`, rule 9).
pub const TOOLBAR: usize = 5;

/// The space between toolbar items.
pub const TOOLBAR_GAP: usize = 3;

/// An action of the toolbar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tool {
    /// Its key.
    pub key: String,
    /// What it does, as a title.
    pub label: String,
    /// Whether it is `? More`, which opens help with every key.
    pub is_more: bool,
    /// The lock's mark after its label, the word of the PIN's cost, when
    /// the locked menu keeps what its key does (ADR 0010).
    pub mark: Option<String>,
}

impl Tool {
    /// The columns it takes: its key, a space and its label, then a space
    /// and its mark.
    pub fn width(&self) -> usize {
        let mark = self
            .mark
            .as_ref()
            .map_or(0, |mark| mark.chars().count() + 1);
        self.key.chars().count() + 1 + self.label.chars().count() + mark
    }
}

/// The lock's mark: the word of the PIN's cost (ADR 0010).
pub(crate) fn lock_mark() -> String {
    text::get(id!("cost-pin"))
}

/// The toolbar of `context` in the key map `map` for a screen `width`
/// columns wide: at most
/// [`TOOLBAR`] actions that fit whole, as `Enter Boot`, the most important
/// first; then `? More`, which opens help with every key, wherever help can
/// be opened. Only the intents `offered` are shown, each labelled as `said`
/// says, and marked with the lock where the locked menu keeps what it
/// does, as `kept` says.
pub fn footer(
    map: Map,
    context: Context,
    width: usize,
    offered: &dyn Fn(Intent) -> bool,
    said: &dyn Fn(Intent) -> Id,
    kept: &dyn Fn(Intent) -> bool,
) -> Vec<Tool> {
    let all = bindings(map, context, offered, said);
    let more = all
        .iter()
        .find(|(intent, ..)| *intent == Intent::Help)
        .map(|(_, key, _)| Tool {
            key: key.clone(),
            label: text::title(id!("action-more")),
            is_more: true,
            mark: None,
        });
    let mut used = more.as_ref().map_or(0, |more| more.width() + TOOLBAR_GAP);
    let mut shown = Vec::new();
    let mut mark = None;
    for (intent, key, what) in all
        .into_iter()
        .filter(|(intent, ..)| *intent != Intent::Help)
    {
        let tool = Tool {
            key,
            label: text::title(what),
            is_more: false,
            mark: kept(intent).then(|| mark.get_or_insert_with(lock_mark).clone()),
        };
        let gap = if shown.is_empty() { 0 } else { TOOLBAR_GAP };
        if shown.len() == TOOLBAR || used + gap + tool.width() > width {
            break;
        }
        used += gap + tool.width();
        shown.push(tool);
    }
    shown.extend(more.filter(|more| more.width() <= width));
    shown
}

/// The name of a key, as the help view shows it.
pub fn key_name(code: Key, modifiers: Modifiers) -> String {
    let name = match code {
        Key::Enter => text!("key-enter"),
        Key::Escape => text!("key-esc"),
        Key::Tab => text!("key-tab"),
        Key::BackTab => text!("key-shift-tab"),
        Key::Up => text!("key-up"),
        Key::Down => text!("key-down"),
        Key::Left => text!("key-left"),
        Key::Right => text!("key-right"),
        Key::PageUp => text!("key-page-up"),
        Key::PageDown => text!("key-page-down"),
        Key::Home => text!("key-home"),
        Key::End => text!("key-end"),
        Key::Delete => text!("key-delete"),
        Key::Backspace => text!("key-backspace"),
        Key::F(number) => text!("key-function", number = number),
        Key::Char(' ') => text!("key-space"),
        Key::Char(c) => c.to_string(),
        other => format!("{other:?}"),
    };
    if modifiers.contains(Modifiers::CTRL) {
        text!("key-ctrl", key = name.to_uppercase())
    } else {
        name
    }
}

/// Every binding of `context` in the key map `map` for the help view, the
/// keys of one intent together: (keys, what they do, as `said` says it).
/// Only the intents `offered` are listed.
pub fn help(
    map: Map,
    context: Context,
    offered: &dyn Fn(Intent) -> bool,
    said: &dyn Fn(Intent) -> Id,
) -> Vec<(String, Id)> {
    keys_by(map, context, &|intent| offered(intent).then_some(intent))
        .into_iter()
        .map(|(intent, keys)| (keys, said(intent)))
        .collect()
}

/// Every key of `context` in the key map `map`, those shown only in help
/// too, the keys of what `does` says their intents do together, as the
/// help view names them: (what they do, `Enter, b`). The intents `does`
/// maps to nothing are left out.
pub(crate) fn keys_by<T: PartialEq>(
    map: Map,
    context: Context,
    does: &dyn Fn(Intent) -> Option<T>,
) -> Vec<(T, String)> {
    let mut keys: Vec<(T, Vec<String>)> = Vec::new();
    let bound = map.table().into_iter();
    for binding in bound.filter(|binding| map.is_bound(binding, context)) {
        let Some(what) = does(binding.intent) else {
            continue;
        };
        let name = key_name(binding.key, binding.modifiers);
        match keys.iter_mut().find(|(known, _)| *known == what) {
            Some((_, names)) if names.contains(&name) => {}
            Some((_, names)) => names.push(name),
            None => keys.push((what, vec![name])),
        }
    }
    keys.into_iter()
        .map(|(what, names)| (what, names.join(", ")))
        .collect()
}
