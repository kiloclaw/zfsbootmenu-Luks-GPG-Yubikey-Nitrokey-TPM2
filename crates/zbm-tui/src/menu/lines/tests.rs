use pretty_assertions::assert_eq;
use zbm_app::{App, Runtime};
use zbm_testkit::fakes::{FakeClock, FakeConsole, FakeSystem};

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::DemoOptions;
use crate::menu::machine::Countdown;
use crate::menu::screen::tests::NoShell;
use crate::menu::theme::Theme;
use crate::menu::world::Action;

/// The demo machine's classic menu (`zbm.menu=classic`), started.
fn started() -> Session<crate::menu::demo::FakeMachine> {
    started_with(&DemoOptions {
        classic: true,
        ..DemoOptions::default()
    })
}

/// The demo machine's menu with `options`, started.
fn started_with(options: &DemoOptions) -> Session<crate::menu::demo::FakeMachine> {
    let mut session = Session::new(options, Theme::new(Capabilities::ASCII));
    session.start();
    session
}

/// The tab of the workspace's menu after `words` are typed, one line each.
fn tab_after(words: &[&str]) -> Tab {
    let mut session = started_with(&DemoOptions {
        classic: false,
        ..DemoOptions::default()
    });
    for word in words {
        let Typed::Keys(keys) = typed(word, Context::Main, Map::Workspace) else {
            panic!("{word} is not a word");
        };
        for key in keys {
            session.press(key);
        }
    }
    session.model().tab()
}

fn key(code: Key) -> KeyPress {
    KeyPress::new(code)
}

#[test]
fn a_number_chooses_an_item_and_a_word_or_a_key_does_what_it_says() {
    let down = key(Key::Down);
    assert_eq!(
        typed("3", Context::Main, Map::Classic),
        Typed::Keys(vec![key(Key::Home), down, down, key(Key::Enter)])
    );
    assert_eq!(
        typed(" 2  k ", Context::Main, Map::Classic),
        Typed::Keys(vec![key(Key::Home), down, key(Key::Char('k'))])
    );
    assert_eq!(
        typed("k", Context::Main, Map::Classic),
        Typed::Keys(vec![key(Key::Char('k'))])
    );
    assert_eq!(
        typed("ok", Context::Main, Map::Classic),
        Typed::Keys(vec![key(Key::Enter)])
    );
    assert_eq!(
        typed("back", Context::Main, Map::Classic),
        Typed::Keys(vec![key(Key::Escape)])
    );
    assert_eq!(
        typed("help", Context::Main, Map::Classic),
        Typed::Keys(vec![key(Key::Char('?'))])
    );
    assert_eq!(typed("", Context::Main, Map::Classic), Typed::Again);
    assert_eq!(
        typed("0", Context::Main, Map::Classic),
        Typed::Unknown("0".to_owned())
    );
    assert_eq!(
        typed("boot it now", Context::Main, Map::Classic),
        Typed::Unknown("boot it now".to_owned())
    );
    assert_eq!(
        typed("2 back x", Context::Main, Map::Classic),
        Typed::Unknown("2 back x".to_owned())
    );
}

#[test]
fn in_the_workspace_boot_opens_the_boot_tab() {
    assert_eq!(tab_after(&["log", "boot"]), Tab::Boot);
}

#[test]
fn in_the_workspace_snapshots_opens_the_snapshots_tab() {
    assert_eq!(tab_after(&["snapshots"]), Tab::Snapshots);
}

#[test]
fn in_the_workspace_pools_opens_the_pools_tab() {
    assert_eq!(tab_after(&["pools"]), Tab::Pools);
}

#[test]
fn in_the_workspace_log_opens_the_log_tab() {
    assert_eq!(tab_after(&["log"]), Tab::Log);
    assert_eq!(
        typed("log", Context::Main, Map::Workspace),
        Typed::Keys(vec![key(Key::Char('4'))])
    );
}

#[test]
fn the_line_view_takes_its_words_in_any_case() {
    assert_eq!(
        typed("OK", Context::Main, Map::Classic),
        Typed::Keys(vec![key(Key::Enter)])
    );
    assert_eq!(
        typed("Back", Context::Main, Map::Workspace),
        Typed::Keys(vec![key(Key::Escape)])
    );
    assert_eq!(tab_after(&["POOLS"]), Tab::Pools);
    // A capital goes to the key map as typed, which takes it as its letter.
    assert_eq!(
        typed("B", Context::Main, Map::Workspace),
        Typed::Keys(vec![key(Key::Char('B'))])
    );
}

