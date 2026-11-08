//! The review pages of a rollback, a destroy, a rewind and a forced import
//! with the names the input gives.

use zbm_display::keys::{Key, KeyPress};
use zbm_tui::menu::model::{Model, Then};
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
/// moved, drawn and said; then closed.
pub fn review_pages(
    mut model: Model,
    next: &mut impl FnMut() -> Untrusted<String>,
    size: (u16, u16),
) -> Model {
    let Some((target, actions)) = reviewed(&model, next) else {
        return model;
    };
    for action in actions {
        let _ = follow(&mut model, Then::Review(action.clone()));
        check_page(&model, size);
        for refused in [false, true] {
            let planned = Event::Planned {
                action: Box::new(action.clone()),
                plan: hostile_plan(next, &action, &target, refused),
            };
            model = update(model, Message::Event(Box::new(planned))).0;
            for key in [Key::Char('x'), Key::PageDown, Key::Tab, Key::Tab] {
                check_page(&model, size);
                model = update(model, Message::Key(KeyPress::new(key))).0;
            }
        }
        model = update(model, Message::Key(KeyPress::new(Key::Escape))).0;
    }
    model
}
