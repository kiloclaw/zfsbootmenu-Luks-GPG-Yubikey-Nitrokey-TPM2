//! The dialogs: start-up's and the machine's, and the sheets of choices
//! with every choice followed; with the names the input gives.

use std::mem::discriminant;

use zbm_display::keys::{Key, KeyPress};
use zbm_tui::menu::keymap::Map;
use zbm_tui::menu::model::{Dialog, Model, PaletteItem, Screen, Then};
use zbm_tui::menu::update::{follow, palette_items};
use zbm_tui::menu::world::{
    CommandLine, Event, Method, Notice, Outcome, PcrChange, Pool, Question, SnapshotInfo, Tpm2Seal,
};
use zbm_tui::menu::{Message, update};
use zbm_types::{DatasetName, PoolName, Untrusted};

use super::{MAPS, check_page, machine, pools, press, reviews, sent};

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
/// power; then the palette, with what the input types, its history, its
/// entries followed into their dry runs, the save editor and the palette
/// locked.
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
        model = history(model, size);
        model = followed(model, next, size);
        model = save_editor(model, next, size);
        model = locked(model, &typed, next, size);
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
/// Its first boot of a clone is followed: the name it asks for.
fn palette(model: Model, typed: &Untrusted<String>, size: (u16, u16)) -> Model {
    let mut model = press(model, Key::Char(':'));
    check_page(&model, size);
    for c in typed.get().chars().take(8) {
        model = press(model, Key::Char(c));
    }
    check_page(&model, size);
    let items = model.palette.take().map(|palette| palette.items);
    let clone = items
        .into_iter()
        .flatten()
        .find(|item| matches!(item.then, Then::BootClone(_)));
    if let Some(item) = clone {
        let _ = follow(&mut model, item.then);
        check_page(&model, size);
        model.dialog = None;
    }
    model
}

/// `model` after `c` is pressed with `Ctrl`.
fn ctrl(model: Model, c: char) -> Model {
    let key = KeyPress::new(Key::Char(c)).with_ctrl();
    update(model, Message::Key(key)).0
}

/// The palette's history: the entries of each kind and section chosen
/// ([`firsts`]), the names in them the input's, of which it keeps the last
/// twenty, shown alone (`Ctrl-R`); the selected one's command
/// completed (`Tab`), its last word deleted (`Ctrl-W`), and the history
/// hidden again; closed.
fn history(mut model: Model, size: (u16, u16)) -> Model {
    for then in firsts(&palette_items(&model)) {
        model.recent.chose(&then);
    }
    let mut model = press(model, Key::Char(':'));
    check_page(&model, size);
    model = ctrl(model, 'r');
    check_page(&model, size);
    model = press(model, Key::Tab);
    check_page(&model, size);
    model = ctrl(model, 'w');
    check_page(&model, size);
    model = ctrl(model, 'r');
    check_page(&model, size);
    press(model, Key::Escape)
}

/// What the palette's entries do, of each kind once: the first entry of
/// each section, and of each kind of what an entry does, a change's kind
/// its action's.
fn firsts(items: &[PaletteItem]) -> Vec<Then> {
    let mut sections = Vec::new();
    let mut kinds = Vec::new();
    let mut firsts = Vec::new();
    for item in items {
        let action = match &item.then {
            Then::Act(action) | Then::Review(action) | Then::Confirm { action, .. } => {
                Some(discriminant(action))
            }
            _ => None,
        };
        let kind = (discriminant(&item.then), action);
        if !sections.contains(&item.section) || !kinds.contains(&kind) {
            sections.push(item.section);
            kinds.push(kind);
            firsts.push(item.then.clone());
        }
    }
    firsts
}

/// The palette's entries, of each kind once ([`firsts`]), each chosen with
/// its `Enter`, what it asks answered, and the dry run of a change, its
/// plan the input names; then the view as it was.
fn followed(
    mut model: Model,
    next: &mut impl FnMut() -> Untrusted<String>,
    size: (u16, u16),
) -> Model {
    let (screen, focus) = (model.screen, model.focus.clone());
    for then in firsts(&palette_items(&model)) {
        model = answered(press(selected(model, &then), Key::Enter), size);
        model = reviews::dry_run(model, next, size);
        model = restored(model, screen, focus.as_ref());
    }
    model
}