#[test]
fn the_classic_map_keeps_the_digits_and_has_no_tab_words() {
    let down = key(Key::Down);
    for map in [Map::Classic, Map::Workspace] {
        assert_eq!(
            typed("2", Context::Main, map),
            Typed::Keys(vec![key(Key::Home), down, key(Key::Enter)])
        );
    }
    for word in ["boot", "snapshots", "pools", "log"] {
        assert_eq!(
            typed(word, Context::Main, Map::Classic),
            Typed::Unknown(word.to_owned())
        );
    }
}

#[test]
fn the_workspace_s_page_names_its_own_keys_and_words() {
    let session = started_with(&DemoOptions {
        classic: false,
        ..DemoOptions::default()
    });
    let page = page(session.model());

    assert!(page.contains(&text!("lines-number-workspace")), "{page:#?}");
    assert!(page.contains(&text!("lines-words-workspace")), "{page:#?}");
    assert!(!page.contains(&text!("lines-number")), "{page:#?}");
    assert!(!page.contains(&text!("lines-words")), "{page:#?}");
}

#[test]
fn the_line_view_takes_english_words_in_another_language() {
    use crate::text::{Language, in_language};
    let key = |word: &str, context| match typed(word, context, Map::Workspace) {
        Typed::Keys(keys) => keys.first().map(|key| key.key),
        _ => None,
    };
    in_language(Language::Lookalike, || {
        for word in ["back", "BACK", "ьасκ"] {
            assert_eq!(key(word, Context::Choose), Some(Key::Escape), "{word}");
        }
        assert_eq!(key("ok", Context::Choose), Some(Key::Enter));
        assert_eq!(key("help", Context::Main), Some(Key::Char('?')));
        assert_eq!(key("рооӏѕ", Context::Main), key("pools", Context::Main));
        assert_eq!(key("pools", Context::Main), Some(Key::Char('3')));
        assert_eq!(key("nothing", Context::Main), None);
    });
}

#[test]
fn tab_words_are_unknown_where_no_tab_can_be_chosen() {
    for word in ["boot", "snapshots", "pools", "log"] {
        assert_eq!(
            typed(word, Context::Choose, Map::Workspace),
            Typed::Unknown(word.to_owned())
        );
    }
}

#[test]
fn the_workspace_s_page_offers_tab_words_only_where_they_choose_a_tab() {
    let machine = crate::menu::demo::FakeMachine::new(&DemoOptions::default());
    let snapshot = machine.snapshot().clone();
    let mut model = Model::new(snapshot, Theme::new(Capabilities::ASCII));
    model.map = Map::Workspace;
    let offers_tabs = |model: &Model| page(model).contains(&text!("lines-words-workspace"));

    // Start-up, the Boot tab, binds no digit: a digit there waits for a
    // passphrase's dialog.
    assert_eq!(model.context(), Context::Startup);
    assert!(!offers_tabs(&model), "{:#?}", page(&model));
    model.screen = crate::menu::model::Screen::Main;
    assert!(offers_tabs(&model), "{:#?}", page(&model));
    model.dialog = Some(crate::menu::model::Dialog::Error {
        what: "It failed".to_owned(),
        why: Vec::new(),
        next: Vec::new(),
    });
    assert_eq!(model.context(), Context::Choose);
    assert!(!offers_tabs(&model), "{:#?}", page(&model));
    assert!(page(&model).contains(&text!("lines-words")));
}

