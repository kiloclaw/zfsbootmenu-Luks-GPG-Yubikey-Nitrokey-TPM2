use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use pretty_assertions::assert_eq;
use zbm_config::args::ZbmArgs;
use zbm_testkit::fakes::{FakeClock, FakeSystem};

use super::*;

mod prompts;

/// How the screen answers the next question.
enum Reply {
    /// With this action.
    With(Action),
    /// With this action, after this many seconds.
    Late(u64, Action),
    /// With what the dialog's check gives, a second passing between checks.
    Check,
    /// The terminal fails.
    Fail,
}

/// What the screen was shown and did.
#[derive(Default)]
struct Log {
    sent: Vec<Event>,
    asked: Vec<Event>,
    replies: VecDeque<Reply>,
    suspended: usize,
    closed: bool,
}

/// A start-up screen that answers from a script.
struct Screen {
    snapshot: Snapshot,
    log: Rc<RefCell<Log>>,
    clock: FakeClock,
}

impl StartupScreen for Screen {
    fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    fn send(&mut self, event: Event) -> io::Result<()> {
        if let Event::Changed(snapshot) = &event {
            self.snapshot = snapshot.clone();
        }
        self.log.borrow_mut().sent.push(event);
        Ok(())
    }

    fn ask(
        &mut self,
        event: &Event,
        check: &mut dyn FnMut() -> Option<Action>,
    ) -> io::Result<Action> {
        let reply = {
            let mut log = self.log.borrow_mut();
            log.asked.push(event.clone());
            log.replies.pop_front()
        };
        match reply.unwrap_or_else(|| panic!("nothing answers {event:?}")) {
            Reply::With(action) => Ok(action),
            Reply::Late(seconds, action) => {
                self.clock.advance(Duration::from_secs(seconds));
                Ok(action)
            }
            Reply::Check => {
                for _ in 0..100 {
                    if let Some(action) = check() {
                        return Ok(action);
                    }
                    self.clock.advance(Duration::from_secs(1));
                }
                panic!("the check never answered {event:?}");
            }
            Reply::Fail => Err(io::Error::other("the terminal is gone")),
        }
    }

    fn suspend(&mut self, run: &mut dyn FnMut()) -> io::Result<()> {
        self.log.borrow_mut().suspended += 1;
        run();
        Ok(())
    }

    fn close(self: Box<Self>) -> io::Result<()> {
        self.log.borrow_mut().closed = true;
        Ok(())
    }
}

/// What a test sees of the screen.
#[derive(Clone)]
struct Seen(Rc<RefCell<Log>>);

impl Seen {
    /// Answers the next question with `reply`.
    fn reply(&self, reply: Reply) -> &Seen {
        self.0.borrow_mut().replies.push_back(reply);
        self
    }

    /// The last question asked.
    fn asked(&self) -> Event {
        self.0.borrow().asked.last().cloned().expect("a question")
    }

    /// How many questions were asked.
    fn questions(&self) -> usize {
        self.0.borrow().asked.len()
    }

    /// The events shown.
    fn sent(&self) -> Vec<Event> {
        self.0.borrow().sent.clone()
    }

    /// How many times the console was handed over.
    fn suspended(&self) -> usize {
        self.0.borrow().suspended
    }

    /// Whether the view was closed.
    fn is_closed(&self) -> bool {
        self.0.borrow().closed
    }
}

/// A screen on `fake`'s clock, and what a test sees of it.
fn screen(fake: &FakeSystem) -> (Box<dyn StartupScreen>, Seen) {
    let log = Rc::new(RefCell::new(Log::default()));
    let screen = Screen {
        snapshot: Snapshot::default(),
        log: Rc::clone(&log),
        clock: fake.clock.clone(),
    };
    (Box::new(screen), Seen(log))
}

/// The front end on a screen of `fake`.
fn front(fake: &FakeSystem) -> (MenuFront<'_>, Seen) {
    let (screen, seen) = screen(fake);
    let state = State::new(&fake.files, "/zfsbootmenu");
    let system = fake.system();
    let luks = Settings::new("/zfsbootmenu");
    (MenuFront::new(screen, (system, system), state, luks), seen)
}

