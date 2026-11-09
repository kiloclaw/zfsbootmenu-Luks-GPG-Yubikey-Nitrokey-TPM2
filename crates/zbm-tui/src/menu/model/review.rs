//! The review page of a change (ADR 0016, points 12 to 15): what it
//! loses, keeps and allows, as the machine plans it, then the target's
//! name typed. The workspace opens it where the classic map asks for the
//! name in an alert, and, without the field, as the dry run of a change
//! the command line chose.

use std::collections::BTreeSet;

use crate::menu::world::{Action, Loss, Offer, Outcome, Plan, Safer};
use crate::text::text;

/// What `Enter` presses on a review page: `Tab` and `Shift-Tab` move
/// among them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Focus {
    /// The field of the name, which `Enter` confirms as the verb does.
    #[default]
    Field,
    /// The safer action.
    Safer,
    /// The button of the change.
    Verb,
}

/// Whether a change being chosen shows its plan before it runs: one the
/// command line chose does, from its `Enter` while the dialogs it opened
/// are open; one a key of a view chose does not.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DryRun {
    /// The change runs once chosen.
    #[default]
    Off,
    /// The change, once chosen, shows its plan first.
    Pending,
}

/// A row of what a change loses, as its page lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lost {
    /// What, in words, the names in it printable.
    pub(crate) what: String,
    /// The space it holds, in bytes, when known.
    pub(crate) bytes: Option<u64>,
}

/// A change under review: the machine plans it, then its target's name is
/// typed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reviewing {
    /// The change, which `Enter` asks for once its name is typed.
    pub action: Action,
    /// Its plan, with what it loses, keeps and allows; `None` while the
    /// machine plans it.
    pub plan: Option<Plan>,
    /// What was typed; `None` for a dry run, a page without the field.
    pub typed: Option<String>,
    /// What has to be typed: the last component of the environment rolled
    /// back, as the classic map asks; the short name of the one snapshot
    /// destroyed, or the last component of the environment whose several
    /// snapshots are; the name of a pool imported; nothing for a dry run.
    pub name: String,
    /// How many rows the list of what is lost is scrolled down.
    pub scroll: usize,
    /// What `Enter` presses.
    pub(crate) focus: Focus,
}

impl Reviewing {
    /// The page of `action`, before its plan: `None` for an action that
    /// has no target to name.
    pub(crate) fn new(action: Action) -> Option<Reviewing> {
        let name = match &action {
            Action::Rollback(snapshot) => snapshot.dataset().last_component().to_owned(),
            Action::Destroy(snapshots) => match snapshots.as_slice() {
                [snapshot] => snapshot.name().to_owned(),
                _ => snapshots.first()?.dataset().last_component().to_owned(),
            },
            Action::Import(pool, _) => pool.to_string(),
            _ => return None,
        };
        Some(Reviewing {
            action,
            plan: None,
            typed: Some(String::new()),
            name,
            scroll: 0,
            focus: Focus::Field,
        })
    }

    /// The dry run of `action`, which the command line chose, before its
    /// plan: the page without the field (ADR 0016, point 12), whose
    /// `Enter` asks for the change once it is planned.
    pub(crate) fn dry_run(action: Action) -> Reviewing {
        Reviewing {
            action,
            plan: None,
            typed: None,
            name: String::new(),
            scroll: 0,
            focus: Focus::Verb,
        }
    }

    /// Whether the plan came and refuses the change: something stops it, as
    /// ZFS would refuse it, or a step would fail.
    pub(crate) fn is_refused(&self) -> bool {
        self.plan.as_ref().is_some_and(|plan| {
            !plan.review.stops.is_empty() || matches!(plan.outcome, Outcome::Failed { .. })
        })
    }

    /// Whether the page shows the field: it has one, and the plan does not
    /// refuse the change.
    pub(crate) fn has_field(&self) -> bool {
        self.typed.is_some() && !self.is_refused()
    }

    /// Whether `Enter` asks for the change: the plan came, refuses nothing,
    /// and the name was typed as it is, on a page with the field.
    pub(crate) fn is_ready(&self) -> bool {
        self.plan.is_some()
            && !self.is_refused()
            && self.typed.as_deref().is_none_or(|typed| typed == self.name)
    }