#[test]
fn in_a_field_the_line_is_the_text_and_a_blank_one_goes_back() {
    let Typed::Keys(keys) = typed("pw 1\n", Context::Text, Map::Classic) else {
        panic!("keys");
    };
    assert_eq!(keys[0], key(Key::Char('u')).with_ctrl());
    let typed_text: String = keys[1..keys.len() - 1]
        .iter()
        .filter_map(|press| match press.key {
            Key::Char(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(typed_text, "pw 1");
    assert_eq!(keys.last(), Some(&key(Key::Enter)));
    assert_eq!(
        typed("", Context::Text, Map::Classic),
        Typed::Keys(vec![key(Key::Escape)])
    );
}

#[test]
fn the_page_numbers_the_environments_and_says_what_can_be_typed() {
    let session = started();
    let page = page(session.model());

    assert_eq!(page[0], "Boot Environments");
    assert!(page[1].starts_with("  1) zroot/ROOT/void, "), "{page:#?}");
    assert!(
        page.iter().any(|line| line.starts_with("Keys: ")),
        "{page:#?}"
    );
    assert!(page.contains(&text!("lines-number")), "{page:#?}");
    assert!(page.contains(&text!("lines-words")), "{page:#?}");
}

#[test]
fn typed_lines_boot_the_second_environment_and_the_countdown_is_said() {
    let clock = FakeClock::new();
    let console = FakeConsole::new().with_clock(clock.clone());
    console.timeout().secret(b"2");
    let mut session = started();
    let mut front = LineFront::new(&console, &clock);

    let why = front.run(&mut session).unwrap();

    assert!(why.starts_with("Booting tank/ROOT/arch"), "{why}");
    assert!(
        matches!(session.actions.as_slice(), [Action::Boot { environment, .. }]
            if environment.as_str() == "tank/ROOT/arch"),
        "{:?}",
        session.actions
    );
    let shown = console.output_text();
    assert!(shown.contains("  2) tank/ROOT/arch"), "{shown}");
    assert!(shown.contains("zroot/ROOT/void in"), "{shown}");
    assert!(shown.ends_with(&format!("{why}\n")), "{shown}");
}

#[test]
fn the_workspace_s_page_numbers_the_environments_and_a_number_boots_one() {
    let clock = FakeClock::new();
    let console = FakeConsole::new().with_clock(clock.clone());
    let mut session = started_with(&DemoOptions::default());
    let listed = page(session.model());
    let arch = listed
        .iter()
        .find_map(|line| {
            let (number, rest) = line.trim_start().split_once(") ")?;
            rest.starts_with("tank/ROOT/arch")
                .then(|| number.to_owned())
        })
        .expect("tank/ROOT/arch is numbered");
    console.timeout().secret(arch.as_bytes());
    let mut front = LineFront::new(&console, &clock);

    let why = front.run(&mut session).unwrap();

    // The pool headers are not numbered: the environments are, in the
    // order the Boot tab shows them.
    assert_eq!(session.model().map, Map::Workspace);
    assert!(
        listed[1].starts_with("  1) zroot/ROOT/void, "),
        "{listed:#?}"
    );
    assert_eq!(arch, "3", "{listed:#?}");
    assert!(why.starts_with("Booting tank/ROOT/arch"), "{why}");
    assert!(
        listed.contains(&text!("lines-number-workspace")),
        "{listed:#?}"
    );
}

#[test]
fn a_page_is_printed_again_only_when_it_changed_or_is_asked_for() {
    let clock = FakeClock::new();
    let console = FakeConsole::new().with_clock(clock.clone());
    // Stop the countdown, ask for the page, type nonsense, then the kernels
    // of the first, and boot its second kernel.
    console
        .secret(b"back")
        .secret(b"")
        .secret(b"what now")
        .secret(b"1 k")
        .secret(b"2");
    let mut session = started();
    let mut front = LineFront::new(&console, &clock);

    let why = front.run(&mut session).unwrap();

    let shown = console.output_text();
    assert_eq!(shown.matches("Boot Environments\n").count(), 2, "{shown}");
    assert!(shown.contains(&text!("lines-countdown-stopped")), "{shown}");
    assert!(
        shown.contains(&text!("lines-unknown", typed = "what now")),
        "{shown}"
    );
    assert!(shown.contains("Kernels\n  1) 6.12.9_1"), "{shown}");
    assert!(why.starts_with("Booting zroot/ROOT/void"), "{why}");
    assert!(
        matches!(session.actions.as_slice(), [Action::Boot { kernel: Some(kernel), .. }]
            if kernel.get() == "6.12.8_1"),
        "{:?}",
        session.actions
    );
}

#[test]
fn the_input_s_end_ends_the_line_view() {
    let clock = FakeClock::new();
    let console = FakeConsole::new().with_clock(clock.clone());
    let mut session = started();
    let mut front = LineFront::new(&console, &clock);

    let ended = front.run(&mut session).unwrap_err();

    assert_eq!(ended.kind(), io::ErrorKind::UnexpectedEof);
}

#[test]
fn a_snapshot_s_actions_are_numbered_and_a_clone_is_named_by_a_line() {
    let clock = FakeClock::new();
    let console = FakeConsole::new().with_clock(clock.clone());
    // Stop the countdown, the snapshots of the first, the first snapshot,
    // its third action (clone), a name, then power off.
    for line in [
        "back",
        "1 s",
        "1",
        "3",
        "zbmtest-clone",
        "back",
        "back",
        "x",
        "2",
    ] {
        console.secret(line.as_bytes());
    }
    let mut session = started();
    let mut front = LineFront::new(&console, &clock);

    let _ = front.run(&mut session);

    let shown = console.output_text();
    assert!(shown.contains("  3) Clone"), "{shown}");
    assert!(shown.contains(&text!("lines-text")), "{shown}");
    assert!(
        session.actions.iter().any(|action| matches!(action,
            Action::Clone(_, target) if target.as_str().ends_with("/zbmtest-clone"))),
        "{:?}\n{shown}",
        session.actions
    );
}

#[test]
fn a_pin_is_typed_with_echo_off_and_never_printed() {
    let clock = FakeClock::new();
    let console = FakeConsole::new().with_clock(clock.clone());
    // The demo asks for the TPM2 PIN of its first volume: a wrong one, then
    // the right one.
    console.secret(b"9999").secret(crate::menu::demo::PIN);
    let mut session = Session::new(
        &DemoOptions {
            locked: true,
            ..DemoOptions::default()
        },
        Theme::new(Capabilities::ASCII),
    );
    session.start();
    let mut front = LineFront::new(&console, &clock);
    assert!(
        is_asking_secret(session.model()),
        "{:?}",
        session.model().dialog
    );

    let _ = front.run(&mut session);

    let shown = console.output_text();
    assert!(shown.contains(&text!("lines-text")), "{shown}");
    assert!(
        !shown.contains("9999") && !shown.contains("1234"),
        "{shown}"
    );
    assert!(shown.contains("Wrong PIN."), "{shown}");
    assert_eq!(console.echo_changes()[..4], [false, true, false, true]);
    let still_asks = matches!(&session.model().dialog,
        Some(crate::menu::model::Dialog::Unlock(prompt)) if prompt.volume == "luks-zroot-a");
    assert!(!still_asks, "{:?}", session.model().dialog);
}

#[test]
fn the_lines_show_where_asked_or_where_a_screen_reader_is_likely() {
    assert!(is_wanted(Some(true), true, false));
    assert!(!is_wanted(Some(false), false, true));
    assert!(is_wanted(None, false, false));
    assert!(is_wanted(None, true, true));
    assert!(!is_wanted(None, true, false));
}

#[test]
fn the_line_screen_s_menu_reads_keys_with_its_map_from_one_round_to_the_next() {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    let clock = FakeClock::new();
    let console = FakeConsole::new().with_clock(clock.clone());
    let theme = Theme::new(Capabilities::ASCII);
    let mut screen = LineScreen::new(&console, &clock, theme, Map::Workspace);

    assert_eq!(round(&fake, &mut screen), io::ErrorKind::UnexpectedEof);
    // The next round begins on the same model, its map kept.
    assert_eq!(round(&fake, &mut screen), io::ErrorKind::UnexpectedEof);
    assert_eq!(screen.model.map(|model| model.map), Some(Map::Workspace));
}

/// One round of the menu on `screen`, on the machine `fake`; it ends at
/// once, as the console's input did: how.
fn round(fake: &FakeSystem, screen: &mut LineScreen<'_>) -> io::ErrorKind {
    let stop = || false;
    let runtime = Runtime::default();
    let app = App::new(zbm_platform::Context::new(fake.system()), &runtime);
    let machine = RealMachine::new(&app, fake.system(), &NoShell, &stop, Countdown::Skip).unwrap();
    screen.run(machine).unwrap_err().kind()
}

#[test]
fn what_the_machine_says_is_printed_escaped() {
    use crate::menu::model::{Dialog, Editor, Screen};
    use crate::menu::world::{Event, Question, Stage, StageState};

    let mut model = started().into_model();
    let environment = model.selected_environment().unwrap().name.clone();
    model.progress = Some("Unlocking /dev/sda2\u{1b}]0;x\u{7}".to_owned());
    model.screen = Screen::Startup;
    model.snapshot.startup = vec![Stage {
        name: "Import \u{202e}tank".to_owned(),
        state: StageState::Failed("cannot open\u{9b}31m".to_owned()),
    }];
    let startup = page(&model);
    model.screen = Screen::CommandLine;
    model.editor = Some(Editor {
        environment,
        kernel: None,
        text: "quiet\tsplash\u{2066}".to_owned(),
    });
    let editor = page(&model);
    model.dialog = Some(Dialog::Error {
        what: "Booting\u{1b}[2J failed".to_owned(),
        why: vec!["no \u{200f}kernel".to_owned()],
        next: vec!["run\u{7}".to_owned()],
    });
    let error = page(&model);
    let asked = Event::Asked {
        question: Question::RetryUnlock {
            volume: "luks\u{7}a".to_owned(),
        },
        seconds: None,
    };
    let (model, _) = crate::menu::update(model, crate::menu::Message::Event(Box::new(asked)));
    let question = page(&model);

    for (page, escaped) in [
        (&startup, "Unlocking /dev/sda2\\x1b]0;x\\x07"),
        (
            &startup,
            "Import \\u{202e}tank: failed, cannot open\\x9b31m",
        ),
        (&editor, "quiet\\x09splash\\u{2066}"),
        (&error, "Booting\\x1b[2J failed"),
        (&error, "no \\u{200f}kernel"),
        (&error, "run\\x07"),
        (&question, "luks\\x07a"),
    ] {
        assert!(
            page.iter().any(|line| line.contains(escaped)),
            "{escaped} not in {page:#?}"
        );
        let raw = |line: &String| line.contains(zbm_types::must_escape);
        assert!(!page.iter().any(raw), "{page:#?}");
    }
}
