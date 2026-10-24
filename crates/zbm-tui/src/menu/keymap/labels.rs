//! What keys say: the toolbar, the help view and the names of keys, from
//! the catalogue.

use ratatui::crossterm::event::{KeyCode, KeyModifiers};

use super::{Context, Footer, Intent, KEYMAP};
use crate::text::{self, Id, id, text};
use Context::{Diff, Kernels, Main, Packages, Pools, Recovery, Snapshots, Startup};

/// What a key does for an intent in a context, as the toolbar and the
/// help view say it: a message of the catalogue with a `.title`.
pub fn describe(context: Context, intent: Intent) -> Id {
    match (context, intent) {
        (Main | Kernels, Intent::Primary) | (_, Intent::Boot) => id!("action-boot"),
        (Snapshots | Context::Choose, Intent::Primary) => id!("action-choose"),
        (Pools, Intent::Primary) => id!("action-details"),
        (Diff, Intent::Primary) => id!("action-restore"),
        (Packages, Intent::Primary) => id!("action-show-files"),
        (Recovery, Intent::Primary) => id!("action-run"),
        (Startup | Context::Text, Intent::Primary) => id!("action-confirm"),
        (Context::Choose | Context::Text, Intent::Back) => id!("action-cancel"),
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
        (_, Intent::Clone) => id!("action-clone"),
        (_, Intent::CloneAndPromote) => id!("action-clone-promote"),
        (_, Intent::Duplicate) => id!("action-duplicate"),
        (_, Intent::NewSnapshot) => id!("action-snapshot"),
        (_, Intent::Diff) => id!("action-diff"),
        (_, Intent::Mark) => id!("action-mark"),
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
    }
}

/// The bindings of `context` shown in the toolbar, in order: (intent, key,
/// what it does).
pub fn bindings(context: Context) -> Vec<(Intent, String, Id)> {
    let idle = matches!(context, Context::Startup | Context::Help | Context::Log);
    KEYMAP
        .iter()
        .filter(|binding| binding.scope.contains(context))
        .filter(|binding| !(idle && binding.intent == Intent::Primary))
        .filter_map(|binding| {
            let key = match binding.footer {
                Footer::Hidden => return None,
                Footer::Key => key_name(binding.code, binding.modifiers),
                Footer::Keys(id) => text::get(id),
            };
            Some((binding.intent, key, describe(context, binding.intent)))
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
}

impl Tool {
    /// The columns it takes: its key, a space and its label.
    pub fn width(&self) -> usize {
        self.key.chars().count() + 1 + self.label.chars().count()
    }
}

/// The toolbar of `context` for a screen `width` columns wide: at most
/// [`TOOLBAR`] actions that fit whole, as `Enter Boot`, the most important
/// first; then `? More`, which opens help with every key, wherever help can
/// be opened.
pub fn footer(context: Context, width: usize) -> Vec<Tool> {
    let all = bindings(context);
    let more = all
        .iter()
        .find(|(intent, ..)| *intent == Intent::Help)
        .map(|(_, key, _)| Tool {
            key: key.clone(),
            label: text::title(id!("action-more")),
            is_more: true,
        });
    let mut used = more.as_ref().map_or(0, |more| more.width() + TOOLBAR_GAP);
    let mut shown = Vec::new();
    for (_, key, what) in all
        .into_iter()
        .filter(|(intent, ..)| *intent != Intent::Help)
    {
        let tool = Tool {
            key,
            label: text::title(what),
            is_more: false,
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
pub fn key_name(code: KeyCode, modifiers: KeyModifiers) -> String {
    let name = match code {
        KeyCode::Enter => text!("key-enter"),
        KeyCode::Esc => text!("key-esc"),
        KeyCode::Tab => text!("key-tab"),
        KeyCode::Up => text!("key-up"),
        KeyCode::Down => text!("key-down"),
        KeyCode::Left => text!("key-left"),
        KeyCode::Right => text!("key-right"),
        KeyCode::PageUp => text!("key-page-up"),
        KeyCode::PageDown => text!("key-page-down"),
        KeyCode::Home => text!("key-home"),
        KeyCode::End => text!("key-end"),
        KeyCode::F(number) => text!("key-function", number = number),
        KeyCode::Char(' ') => text!("key-space"),
        KeyCode::Char(c) => c.to_string(),
        other => format!("{other:?}"),
    };
    if modifiers.contains(KeyModifiers::CONTROL) {
        text!("key-ctrl", key = name.to_uppercase())
    } else {
        name
    }
}

/// Every binding of `context` for the help view, the keys of one intent
/// together: (keys, what they do).
pub fn help(context: Context) -> Vec<(String, Id)> {
    let mut lines: Vec<(Intent, Vec<String>)> = Vec::new();
    for binding in KEYMAP
        .iter()
        .filter(|binding| binding.scope.contains(context))
    {
        let name = key_name(binding.code, binding.modifiers);
        match lines
            .iter_mut()
            .find(|(intent, _)| *intent == binding.intent)
        {
            Some((_, keys)) => keys.push(name),
            None => lines.push((binding.intent, vec![name])),
        }
    }
    lines
        .into_iter()
        .map(|(intent, keys)| (keys.join(", "), describe(context, intent)))
        .collect()
}
