//! Tests of the review page as drawn: the name to type shows whole at the
//! smallest size; what stops a change leaves room for what it loses, and
//! says how many more stops there are; the lines below what is lost wrap
//! where there is room; the button `Enter` presses shows it; a page that
//! refuses its change offers no safer action; the hint line leaves the
//! page; names from the machine are drawn printable; a destroy keeps every
//! bookmark of its environment, and names one snapshot whole; the space
//! freed is said of one snapshot or of several.

use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};
use zbm_display::keys::Key;
use zbm_types::{ImportPolicy, PoolName, SnapshotName, Untrusted};

use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::model::{Dialog, Model, Reviewing};
use crate::menu::theme::{Role, Theme};
use crate::menu::view::tests::screen;
use crate::menu::view::{view, words};
use crate::menu::world::{
    Access, Action, ImportOptions, Keep, Loss, Outcome, Plan, PoolHost, PoolUse, Review, Safer,
    Stop,
};
use crate::text::text;

pub(super) fn theme() -> Theme {
    Theme::new(Capabilities::FULL)
}

/// The demo under the workspace, started, on the Snapshots tab, with the
/// review page of the rollback to the daily snapshot of 20 October open,
/// as the demo plans it.
pub(super) fn rollback_page() -> Session {
    let mut session = Session::new(&DemoOptions::default(), theme());
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char('2'));
    for _ in 0..3 {
        session.key(Key::Down);
    }
    session.key(Key::Char('r'));
    session
}

/// The page of `action` over the demo's Snapshots tab, with `plan`.
fn reviewed(action: Action, plan: Plan) -> Model {
    let mut model = rollback_page().into_model();
    let mut reviewing = Reviewing::new(action).unwrap();
    reviewing.plan = Some(plan);
    model.dialog = Some(Dialog::Review(Box::new(reviewing)));
    model
}

/// A plan that ends well with `review`.
pub(super) fn plan(review: Review) -> Plan {
    Plan {
        steps: Vec::new(),
        outcome: Outcome::Done(String::new()),
        review,
    }
}

/// The forced import of `pool`, read-only.
fn forced(pool: PoolName) -> Action {
    let options = ImportOptions {
        access: Access::ReadOnly,
        policy: ImportPolicy::Force,
        rewind: false,
        id: None,
    };
    Action::Import(pool, options)
}

/// The text of the page drawn at `width` by `height`, its lines without
/// the borders, joined by a space: a sentence wrapped reads whole.
pub(super) fn prose(model: &Model, width: u16, height: u16) -> String {
    let drawn = screen(model, width, height);
    let lines: Vec<&str> = drawn
        .lines()
        .map(|line| line.trim_matches(['│', ' ']))
        .filter(|line| !line.is_empty())
        .collect();
    lines.join(" ")
}

