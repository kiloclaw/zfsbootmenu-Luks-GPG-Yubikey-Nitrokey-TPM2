//! The review pages of a rollback, a destroy, a rewind and a forced import
//! with the names the input gives, with their field and without it, as a
//! dry run shows them; and the dry runs the palette opens.

use zbm_display::keys::{Key, KeyPress};
use zbm_tui::menu::model::{Dialog, Model, Then};
use zbm_tui::menu::update::follow;
use zbm_tui::menu::world::{
    Access, Action, Event, ImportOptions, Keep, Loss, Outcome, Plan, PoolHost, PoolUse, Review,
    Safer, Step, StepKind, Stop,
};
use zbm_tui::menu::{Message, update};
use zbm_types::{DatasetName, ImportPolicy, PoolName, SnapshotName, Untrusted};

use super::check_page;

/// What a rollback to `target` loses, with the names and sizes the input
/// gives: every kind of loss, a filesystem the input names among them.
fn hostile_losses(
    next: &mut impl FnMut() -> Untrusted<String>,
    target: &SnapshotName,
    length: u64,
) -> Vec<Loss> {
    let made = format!("{}/{}", target.pool(), next().get());
    vec![
        Loss::Snapshot {
            name: target.clone(),
            used: Some(u64::MAX),
        },
        Loss::Bookmark {
            dataset: target.dataset().clone(),
            name: next().into_inner(),
        },
        Loss::Written {
            since: target.clone(),
            bytes: Some(length),
        },
        Loss::Newer(target.clone()),
        Loss::SinceCheckpoint {
            created: Some(next()),
            space: Some(length),
            discarding: true,
            datasets: Some(made.parse::<DatasetName>().into_iter().collect()),
            snapshots: None,
        },
    ]
}

/// What stops a change of `target`: the clone, the holds, one without its
/// tag, and the host the input names, then more holds than a page has
/// rows.
fn hostile_stops(
    next: &mut impl FnMut() -> Untrusted<String>,
    target: &SnapshotName,
    host: &PoolHost,
) -> Vec<Stop> {
    let mut stops = vec![
        Stop::Clone {
            snapshot: target.clone(),
            clone: next(),
        },
        Stop::Hold {
            snapshot: target.clone(),
            tag: Some(next()),
        },
        Stop::Hold {
            snapshot: target.clone(),
            tag: None,
        },
        Stop::Active {
            pool: target.pool(),
            host: Some(host.clone()),
        },
    ];
    let tag = next();
    stops.extend((0..40).map(|_| Stop::Hold {
        snapshot: target.clone(),
        tag: Some(tag.clone()),
    }));
    stops
}

/// The steps of `action` on `target`, their commands the input gives.
fn hostile_steps(
    next: &mut impl FnMut() -> Untrusted<String>,
    action: &Action,
    target: &SnapshotName,
) -> Vec<Step> {
    let mut steps = vec![Step {
        kind: StepKind::Rollback(target.clone()),
        command: Some(next().into_inner()),
    }];
    if let Action::Destroy(snapshots) = action {
        steps.push(Step {
            kind: StepKind::Destroy(snapshots.clone()),
            command: Some(next().into_inner()),
        });
    }
    if let Action::Import(pool, options) = action {
        steps.push(Step {
            kind: StepKind::Import {
                pool: pool.clone(),
                options: options.clone(),
            },
            command: Some(next().into_inner()),
        });
    }
    steps.push(Step {
        kind: StepKind::Export(target.pool()),
        command: None,
    });
    steps
}

/// The safer action of `action`: a clone of `target` for a rollback, the
/// read-only import for a rewind, none for a forced import or a destroy.
fn safer(action: &Action, target: &SnapshotName) -> Option<Safer> {
    match action {
        Action::Import(pool, options) if options.rewind => {
            let read_only = ImportOptions {
                access: Access::ReadOnly,
                policy: ImportPolicy::Strict,
                rewind: false,
                id: None,
            };
            Some(Safer::Import(Box::new(Action::Import(
                pool.clone(),
                read_only,
            ))))
        }
        Action::Import(..) | Action::Destroy(_) => None,
        _ => Some(Safer::Clone(target.clone())),
    }
}

/// A plan of `action` on `target` with the names the input gives: what it
/// loses, its steps, who else used the pool, this host for a rewind; with
/// `refused`, what stops it and a failure.
fn hostile_plan(
    next: &mut impl FnMut() -> Untrusted<String>,
    action: &Action,
    target: &SnapshotName,
    refused: bool,
) -> Plan {
    let host = PoolHost {
        hostid: next().get().parse().ok(),
        hostname: Some(next()),
    };
    let length = u64::try_from(next().get().len()).unwrap_or(u64::MAX);
    let outcome = if refused {
        Outcome::Failed {
            what: next().into_inner(),
            why: vec![next().into_inner()],
            next: Vec::new(),
        }
    } else {
        Outcome::Done(String::new())
    };
    let this_host = matches!(action, Action::Import(_, options) if options.rewind);
    let review = Review {
        loses: hostile_losses(next, target, length),
        freed: Some(u64::MAX),
        keeps: vec![Keep::Snapshots(usize::MAX), Keep::Bookmarks(1)],
        undo: Some(Box::new(Action::Export(target.pool()))),
        host_id_stays: refused,
        safer: safer(action, target),
        stops: if refused {
            hostile_stops(next, target, &host)
        } else {
            Vec::new()
        },
        pool_use: Some(PoolUse {
            host: Some(host),
            written: Some(length.saturating_mul(1 << 40)),
            this_host,
            active: refused,
        }),
    };
    Plan {
        steps: hostile_steps(next, action, target),
        outcome,
        review,
    }
}

