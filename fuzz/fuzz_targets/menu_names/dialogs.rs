//! The dialogs: start-up's and the machine's, and the sheets of choices
//! with every choice followed; with the names the input gives.

use zbm_display::keys::Key;
use zbm_tui::menu::model::{Dialog, Model, Screen};
use zbm_tui::menu::update::follow;
use zbm_tui::menu::world::{
    Event, Method, Notice, Outcome, PcrChange, Pool, Question, SnapshotInfo, Tpm2Seal,
};
use zbm_types::{PoolName, Untrusted};

use super::{MAPS, check_page, machine, pools, press, sent};

/// What the machine asks and says, with the names and texts the input
/// gives: a secret, and a device waited for, each after another try;
/// start-up's questions; a pool that did not import; the re-seal of the
/// first volume's TPM2 key; what start-up does; how actions end.
fn asked(model: &Model, next: &mut impl FnMut() -> Untrusted<String>) -> Vec<Event> {
    let asked = |question| Event::Asked {
        question,
        seconds: Some(u64::MAX),
    };
    let first = model.snapshot.volumes.first();
    let volume = first.map_or_else(|| next().into_inner(), |v| v.name.get().clone());
    let pool = next()
        .parse::<PoolName>()
        .or_else(|_| "tank".parse::<PoolName>());
    let mut events = vec![
        Event::UnlockRequested {
            volume: next().into_inner(),
            method: Method::Passphrase,
            tries_left: Some(u32::MAX),
            notice: Some(Notice::Wrong(next().into_inner())),
            wait: None,
        },
        Event::UnlockRequested {
            volume: next().into_inner(),
            method: Method::KeyDevice,
            tries_left: None,
            notice: Some(Notice::Instead(next().into_inner())),
            wait: Some(u64::MAX),
        },
        asked(Question::RetryUnlock {
            volume: next().into_inner(),
        }),
        asked(Question::MissingVolume {
            device: next().into_inner(),
        }),
        asked(Question::MissingDevices),
        asked(Question::ImportFailed {
            pool: Some(next().into_inner()),
        }),
        asked(Question::ImportFailed { pool: None }),
        Event::Asked {
            question: Question::Unsupported {
                advice: next().into_inner(),
            },
            seconds: None,
        },
        Event::ResealOffered(volume),
        Event::Progress(next().into_inner()),
    ];
    if let Ok(pool) = pool {
        let reason = next().into_inner();
        events.push(Event::ImportProblem { pool, reason });
    }
    events.extend(ended(next));
    events
}

/// How actions end, as the input says: done, with a warning, failed with
/// its causes and the next steps, cancelled; then the menu leaves.
fn ended(next: &mut impl FnMut() -> Untrusted<String>) -> Vec<Event> {
    vec![
        Event::Finished(Outcome::Done(next().into_inner())),
        Event::Finished(Outcome::Warning(next().into_inner())),
        Event::Finished(Outcome::Failed {
            what: next().into_inner(),
            why: vec![next().into_inner(), next().into_inner()],
            next: vec![next().into_inner()],
        }),
        Event::Finished(Outcome::Cancelled),
        Event::Leaving(next().into_inner()),
    ]
}

/// The TPM2 key of a volume, sealed to PCRs 0 and 7, released no more:
/// the firmware changed as the event log the input writes says, and a PCR
/// of no known use changed.
fn changed_seal(next: &mut impl FnMut() -> Untrusted<String>) -> Tpm2Seal {
    Tpm2Seal {
        pcrs: vec![0, 7],
        changed: vec![
            PcrChange {
                index: 0,
                detail: Some(next().into_inner()),
            },
            PcrChange {
                index: u8::MAX,
                detail: None,
            },
        ],
    }
}

/// What the machine asks and says, each over the view in each key map:
/// start-up's while it runs, the others' over the Boot tab. The first
/// volume's TPM2 key no longer releases, as its row in the Pools view
/// says, and the machine offers to seal it again.
pub fn questions(mut model: Model, next: &mut impl FnMut() -> Untrusted<String>, size: (u16, u16)) {
    if let Some(volume) = model.snapshot.volumes.first_mut() {
        volume.tpm2 = Some(changed_seal(next));
    }
    let events = asked(&model, next);
    for map in MAPS {
        model.map = map;
        model.dialog = None;
        model.leaving = None;
        model.notification = None;
        model.refresh();
        model.screen = Screen::Pools;
        check_page(&model, size);
        for event in events.iter().cloned() {
            model.screen = match event {
                Event::UnlockRequested { .. } | Event::Asked { .. } => Screen::Startup,
                _ => Screen::Main,
            };
            model.dialog = None;
            model.leaving = None;
            model = sent(model, event);
            check_page(&model, size);
        }
    }
}