    /// The safer action, when the plan gives one the machine `offers`;
    /// none on a page that refuses the change, whose only button closes it.
    pub(crate) fn safer(&self, offers: &BTreeSet<Offer>) -> Option<&Safer> {
        if self.is_refused() {
            return None;
        }
        let safer = self.plan.as_ref()?.review.safer.as_ref()?;
        let kind = match safer {
            Safer::Clone(_) => Offer::Clone,
            Safer::Import(_) => Offer::Import,
        };
        offers.contains(&kind).then_some(safer)
    }

    /// The focus after `Tab`, forwards, or `Shift-Tab`: among the field,
    /// the safer action when there is one, and the verb.
    pub(crate) fn moved(&self, forwards: bool, has_safer: bool) -> Focus {
        let stops: &[Focus] = if has_safer {
            &[Focus::Field, Focus::Safer, Focus::Verb]
        } else {
            &[Focus::Field, Focus::Verb]
        };
        let at = stops
            .iter()
            .position(|focus| *focus == self.focus)
            .unwrap_or(0);
        let next = if forwards {
            (at + 1) % stops.len()
        } else {
            (at + stops.len() - 1) % stops.len()
        };
        stops.get(next).copied().unwrap_or_default()
    }

    /// The rows of what the plan says the change loses, in its order; none
    /// before the plan.
    pub(crate) fn lost(&self) -> Vec<Lost> {
        let loses = self.plan.as_ref().map(|plan| plan.review.loses.as_slice());
        loses.unwrap_or_default().iter().flat_map(rows).collect()
    }
}

/// The rows of `loss`: one, but for what a rewind loses, which lists the
/// filesystems and snapshots made since the checkpoint.
fn rows(loss: &Loss) -> Vec<Lost> {
    let row = |what: String, bytes: Option<u64>| Lost { what, bytes };
    match loss {
        Loss::Snapshot { name, used } => vec![row(name.name().to_owned(), *used)],
        Loss::Bookmark { name, .. } => {
            let name = zbm_types::Printable(name).to_string();
            vec![row(text!("review-lost-bookmark", bookmark = name), None)]
        }
        Loss::Written { since, bytes } => {
            let what = text!("review-lost-written", snapshot = since.name());
            vec![row(what, *bytes)]
        }
        Loss::Newer(snapshot) => {
            vec![row(
                text!("review-lost-newer", snapshot = snapshot.name()),
                None,
            )]
        }
        Loss::SinceCheckpoint {
            created,
            space,
            discarding,
            datasets,
            snapshots,
        } => {
            let what = created.as_ref().map_or_else(
                || text!("review-lost-checkpoint-unknown"),
                |created| {
                    let created = created.printable().to_string();
                    text!("review-lost-checkpoint", created = created)
                },
            );
            let mut rows = vec![row(what, *space)];
            if *discarding {
                rows.push(row(text!("review-lost-discarding"), None));
            }
            rows.extend(made_since(datasets.as_deref(), snapshots.as_deref()));
            rows
        }
    }
}

/// The rows of the filesystems and snapshots made since a checkpoint;
/// one that says they could not be listed for each list unknown.
fn made_since(
    datasets: Option<&[zbm_types::DatasetName]>,
    snapshots: Option<&[zbm_types::SnapshotName]>,
) -> Vec<Lost> {
    let row = |what: String| Lost { what, bytes: None };
    let mut rows = match datasets {
        Some(datasets) => datasets
            .iter()
            .map(|dataset| row(text!("review-lost-dataset", dataset = dataset.to_string())))
            .collect(),
        None => vec![row(text!("review-lost-datasets-unknown"))],
    };
    match snapshots {
        Some(snapshots) => rows.extend(snapshots.iter().map(|snapshot| {
            row(text!(
                "review-lost-snapshot",
                snapshot = snapshot.to_string()
            ))
        })),
        None => rows.push(row(text!("review-lost-snapshots-unknown"))),
    }
    rows
}