/// The changes reviewed: a rollback of the focused environment to the
/// snapshot the input names, the destroy of it and of another it names,
/// and the rewind and the forced import of the pool it names; each with
/// the snapshot its plan names.
fn reviewed(
    model: &Model,
    next: &mut impl FnMut() -> Untrusted<String>,
) -> Option<(SnapshotName, Vec<Action>)> {
    let environment = model.focus.clone()?;
    let named = format!("{environment}@{}", next().get()).parse();
    let target = named
        .or_else(|_| format!("{environment}@target").parse::<SnapshotName>())
        .ok()?;
    let other = format!("{environment}@{}", next().get())
        .parse::<SnapshotName>()
        .unwrap_or_else(|_| target.clone());
    let pool = next()
        .get()
        .parse::<PoolName>()
        .unwrap_or_else(|_| target.pool());
    let import = |rewind, policy| {
        let options = ImportOptions {
            access: Access::ReadWrite,
            policy,
            rewind,
            id: None,
        };
        Action::Import(pool.clone(), options)
    };
    let actions = vec![
        Action::Rollback(target.clone()),
        Action::Destroy(vec![target.clone(), other]),
        import(true, ImportPolicy::Strict),
        import(false, ImportPolicy::Force),
    ];
    Some((target, actions))
}

/// The review pages of a rollback, a destroy, a rewind and a forced import
/// with the names the input gives: each while it is planned, then with a
/// plan the input names refused and not, typed in, scrolled, its focus
/// moved, drawn and said; then closed. Each again without its field, as a
/// dry run shows it, with the same plans, its focus moved.
pub fn review_pages(
    mut model: Model,
    next: &mut impl FnMut() -> Untrusted<String>,
    size: (u16, u16),
) -> Model {
    let Some((target, actions)) = reviewed(&model, next) else {
        return model;
    };
    for action in actions {
        let mut plans = Vec::new();
        for has_field in [true, false] {
            let _ = follow(&mut model, Then::Review(action.clone()));
            if !has_field && let Some(Dialog::Review(page)) = &mut model.dialog {
                page.typed = None;
                page.name.clear();
            }
            check_page(&model, size);
            for refused in [false, true] {
                if has_field {
                    plans.push(hostile_plan(next, &action, &target, refused));
                }
                let plan = plans.get(usize::from(refused)).cloned();
                let keys: &[Key] = if has_field {
                    &[Key::Char('x'), Key::PageDown, Key::Tab, Key::Tab]
                } else {
                    &[Key::Char('x'), Key::Tab]
                };
                model = planned(model, &action, plan, keys, size);
            }
            model = update(model, Message::Key(KeyPress::new(Key::Escape))).0;
        }
    }
    model
}

/// `model` once the machine sent `plan` of `action`, drawn and said before
/// each of `keys` is pressed.
fn planned(
    mut model: Model,
    action: &Action,
    plan: Option<Plan>,
    keys: &[Key],
    size: (u16, u16),
) -> Model {
    if let Some(plan) = plan {
        let planned = Event::Planned {
            action: Box::new(action.clone()),
            plan,
        };
        model = update(model, Message::Event(Box::new(planned))).0;
    }
    for key in keys {
        check_page(&model, size);
        model = update(model, Message::Key(KeyPress::new(*key))).0;
    }
    model
}

/// The dry run or review page open in `model`, if one is, as the palette
/// opened it: a plan of its change with the names the input gives,
/// scrolled, its focus moved, drawn and said; then closed. A refused
/// plan's page without the field is [`review_pages`]'.
pub fn dry_run(
    model: Model,
    next: &mut impl FnMut() -> Untrusted<String>,
    size: (u16, u16),
) -> Model {
    let Some(Dialog::Review(page)) = &model.dialog else {
        return model;
    };
    let action = page.action.clone();
    let Some(target) = target_of(&model, &action, next) else {
        return model;
    };
    let plan = hostile_plan(next, &action, &target, false);
    let model = planned(model, &action, Some(plan), &[Key::PageDown, Key::Tab], size);
    update(model, Message::Key(KeyPress::new(Key::Escape))).0
}

/// The snapshot a plan of `action` names: the one it rolls back to or
/// destroys first; else one the input names, of the focused environment
/// or the first.
fn target_of(
    model: &Model,
    action: &Action,
    next: &mut impl FnMut() -> Untrusted<String>,
) -> Option<SnapshotName> {
    match action {
        Action::Rollback(target) => return Some(target.clone()),
        Action::Destroy(targets) => return targets.first().cloned(),
        _ => {}
    }
    let first = model.snapshot.environments.first();
    let environment = model
        .focus
        .clone()
        .or_else(|| first.map(|e| e.name.clone()))?;
    let named = format!("{environment}@{}", next().get()).parse();
    named
        .or_else(|_| format!("{environment}@target").parse::<SnapshotName>())
        .ok()
}