fn asked(question: Question, seconds: Option<u64>) -> Event {
    Event::Asked { question, seconds }
}

#[test]
fn questions_go_on_by_themselves_and_stop_waiting_without_the_view() {
    let fake = FakeSystem::new();
    let system = fake.system();
    let (mut front, seen) = front(&fake);
    assert_eq!(front.devices_missing(&system, 0), Answer::Continue);
    assert_eq!(seen.questions(), 0);

    seen.reply(Reply::With(Action::Answer(Answer::Escape)));
    assert_eq!(front.devices_missing(&system, 5), Answer::Escape);
    assert_eq!(seen.asked(), asked(Question::MissingDevices, Some(5)));

    // Below 0, the question waits for an answer; one that is not an
    // answer goes on.
    seen.reply(Reply::With(Action::SkipUnlock("cryptroot".into())));
    assert_eq!(front.devices_missing(&system, -1), Answer::Continue);
    assert_eq!(seen.asked(), asked(Question::MissingDevices, None));

    // A terminal that fails closes the view, and start-up stops waiting.
    seen.reply(Reply::Fail);
    assert_eq!(front.devices_missing(&system, 5), Answer::Escape);
    assert_eq!(front.devices_missing(&system, 5), Answer::Escape);
    assert_eq!(seen.questions(), 3);
    assert!(!seen.is_closed());
}

#[test]
fn the_import_asks_before_trying_again_and_says_what_it_cannot_import() {
    let fake = FakeSystem::new();
    let (mut front, seen) = front(&fake);
    seen.reply(Reply::With(Action::Answer(Answer::Continue)));
    let pool = PoolRef::new("tank").unwrap();
    let answer = front.retry(Some(&pool), RetryDelay::from_conf("3"));
    assert_eq!(answer, Answer::Continue);
    let failed = Question::ImportFailed {
        pool: Some("tank".into()),
    };
    assert_eq!(seen.asked(), asked(failed, Some(3)));

    seen.reply(Reply::With(Action::Answer(Answer::Escape)));
    assert_eq!(front.retry(None, RetryDelay::default()), Answer::Escape);
    let failed = Question::ImportFailed { pool: None };
    assert_eq!(seen.asked(), asked(failed, Some(5)));

    seen.reply(Reply::With(Action::Answer(Answer::Continue)));
    front.unsupported_features("Upgrade ZFSBootMenu.");
    let unsupported = Question::Unsupported {
        advice: "Upgrade ZFSBootMenu.".into(),
    };
    assert_eq!(seen.asked(), asked(unsupported, Some(30)));
}

#[test]
fn the_console_is_handed_over_until_the_view_closes() {
    let fake = FakeSystem::new();
    let system = fake.system();
    let (mut front, seen) = front(&fake);
    let mut ran = 0;
    front.with_console(&system, &mut |_| ran += 1);
    assert_eq!((ran, seen.suspended()), (1, 1));

    Operator::emergency_shell(&mut front, "unable to import a pool");
    assert_eq!(seen.suspended(), 2);
    let banner = fake.console.output_text();
    assert!(banner.contains("emergency shell"));
    assert!(banner.contains(": unable to import a pool"));
    assert!(
        fake.runner
            .lines()
            .iter()
            .any(|line| line == "/bin/bash -i")
    );

    front.finish();
    assert!(seen.is_closed());
    front.finish();
    Recovery::emergency_shell(&mut front, "no pool");
    front.with_console(&system, &mut |_| ran += 1);
    assert_eq!((ran, seen.suspended()), (2, 2));
    front.send(Event::StageStarted("Importing pools".into()));
    assert!(seen.sent().is_empty());
}