/// The sheets of choices in each key map, each choice followed: those of a
/// snapshot the input names, of each pool a scan found with the input's
/// names, one with a checkpoint and one forced to import, and of the
/// power; then the palette, with what the input types.
pub fn sheets(mut model: Model, next: &mut impl FnMut() -> Untrusted<String>, size: (u16, u16)) {
    let Some(name) = model.focus.clone() else {
        return;
    };
    let mut loaded = machine::taken(&name, model.snapshot.now, next);
    if let (Event::SnapshotsLoaded(list), Ok(named)) =
        (&mut loaded, format!("{name}@sheet").parse())
    {
        list.snapshots.push(SnapshotInfo {
            name: named,
            created: model.snapshot.now,
            used: u64::MAX,
        });
    }
    let scanned = Event::PoolsScanned(rewound(pools::hostile_pools(next), next));
    let typed = next();
    for map in MAPS {
        model.map = map;
        model.dialog = None;
        model.screen = Screen::Snapshots;
        model = sent(model, loaded.clone());
        model = press(model, Key::Home);
        for _ in 0..model.snapshots.matches.len() {
            if model.selected_snapshot().is_some() {
                break;
            }
            model = press(model, Key::Down);
        }
        model = chosen(press(model, Key::Enter), size);
        model.screen = Screen::Pools;
        model = sent(model, scanned.clone());
        model = press(model, Key::Home);
        for _ in 0..model.pools.matches.len() {
            for key in [Key::Enter, Key::Char('i')] {
                model = chosen(press(model, key), size);
            }
            model = press(model, Key::Down);
        }
        model = chosen(press(model, Key::Char('x')), size);
        model = palette(model, &typed, size);
    }
}

/// The pools of a scan, the first imported with a checkpoint to rewind to;
/// and a copy of the last, not imported, of a name the input gives, else
/// `other`, which is imported by force since no imported pool shadows it.
fn rewound(mut pools: Vec<Pool>, next: &mut impl FnMut() -> Untrusted<String>) -> Vec<Pool> {
    if let Some(first) = pools.first_mut() {
        first.checkpoint = Some(true);
    }
    let name = next()
        .parse::<PoolName>()
        .or_else(|_| "other".parse::<PoolName>());
    if let (Some(mut other), Ok(name)) = (pools.last().cloned(), name) {
        other.name = name;
        other.id = Some(u64::MAX - 1);
        pools.push(other);
    }
    pools
}

/// The sheet open in `model`, then each of its choices followed from the
/// view under it: a confirmation, a review page, a name asked for, which
/// is typed, and what it then asks; then the sheet is closed.
fn chosen(mut model: Model, size: (u16, u16)) -> Model {
    check_page(&model, size);
    let Some(Dialog::Choose { choices, .. }) = &model.dialog else {
        model.dialog = None;
        return model;
    };
    let choices = choices.clone();
    let (screen, focus) = (model.screen, model.focus.clone());
    for choice in choices {
        model.dialog = None;
        let _ = follow(&mut model, choice.then);
        check_page(&model, size);
        if matches!(model.dialog, Some(Dialog::Name(_))) {
            model = press(press(model, Key::Char('x')), Key::Enter);
            check_page(&model, size);
        }
        model.screen = screen;
        model.focus.clone_from(&focus);
    }
    model.dialog = None;
    model
}

/// The palette, its entries named by the machine's environments,
/// snapshots and pools, then filtered by what the input types; closed.
fn palette(model: Model, typed: &Untrusted<String>, size: (u16, u16)) -> Model {
    let mut model = press(model, Key::Char(':'));
    check_page(&model, size);
    for c in typed.get().chars().take(8) {
        model = press(model, Key::Char(c));
    }
    check_page(&model, size);
    model.palette = None;
    model
}
