//! The menu as words (ROADMAP.md UI8, ADR 0011): what the line view prints
//! and what `Announce` says, from the same parts and catalogue messages as
//! the views draw, as plain text: no glyph, no colour, no column.

use ratatui::text::Line;
use zbm_types::Printable;

use super::{health, main_view, question, screens, unlock};
use crate::menu::format::{age, size};
use crate::menu::keymap;
use crate::menu::model::{Asking, Column, Dialog, Model, Naming, RECOVERY, Row, Screen, Segment};
use crate::menu::world::{Level, Outcome, StageState};
use crate::text::{self, text};

/// A dialog in words: its title, what it says, and its choices, which the
/// line view numbers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Said {
    /// The title.
    pub(crate) title: String,
    /// What it says.
    pub(crate) lines: Vec<String>,
    /// The choices, in order; empty when it has none to number.
    pub(crate) choices: Vec<String>,
}

/// The text of `line`, its spans one after the other, without the blanks
/// at its end.
pub(crate) fn plain(line: &Line<'_>) -> String {
    let text: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    text.trim_end().to_owned()
}

/// The title of the current screen; in the workspace's Boot tab, then the
/// path of what its columns select.
pub(crate) fn title(model: &Model) -> String {
    let title = screen_title(model);
    let path = model.path();
    if model.map != keymap::Map::Workspace || path.is_empty() {
        return title;
    }
    text!("lines-path", title = title, path = path.join(" > "))
}

/// The title of the current screen alone.
fn screen_title(model: &Model) -> String {
    match model.screen {
        Screen::Startup => text!("view-startup"),
        Screen::Main => text!("view-main"),
        Screen::Snapshots => text!("view-snapshots"),
        Screen::Kernels => text!("view-kernels"),
        Screen::CommandLine => text!("view-command-line"),
        Screen::Pools => text!("view-pools"),
        Screen::Recovery => text!("view-recovery"),
        Screen::Log => text!("view-log"),
        Screen::Help(_) => text!("view-help"),
        Screen::Diff => text!("view-changes"),
    }
}

/// What the countdown says while it runs, in its longest form.
pub(crate) fn countdown(model: &Model) -> Option<String> {
    let (forms, _) = health::banner(model)?;
    forms.first().map(|form| form.trim().to_owned())
}

/// The notification line: an action running, the outcome of the last one,
/// or what start-up is doing.
pub(crate) fn notification(model: &Model) -> Option<String> {
    if let Some(ran) = model.working {
        return Some(text!("working", seconds = ran / 1000));
    }
    if let Some(outcome) = &model.notification {
        return Some(match outcome {
            Outcome::Done(what) => format!("{}: {what}", text!("outcome-done")),
            Outcome::Warning(what) => format!("{}: {what}", text!("outcome-warning")),
            Outcome::Failed { what, .. } => format!("{}: {what}", text!("outcome-failed")),
            Outcome::Cancelled => format!(
                "{}: {}",
                text!("outcome-cancelled"),
                text!("outcome-cancelled.what")
            ),
        });
    }
    model.progress.clone()
}

/// The rows of the current screen, in the order it shows them; whether
/// they are items to choose, which the line view numbers.
pub(crate) fn rows(model: &Model) -> (Vec<String>, bool) {
    match model.screen {
        Screen::Startup => (startup(model), false),
        Screen::Main => (environments(model), true),
        Screen::Snapshots => (snapshots(model), model.column() == Column::Loaded),
        Screen::Kernels => (kernels(model), true),
        Screen::CommandLine => {
            let editor = model.editor.as_ref();
            (
                editor
                    .map(|editor| editor.text.clone())
                    .into_iter()
                    .collect(),
                false,
            )
        }
        Screen::Pools => {
            let pools = model.pool_rows();
            let rows = in_order(&model.pools.matches, |index| pools.get(index).cloned());
            (rows, true)
        }
        Screen::Recovery => {
            let rows = in_order(&model.recovery.matches, |index| {
                RECOVERY.get(index).map(|(label, _)| text::title(*label))
            });
            (rows, true)
        }
        Screen::Log => (log(model), false),
        Screen::Help(context) => (help(model, context), false),
        Screen::Diff => (changes(model), true),
    }
}