/// The palette opened, its entry that does `then` selected. That entry is
/// found: an entry followed leaves no palette, filter or dialog behind.
fn selected(model: Model, then: &Then) -> Model {
    let mut model = press(model, Key::Char(':'));
    let palette = model.palette.as_mut().expect("the palette opened");
    let found = palette.matches.iter().position(|ranked| {
        (palette.items.get(ranked.index)).is_some_and(|item| item.then == *then)
    });
    palette.selected = found.expect("the entry followed is in the palette");
    model
}

/// `model` with what an entry followed left closed: its dialog, palette,
/// editor and a filter being typed in any list; on `screen`, `focus`
/// focused.
fn restored(mut model: Model, screen: Screen, focus: Option<&DatasetName>) -> Model {
    model.dialog = None;
    model.palette = None;
    model.editor = None;
    for list in [
        &mut model.environments,
        &mut model.snapshots,
        &mut model.kernels,
        &mut model.pools,
        &mut model.recovery,
    ] {
        list.filtering = false;
    }
    model.screen = screen;
    model.focus = focus.cloned();
    model
}

/// What the dialog open in `model` asks, answered: a name, typed; a
/// confirmation, its name typed; the first choice; each drawn and said,
/// until a review page or none is open.
fn answered(mut model: Model, size: (u16, u16)) -> Model {
    check_page(&model, size);
    for _ in 0..3 {
        model = match &model.dialog {
            Some(Dialog::Name(_)) => press(press(model, Key::Char('x')), Key::Enter),
            Some(Dialog::Confirm { name, .. }) => {
                let name = name.clone();
                let typed = name
                    .chars()
                    .fold(model, |model, c| press(model, Key::Char(c)));
                press(typed, Key::Enter)
            }
            Some(Dialog::Choose { .. }) => press(model, Key::Enter),
            _ => return model,
        };
        check_page(&model, size);
    }
    model
}

/// The workspace's save editor of the focused environment, which starts
/// from the machine's command line, the input's: `Enter` opens its dry
/// run, the plan the input names. Then lines the machine would refuse,
/// each drawn and refused by `Enter`: one with control characters, and the
/// input's text without them, repeated to more bytes than ZFS keeps of a
/// property; then the view as it was.
fn save_editor(
    mut model: Model,
    next: &mut impl FnMut() -> Untrusted<String>,
    size: (u16, u16),
) -> Model {
    let Some(environment) = model.focus.clone().filter(|_| model.map == Map::Workspace) else {
        return model;
    };
    let screen = model.screen;
    let _ = follow(&mut model, Then::SaveCommandLine(environment.clone()));
    check_page(&model, size);
    model = press(model, Key::Enter);
    check_page(&model, size);
    model = reviews::dry_run(model, next, size);
    model.dialog = None;
    let typed = next().into_inner();
    let unit: String = typed
        .chars()
        .filter(|c| !c.is_control())
        .chain(" x".chars())
        .collect();
    let long = unit.repeat(CommandLine::LONGEST / unit.len() + 1);
    for line in [format!("{typed}\u{1b}[2J\u{7}\t{typed}"), long] {
        if let Some(editor) = &mut model.editor {
            editor.text = line;
        }
        check_page(&model, size);
        model = press(model, Key::Enter);
        check_page(&model, size);
        model.dialog = None;
    }
    model.editor = None;
    model.screen = screen;
    model.focus = Some(environment);
    model
}

/// The palette of the locked menu, what the lock keeps marked, filtered by
/// what the input types; then the entries of each kind ([`firsts`]) chosen
/// until one opens a dry run, which is drawn, its button marked; then the
/// menu as it was, unlocked.
fn locked(
    mut model: Model,
    typed: &Untrusted<String>,
    next: &mut impl FnMut() -> Untrusted<String>,
    size: (u16, u16),
) -> Model {
    let (screen, focus) = (model.screen, model.focus.clone());
    model.snapshot.menu_locked = true;
    check_page(&model, size);
    model = press(model, Key::Char(':'));
    check_page(&model, size);
    for c in typed.get().chars().take(8) {
        model = press(model, Key::Char(c));
    }
    check_page(&model, size);
    model = restored(model, screen, focus.as_ref());
    for then in firsts(&palette_items(&model)) {
        model = answered(press(selected(model, &then), Key::Enter), size);
        let reviewed = matches!(model.dialog, Some(Dialog::Review(_)));
        model = restored(reviews::dry_run(model, next, size), screen, focus.as_ref());
        if reviewed {
            break;
        }
    }
    model.snapshot.menu_locked = false;
    model
}