/// `model` drawn at `width` by `height`.
pub(super) fn buffer(model: &Model, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| view(model, frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The style of the button `label`, as drawn at 80 by 24.
pub(super) fn button(model: &Model, label: &str) -> Style {
    let buffer = buffer(model, 80, 24);
    let drawn = format!("[ {label} ]");
    for y in 0..24 {
        let line: String = (0..80)
            .map(|x| buffer[(x, y)].symbol().to_owned())
            .collect();
        if let Some(at) = line.find(&drawn) {
            let x = u16::try_from(line[..at].chars().count()).unwrap();
            return buffer[(x, y)].style();
        }
    }
    panic!("no button {label}");
}

#[test]
fn a_long_name_to_type_shows_whole_at_the_smallest_size() {
    let name = "backup-of-the-old-server-in-the-rack-downstairs";
    assert_eq!(name.len(), 47);
    let model = reviewed(forced(name.parse().unwrap()), plan(Review::default()));

    let drawn = screen(&model, 52, 20);

    assert!(drawn.contains(name), "{drawn}");
}

#[test]
fn many_stops_leave_room_for_what_is_lost_and_say_how_many_more() {
    let snapshot: SnapshotName = "zroot/ROOT/void@x".parse().unwrap();
    let stops = (0..30)
        .map(|_| Stop::Hold {
            snapshot: snapshot.clone(),
            tag: Some(Untrusted::new("zrepl".to_owned())),
        })
        .collect();
    let lost = Loss::Snapshot {
        name: snapshot.clone(),
        used: Some(1024),
    };
    let review = Review {
        loses: vec![lost],
        stops,
        ..Review::default()
    };
    let model = reviewed(Action::Rollback(snapshot), plan(review));

    let drawn = screen(&model, 80, 24);

    assert!(drawn.contains(&text!("review-lost")), "{drawn}");
    let more = drawn
        .lines()
        .map(|line| line.trim_matches(['│', ' ']))
        .find(|line| line.starts_with("and ") && line.ends_with(" more"));
    assert!(more.is_some(), "{drawn}");
}

#[test]
fn the_lines_below_what_is_lost_wrap_where_there_is_room() {
    let session = rollback_page();
    let safer = text!("review-safer-clone", snapshot = "autosnap_2026-10-20_daily");

    let drawn = prose(session.model(), 80, 24);

    assert!(drawn.contains(&safer), "{drawn}");
}

#[test]
fn the_button_enter_presses_shows_it() {
    let theme = theme();
    let mut session = rollback_page();
    let verb = text!("button-roll-back");
    let clone = format!("{}…", text!("review-button-clone"));
    let style = |session: &Session, label: &str| button(session.model(), label);
    let reversed = |style: Style| style.add_modifier.contains(Modifier::REVERSED);
    // The change is disabled until the name is typed, and shows the focus.
    assert!(!reversed(style(&session, &verb)));
    session.key(Key::Tab);
    assert_eq!(style(&session, &clone).bg, theme.style(Role::Selected).bg);
    session.key(Key::Tab);
    assert!(reversed(style(&session, &verb)));
    assert_eq!(style(&session, &verb).fg, theme.style(Role::Muted).fg);

    // Once named, `Enter` presses the change from the field, or the safer
    // action when it has the focus.
    session.type_text("void");
    assert!(reversed(style(&session, &verb)));
    assert_eq!(style(&session, &verb).fg, theme.style(Role::Danger).fg);
    session.key(Key::Tab);
    assert!(!reversed(style(&session, &verb)));
    assert_eq!(style(&session, &clone).bg, theme.style(Role::Selected).bg);
}

#[test]
fn a_page_that_refuses_its_change_offers_no_safer_action() {
    let snapshot: SnapshotName = "zroot/ROOT/void@x".parse().unwrap();
    let review = Review {
        safer: Some(Safer::Clone(snapshot.clone())),
        stops: vec![Stop::Hold {
            snapshot: snapshot.clone(),
            tag: None,
        }],
        ..Review::default()
    };
    let model = reviewed(Action::Rollback(snapshot), plan(review));
    let Some(Dialog::Review(reviewing)) = &model.dialog else {
        panic!("{:?}", model.dialog);
    };

    assert_eq!(reviewing.safer(&model.snapshot.offers), None);
    let safer = text!("review-safer-clone", snapshot = "x");
    let said = words::dialog(&model).unwrap();
    assert!(!said.lines.contains(&safer), "{said:?}");
    assert!(!prose(&model, 120, 40).contains(&safer));
}

#[test]
fn the_hint_line_leaves_a_review_page() {
    let hint = text!("workspace-hint");
    let session = rollback_page();
    assert!(!screen(session.model(), 80, 25).contains(hint.trim()));

    let mut session = session;
    session.key(Key::Escape);
    assert!(screen(session.model(), 80, 25).contains(hint.trim()));
}

#[test]
fn hostile_names_on_a_page_are_drawn_printable() {
    let hostile = || Untrusted::new("\u{1b}[2J\u{7}evil".to_owned());
    let snapshot: SnapshotName = "zroot/ROOT/void@x".parse().unwrap();
    let pool: PoolName = "backup".parse().unwrap();
    let host = PoolHost {
        hostid: None,
        hostname: Some(hostile()),
    };
    let review = Review {
        stops: vec![
            Stop::Clone {
                snapshot: snapshot.clone(),
                clone: hostile(),
            },
            Stop::Hold {
                snapshot: snapshot.clone(),
                tag: Some(hostile()),
            },
            Stop::Hold {
                snapshot,
                tag: None,
            },
            Stop::Active {
                pool: pool.clone(),
                host: Some(host.clone()),
            },
        ],
        pool_use: Some(PoolUse {
            host: Some(host),
            written: Some(0),
            this_host: false,
            active: true,
        }),
        ..Review::default()
    };
    let failed = Outcome::Failed {
        what: "\u{1b}]0;title\u{7}".to_owned(),
        why: Vec::new(),
        next: Vec::new(),
    };
    let plan = Plan {
        steps: Vec::new(),
        outcome: failed,
        review,
    };
    let model = reviewed(forced(pool), plan);
    for (width, height) in [(120, 40), (52, 20)] {
        let drawn = screen(&model, width, height);
        assert!(!drawn.contains(['\u{1b}', '\u{7}']), "{drawn}");
    }
    let said = words::dialog(&model).unwrap();
    assert!(said.lines.iter().all(|line| !line.contains('\u{1b}')));
    assert!(said.lines.contains(&text!("review-active")), "{said:?}");
}

#[test]
fn a_destroy_keeps_every_bookmark_and_names_one_snapshot_whole() {
    let snapshot: SnapshotName = "zroot/ROOT/void@old".parse().unwrap();
    let review = Review {
        loses: vec![Loss::Snapshot {
            name: snapshot.clone(),
            used: Some(4096),
        }],
        freed: Some(4096),
        keeps: vec![Keep::Bookmarks(2)],
        ..Review::default()
    };

    let model = reviewed(Action::Destroy(vec![snapshot]), plan(review));

    let drawn = prose(&model, 120, 40);
    assert!(drawn.contains("Destroy zroot/ROOT/void@old"), "{drawn}");
    let kept = text!("review-kept-every-bookmark", count = 2);
    assert!(
        drawn.contains(&text!("review-kept", kept = kept)),
        "{drawn}"
    );
    assert!(
        drawn.contains(&format!("[ {} ]", text!("button-destroy"))),
        "{drawn}"
    );
    assert!(drawn.contains(&text!("review-no-undo")), "{drawn}");
    // One snapshot: the freed line says so, and its short name is typed.
    assert!(
        drawn.contains("Destroying the snapshot frees 4.0 KiB."),
        "{drawn}"
    );
    let Some(Dialog::Review(reviewing)) = &model.dialog else {
        panic!("no review page");
    };
    assert_eq!(reviewing.name, "old");
}

#[test]
fn the_space_freed_by_several_snapshots_is_said_of_them_all() {
    let snapshot = |name: &str| -> SnapshotName { name.parse().unwrap() };
    let (old, older) = (
        snapshot("zroot/ROOT/void@old"),
        snapshot("zroot/ROOT/void@older"),
    );
    let lost = |name: &SnapshotName| Loss::Snapshot {
        name: name.clone(),
        used: Some(4096),
    };
    let review = Review {
        loses: vec![lost(&older), lost(&old)],
        freed: Some(8192),
        ..Review::default()
    };

    let model = reviewed(Action::Destroy(vec![older, old]), plan(review));

    let drawn = prose(&model, 120, 40);
    assert!(
        drawn.contains("Destroying the snapshots frees 8.0 KiB."),
        "{drawn}"
    );
    assert!(
        drawn.contains("Destroy 2 Snapshots of zroot/ROOT/void"),
        "{drawn}"
    );
    let Some(Dialog::Review(reviewing)) = &model.dialog else {
        panic!("no review page");
    };
    assert_eq!(reviewing.name, "void");
}