/// The items `matches` points at, through `item`, in their order.
fn in_order(
    matches: &[crate::menu::filter::Ranked],
    item: impl Fn(usize) -> Option<String>,
) -> Vec<String> {
    matches
        .iter()
        .filter_map(|ranked| item(ranked.index))
        .collect()
}

/// The stages of start-up, each with how far it got.
fn startup(model: &Model) -> Vec<String> {
    model
        .snapshot
        .startup
        .iter()
        .map(|stage| match &stage.state {
            StageState::Pending => text!("lines-stage-pending", stage = stage.name.as_str()),
            StageState::Running => text!("lines-stage-running", stage = stage.name.as_str()),
            StageState::Done => text!("lines-stage-done", stage = stage.name.as_str()),
            StageState::Skipped => text!("lines-stage-skipped", stage = stage.name.as_str()),
            StageState::Failed(reason) => text!(
                "lines-stage-failed",
                stage = stage.name.as_str(),
                reason = reason.as_str()
            ),
        })
        .collect()
}

/// The boot environments: the name, then what the list says of it.
fn environments(model: &Model) -> Vec<String> {
    let environments = &model.snapshot.environments;
    in_order(&model.environments.matches, |index| {
        let environment = environments.get(index)?;
        let facts: String = main_view::facts(model, environment)
            .iter()
            .map(|span| span.content.trim())
            .filter(|fact| !fact.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("{}, {facts}", environment.name))
    })
}

/// The snapshots of the focused environment: name, age and size; while
/// they are not read, what their column says instead. In the workspace's
/// timeline, the header of each group comes before its snapshots, or
/// alone when it is folded.
fn snapshots(model: &Model) -> Vec<String> {
    if let Some((title, detail)) = screens::unread(model) {
        return vec![format!("{title}: {detail}")];
    }
    let snapshots = model.focus_snapshots();
    let said = |position: usize| {
        let ranked = model.snapshots.matches.get(position)?;
        let snapshot = snapshots.get(ranked.index)?;
        Some(format!(
            "{}, {}, {}",
            snapshot.name.name(),
            age(model.snapshot.now, snapshot.created),
            size(snapshot.used)
        ))
    };
    let Some(timeline) = model.timeline() else {
        return (0..model.snapshots.matches.len())
            .filter_map(said)
            .collect();
    };
    timeline
        .window(0, timeline.rows())
        .into_iter()
        .filter_map(|row| match row {
            Row::Header(group) => {
                let mut header = vec![group.period.label()];
                header.extend(group.facts());
                Some(header.join(", "))
            }
            Row::Snapshot(position) => said(position),
        })
        .collect()
}

/// The rows of [`rows`] that are headers the line view does not number,
/// as they are no item to choose: those of the workspace's timeline whose
/// group is not folded, in order.
pub(crate) fn headers(model: &Model) -> Vec<usize> {
    match model.timeline() {
        Some(timeline) if model.screen == Screen::Snapshots && model.column() == Column::Loaded => {
            timeline.headers()
        }
        _ => Vec::new(),
    }
}

/// The kernels of the focused environment, the default said.
fn kernels(model: &Model) -> Vec<String> {
    let Some(environment) = model.focused() else {
        return Vec::new();
    };
    in_order(&model.kernels.matches, |index| {
        let kernel = environment.kernels.get(index)?;
        let version = kernel.version.printable().to_string();
        Some(if kernel.is_default {
            format!("{version}, {}", text!("label-default"))
        } else {
            version
        })
    })
}

/// The log, oldest first, each entry with its level.
fn log(model: &Model) -> Vec<String> {
    // The workspace says its boot history first.
    let history = if model.map == keymap::Map::Workspace {
        super::history::rows(model)
    } else {
        Vec::new()
    };
    let saved = history.into_iter().map(|(_, row)| row);
    saved
        .chain(model.snapshot.log.entries.iter().map(|entry| {
            let level = match entry.level {
                Level::Info => text!("log-info"),
                Level::Warning => text!("log-warning"),
                Level::Error => text!("log-error"),
            };
            format!("{level}: {}", entry.text.printable())
        }))
        .collect()
}

