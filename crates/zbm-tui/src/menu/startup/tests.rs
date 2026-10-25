use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use zbm_types::Answer;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::world::{Method, Question, Stage, StageState};

/// Keys typed in advance (`None`: a tick passes without one), what was
/// done to the terminal's mode, and the terminal's size, when known.
#[derive(Clone, Default)]
struct Scripted {
    keys: Rc<RefCell<VecDeque<Option<KeyEvent>>>>,
    modes: Rc<RefCell<Vec<&'static str>>>,
    size: Rc<Cell<Option<(u16, u16)>>>,
}

impl Scripted {
    fn typed(&self, text: &str) {
        let mut keys = self.keys.borrow_mut();
        keys.extend(text.chars().map(|c| Some(key(KeyCode::Char(c)))));
    }

    fn press(&self, code: KeyCode) {
        self.keys.borrow_mut().push_back(Some(key(code)));
    }

    fn ticks(&self, count: usize) {
        self.keys
            .borrow_mut()
            .extend(std::iter::repeat_n(None, count));
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

impl Keys for Scripted {
    fn key(&mut self, _timeout: Duration) -> io::Result<Option<KeyEvent>> {
        Ok(self.keys.borrow_mut().pop_front().flatten())
    }

    fn release(&mut self) -> io::Result<()> {
        self.modes.borrow_mut().push("release");
        Ok(())
    }

    fn take(&mut self) -> io::Result<()> {
        self.modes.borrow_mut().push("take");
        Ok(())
    }

    fn size(&mut self) -> Option<(u16, u16)> {
        self.size.get()
    }
}

impl Resize for TestBackend {
    fn set_size(&mut self, size: Size) {
        self.resize(size.width, size.height);
    }
}

/// The stages of start-up, all pending.
fn snapshot() -> Snapshot {
    let stage = |name: &str| Stage {
        name: name.to_owned(),
        state: StageState::Pending,
    };
    Snapshot {
        startup: vec![stage("Unlocking LUKS volumes"), stage("Importing pools")],
        ..Snapshot::default()
    }
}

fn view(keys: &Scripted) -> StartupView<TestBackend, Scripted> {
    let terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    let theme = Theme::new(Capabilities::FULL);
    StartupView::new(terminal, keys.clone(), snapshot(), theme).unwrap()
}

/// What the terminal of `view` shows.
fn shown(view: &StartupView<TestBackend, Scripted>) -> String {
    let buffer = view.terminal().backend().buffer();
    let area = buffer.area;
    let mut text = String::new();
    for y in 0..area.height {
        let line: String = (0..area.width).map(|x| buffer[(x, y)].symbol()).collect();
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

fn unlock_requested(wait: Option<u64>) -> Event {
    Event::UnlockRequested {
        volume: "cryptroot".into(),
        method: if wait.is_some() {
            Method::Fido2
        } else {
            Method::Passphrase
        },
        tries_left: None,
        notice: None,
        wait,
    }
}

#[test]
fn the_stages_show_as_they_run_and_the_main_view_never_does() {
    let keys = Scripted::default();
    let mut view = view(&keys);
    assert!(shown(&view).contains("STARTING ZFSBOOTMENU"));

    view.send(Event::StageStarted("Unlocking LUKS volumes".into()))
        .unwrap();
    assert_eq!(view.model().snapshot.startup[0].state, StageState::Running);
    for stage in ["Unlocking LUKS volumes", "Importing pools"] {
        let done = Event::StageFinished {
            stage: stage.into(),
            state: StageState::Done,
        };
        view.send(done).unwrap();
    }
    assert!(view.model().started);
    assert!(shown(&view).contains("STARTING ZFSBOOTMENU"));
}

#[test]
fn a_secret_is_typed_into_the_dialog_and_comes_back_with_enter() {
    let keys = Scripted::default();
    keys.typed("hunter2");
    keys.press(KeyCode::Enter);
    let mut view = view(&keys);

    let answer = view.ask(&unlock_requested(None), &mut || None).unwrap();

    let Action::Unlock { volume, secret, .. } = answer else {
        panic!("not an unlock: {answer:?}");
    };
    assert_eq!(volume, "cryptroot");
    assert_eq!(secret.unwrap().expose(), b"hunter2");
    assert!(view.model().dialog.is_none());
}

#[test]
fn escape_skips_and_the_time_running_out_goes_on() {
    let keys = Scripted::default();
    keys.press(KeyCode::Esc);
    let mut view = view(&keys);
    let answer = view.ask(&unlock_requested(None), &mut || None).unwrap();
    assert_eq!(answer, Action::SkipUnlock("cryptroot".into()));

    keys.ticks(5);
    let asked = Event::Asked {
        question: Question::MissingDevices,
        seconds: Some(1),
    };
    let answer = view.ask(&asked, &mut || None).unwrap();
    assert_eq!(answer, Action::Answer(Answer::Continue));
    assert!(keys.keys.borrow().is_empty());
}

#[test]
fn a_check_ends_the_dialog_and_a_dialog_closed_without_answer_comes_back() {
    let keys = Scripted::default();
    keys.ticks(3);
    let mut view = view(&keys);
    let mut checks = 0;
    let answer = view
        .ask(&unlock_requested(Some(10)), &mut || {
            checks += 1;
            (checks == 2).then_some(Action::Answer(Answer::Continue))
        })
        .unwrap();
    assert_eq!(answer, Action::Answer(Answer::Continue));
    assert!(view.model().dialog.is_none());

    // An event that opens no dialog is shown again until the check
    // answers.
    keys.ticks(3);
    let mut checks = 0;
    let event = Event::StageStarted("Importing pools".into());
    let answer = view
        .ask(&event, &mut || {
            checks += 1;
            (checks == 3).then_some(Action::Answer(Answer::Escape))
        })
        .unwrap();
    assert_eq!(answer, Action::Answer(Answer::Escape));
}

#[test]
fn the_terminal_is_handed_over_and_given_back() {
    let keys = Scripted::default();
    let mut view = view(&keys);

    let mut ran = 0;
    view.suspend(&mut || ran += 1).unwrap();
    assert_eq!(ran, 1);
    assert!(shown(&view).contains("STARTING ZFSBOOTMENU"));
    let screen: &mut dyn StartupScreen = &mut view;
    screen
        .send(Event::Progress("Unlocking /dev/sda2 ...".into()))
        .unwrap();
    assert_eq!(screen.snapshot().startup.len(), 2);
    screen.suspend(&mut || ran += 1).unwrap();
    let answer = screen.ask(&unlock_requested(None), &mut || {
        Some(Action::Answer(Answer::Escape))
    });
    assert_eq!(answer.unwrap(), Action::Answer(Answer::Escape));
    Box::new(view).close().unwrap();
    assert_eq!(ran, 2);
    assert_eq!(
        *keys.modes.borrow(),
        ["release", "take", "release", "take", "release"]
    );
}

/// The width and height of what the terminal of `view` shows, and the
/// width of the start-up box's top line.
fn drawn_size(view: &StartupView<TestBackend, Scripted>) -> (u16, u16, usize) {
    let area = view.terminal().backend().buffer().area;
    let text = shown(view);
    let top = text
        .lines()
        .find(|line| line.contains("STARTING ZFSBOOTMENU"))
        .map_or(0, |line| line.chars().count());
    (area.width, area.height, top)
}

#[test]
fn the_view_follows_the_terminal_after_a_hand_over_and_as_time_passes() {
    let keys = Scripted::default();
    let mut view = view(&keys);
    assert_eq!(drawn_size(&view), (80, 25, 80));

    // A hook made the console font smaller: more columns and rows.
    keys.size.set(Some((100, 30)));
    view.suspend(&mut || {}).unwrap();
    assert_eq!(drawn_size(&view), (100, 30, 100));

    // The window shrank while a dialog waits.
    keys.ticks(2);
    let mut ticks = 0;
    let answer = view
        .ask(&unlock_requested(Some(10)), &mut || {
            ticks += 1;
            if ticks == 1 {
                keys.size.set(Some((60, 20)));
            }
            (ticks == 2).then_some(Action::Answer(Answer::Continue))
        })
        .unwrap();
    assert_eq!(answer, Action::Answer(Answer::Continue));
    assert_eq!(drawn_size(&view), (60, 20, 60));

    // A size the terminal cannot tell changes nothing.
    keys.size.set(Some((0, 0)));
    view.send(Event::Progress("Importing pools".into()))
        .unwrap();
    assert_eq!(drawn_size(&view), (60, 20, 60));
}