#[test]
fn late_volumes_are_unlocked_only_with_cryptsetup_and_fail_unless_one_opens() {
    let fake = FakeSystem::new();
    let (mut front, seen) = front(&fake);
    assert_eq!(
        Recovery::unlock_late(&mut front),
        Err(BootError::NoCryptsetup)
    );
    assert_eq!(Operator::unlock_late(&mut front), Late::Nothing);
    front.unlock_luks(&fake.system());

    fake.environment.set("PATH", "/sbin");
    fake.files.file_with_mode("/sbin/cryptsetup", "", 0o755);
    assert_eq!(
        Recovery::unlock_late(&mut front),
        Err(BootError::Zluks { status: 1 })
    );
    front.unlock_luks(&fake.system());
    assert_eq!(seen.questions(), 0);
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("zluks"))
    );
}

/// The configuration of a machine with the default boot environment.
fn conf() -> Conf {
    let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(""));
    Conf::parse(&options.conf("/zfsbootmenu")).unwrap()
}

/// A machine with one pool, `tank`, and a terminal that can run the menu.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    let runner = &fake.runner;
    runner.respond("zpool list -H -o name", 0, "tank\n");
    runner.respond("zpool list -H -o name,health", 0, "tank\tONLINE\n");
    runner.respond("zpool list -H -o bootfs --", 0, "tank/ROOT/void\n");
    fake.files.dir("/libexec/hooks/early-setup.d");
    fake.environment.set("TERM", "linux");
    fake
}

#[test]
fn start_up_runs_on_the_view_which_closes_before_the_menu() {
    let fake = machine();
    let startup = Startup::new(fake.system(), &conf());
    let (screen, seen) = screen(&fake);
    let screen = RefCell::new(Some(screen));
    let open = |snapshot: Snapshot, _: Theme| -> io::Result<Box<dyn StartupScreen>> {
        assert_eq!(snapshot.startup, stages());
        Ok(screen.take().expect("one view"))
    };
    let theme = Theme::new(Capabilities::FULL);
    let modes = (Modes::default(), Some(0));
    let status = run_on(&startup, fake.system(), &conf(), modes, (theme, &open));
    assert_eq!(status, Some(0));
    assert!(seen.is_closed());
    let sent = seen.sent();
    let finished = |event: &Event| matches!(event, Event::StageFinished { .. });
    assert_eq!(sent.iter().filter(|event| finished(event)).count(), 6);
    assert!(sent.contains(&Event::StageStarted("Importing pools".into())));
    // The stages' programs run off the console.
    let import = fake
        .runner
        .calls()
        .into_iter()
        .find(|call| call.program() == "zpool" && call.display().starts_with("zpool import"));
    assert!(import.is_some());

    let failed = |_: Snapshot, _: Theme| -> io::Result<Box<dyn StartupScreen>> {
        Err(io::Error::other("not a terminal"))
    };
    let theme = Theme::new(Capabilities::FULL);
    let modes = (Modes::default(), Some(0));
    assert_eq!(
        run_on(&startup, fake.system(), &conf(), modes, (theme, &failed)),
        None
    );
}

#[test]
fn start_up_keeps_the_plain_console_without_a_terminal_or_a_menu() {
    let fake = machine();
    let startup = Startup::new(fake.system(), &conf());
    let modes = Modes {
        console: true,
        ..Modes::default()
    };
    // The image has no menu.
    assert_eq!(run_in_menu(&startup, fake.system(), &conf(), modes), None);
    fake.files
        .file_with_mode(zbm_startup::program::menu::MENU, "", 0o755);
    fake.environment.set("TERM", "dumb");
    assert_eq!(run_in_menu(&startup, fake.system(), &conf(), modes), None);
    assert!(fake.runner.calls().is_empty());
}

#[test]
fn keys_may_come_in_pieces_off_the_virtual_console() {
    assert_eq!(escape_wait("/dev/tty1"), CONSOLE_ESCAPE_WAIT);
    for remote in ["/dev/ttyS0", "/dev/hvc0", "/dev/ttyAMA0"] {
        assert_eq!(escape_wait(remote), SERIAL_ESCAPE_WAIT, "{remote}");
    }
}