/// Help: the keys of the view it was opened from; in the workspace, after
/// the version of the ZFS module running, as its header says.
fn help(model: &Model, context: keymap::Context) -> Vec<String> {
    let workspace = model.map == keymap::Map::Workspace;
    let version = model.snapshot.zfs_version.as_ref().filter(|_| workspace);
    let keys = keymap::help(model.map, context, &|intent| {
        keymap::is_shown(context, intent, model)
    });
    version
        .map(|version| {
            text!(
                "help-zfs-version",
                version = version.printable().to_string()
            )
        })
        .into_iter()
        .chain(
            keys.into_iter()
                .map(|(keys, what)| format!("{keys}: {}", text::title(what))),
        )
        .collect()
}

/// The changes of the differences view: files or packages, as shown.
fn changes(model: &Model) -> Vec<String> {
    let Some(diff) = &model.diff else {
        return Vec::new();
    };
    match diff.segment {
        Segment::Files => in_order(&diff.list.matches, |index| {
            diff.changes
                .get(index)
                .map(|change| change.path.printable().to_string())
        }),
        Segment::Packages => in_order(&diff.package_list.matches, |index| {
            diff.package_changes()
                .get(index)
                .map(|change| Printable(&change.name).to_string())
        }),
    }
}

/// The dialog over the screen, or the palette, in words.
pub(crate) fn dialog(model: &Model) -> Option<Said> {
    let Some(dialog) = &model.dialog else {
        return palette(model);
    };
    Some(match dialog {
        Dialog::Unlock(prompt) => Said {
            title: text!(
                "unlock-title",
                volume = Printable(&prompt.volume).to_string()
            ),
            lines: unlock::body(model, prompt)
                .iter()
                .map(plain)
                .filter(|line| !line.is_empty())
                .collect(),
            choices: Vec::new(),
        },
        Dialog::Question(asking) => asked(asking),
        Dialog::Choose {
            title,
            lines,
            choices,
            ..
        } => Said {
            title: title.clone(),
            lines: lines.clone(),
            choices: choices.iter().map(|choice| choice.label.clone()).collect(),
        },
        Dialog::Confirm {
            title, lines, name, ..
        } => Said {
            title: title.clone(),
            lines: [
                lines.clone(),
                vec![text!("confirm-type", name = name.as_str())],
            ]
            .concat(),
            choices: Vec::new(),
        },
        Dialog::Name(naming) => named(naming),
        Dialog::Error { what, why, next } => {
            let mut lines = why.clone();
            if !next.is_empty() {
                lines.push(text!("error-you-can"));
                lines.extend(next.iter().cloned());
            }
            Said {
                title: what.clone(),
                lines,
                choices: Vec::new(),
            }
        }
    })
}

/// The naming dialog in words: what it says, then the name typed so far.
fn named(naming: &Naming) -> Said {
    let typed = format!("{} {}{}", text!("name-label"), naming.prefix, naming.typed);
    Said {
        title: naming.title.clone(),
        lines: [naming.lines.clone(), vec![typed]].concat(),
        choices: Vec::new(),
    }
}

/// A question in words: what it asks, the seconds left when it counts
/// down, and what `ok` and `back` do.
fn asked(asking: &Asking) -> Said {
    let wording = question::wording(&asking.question);
    let mut lines = wording.lines;
    if let Some(left) = asking.left {
        lines.push((wording.countdown)(left.div_ceil(1000)));
    }
    lines.push(text!("lines-go", go = wording.go));
    if let Some(stop) = wording.stop {
        lines.push(text!("lines-stop", stop = stop));
    }
    Said {
        title: wording.title,
        lines,
        choices: Vec::new(),
    }
}

/// The command palette, when it is open: what it found.
fn palette(model: &Model) -> Option<Said> {
    let palette = model.palette.as_ref()?;
    let items = &palette.items;
    Some(Said {
        title: text!("palette-title"),
        lines: vec![palette.query.clone()],
        choices: palette
            .matches
            .iter()
            .filter_map(|ranked| items.get(ranked.index))
            .map(|item| item.label.clone())
            .collect(),
    })
}

/// What the person may type on the current screen: the keys of the key
/// map that are characters, with what they do.
pub(crate) fn commands(model: &Model) -> Vec<String> {
    let context = model.context();
    keymap::bindings(model.map, context, &|intent| {
        keymap::is_shown(context, intent, model)
    })
    .into_iter()
    .filter(|(_, key, _)| key.chars().count() == 1)
    .map(|(_, key, what)| format!("{key} {}", text::get(what)))
    .collect()
}

#[cfg(test)]
mod tests;
