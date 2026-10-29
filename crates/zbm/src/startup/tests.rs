use std::collections::VecDeque;
use std::time::Duration;

use pretty_assertions::assert_eq;
use zbm_config::args::ZbmArgs;
use zbm_platform::event::{Awaiting, reply};
use zbm_testkit::Shared;
use zbm_testkit::fakes::{FakeClock, FakeSystem};
use zbm_tui::menu::machine::RealMachine;
use zbm_tui::menu::screen::MenuScreen;
use zbm_tui::menu::startup::ToView;

use super::*;

mod prompts;

/// How the view answers the next question.
enum Reply {
    /// With this action.
    With(Action),
    /// With this action, after this many seconds.
    Late(u64, Action),
    /// Not at all: what the question waits for answers it, time passing as
    /// the stages look at it.
    Check,
    /// The terminal fails.
    Fail,
}

/// What the view was shown and did.
#[derive(Default)]
struct Log {
    sent: Vec<Event>,
    asked: Vec<Event>,
    replies: VecDeque<Reply>,
    suspended: usize,
    closed: bool,
    /// The terminal failed: the view is gone.
    failed: bool,
    /// The reply of a question nothing answers, until it is withdrawn.
    waiting: Option<zbm_platform::event::Reply<Action>>,
    withdrawn: usize,
    /// The countdown of each round of the menu that went on from the view.
    rounds: Vec<zbm_app::model::Timeout>,
}

/// A view that answers from a script, on the stages' own thread.
struct Scripted {
    log: Shared<Log>,
    clock: FakeClock,
}

impl ViewLink for Scripted {
    fn show(&self, event: Event) -> bool {
        let mut log = self.log.borrow_mut();
        if !log.failed {
            log.sent.push(event);
        }
        !log.failed
    }

    fn ask(&self, event: Event) -> Option<Awaiting<Action>> {
        let script = {
            let mut log = self.log.borrow_mut();
            log.asked.push(event.clone());
            log.replies.pop_front()
        };
        let (answer, awaiting) = reply();
        match script.unwrap_or_else(|| panic!("nothing answers {event:?}")) {
            Reply::With(action) => answer.send(action),
            Reply::Late(seconds, action) => {
                self.clock.advance(Duration::from_secs(seconds));
                answer.send(action);
            }
            Reply::Check => self.log.borrow_mut().waiting = Some(answer),
            Reply::Fail => {
                self.log.borrow_mut().failed = true;
                return None;
            }
        }
        Some(awaiting)
    }

    fn withdraw(&self) {
        let mut log = self.log.borrow_mut();
        log.waiting = None;
        log.withdrawn += 1;
    }

    fn lend(&self) {
        self.log.borrow_mut().suspended += 1;
    }

    fn resume(&self) {}

    fn close(&self) {
        self.log.borrow_mut().closed = true;
    }
}

/// The start-up view of [`run_on`], serving the mailbox as the real one
/// does, its questions answered from the script.
struct Screen(Shared<Log>);

impl StartupScreen for Screen {
    fn serve(
        self: Box<Self>,
        mailbox: Mailbox<ToView>,
        running: &dyn Fn() -> bool,
    ) -> io::Result<Option<Box<dyn MenuScreen>>> {
        while running() {
            let _ = zbm_sys::wait_readable(&mailbox, Some(Duration::from_millis(10)));
            for message in mailbox.take() {
                let mut log = self.0.borrow_mut();
                match message {
                    ToView::Show(event) => log.sent.push(*event),
                    ToView::Ask { event, reply } => {
                        log.asked.push(*event);
                        if let Some(Reply::With(action)) = log.replies.pop_front() {
                            reply.send(action);
                        }
                    }
                    ToView::Lend { ready } => {
                        log.suspended += 1;
                        ready.send(());
                    }
                    ToView::Withdraw | ToView::Resume => {}
                    ToView::Close { done } => {
                        log.closed = true;
                        done.send(());
                        return Ok(Some(Box::new(Rounds(self.0.clone()))));
                    }
                }
            }
        }
        Ok(None)
    }
}

/// The menu's screen after the view: it records how each round's machine
/// counts down, and ends the round at once.
struct Rounds(Shared<Log>);

impl MenuScreen for Rounds {
    fn run(&mut self, machine: RealMachine<'_, '_>) -> io::Result<String> {
        use zbm_tui::menu::machine::Machine as _;
        let timeout = machine.snapshot().timeout;
        self.0.borrow_mut().rounds.push(timeout);
        Ok(String::new())
    }
}

/// What a test sees of the screen.
#[derive(Clone)]
struct Seen(Shared<Log>);

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

/// The front end on a scripted view of `fake`, and what a test sees of
/// the view.
fn front(fake: &FakeSystem) -> (MenuFront<'_>, Seen) {
    let log = Shared::new(Log::default());
    let view = Scripted {
        log: log.clone(),
        clock: fake.clock.clone(),
    };
    let state = State::new(&fake.files, "/zfsbootmenu");
    let system = fake.system();
    let luks = Settings::new("/zfsbootmenu");
    let link: Box<dyn ViewLink> = Box::new(view);
    let front = MenuFront::new((link, Snapshot::default()), (system, system), state, luks);
    (front, Seen(log))
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
    let (front, seen) = front(&fake);
    seen.reply(Reply::With(Action::Answer(Answer::Continue)));
    let pool = "tank".parse::<PoolName>().unwrap();
    let answer = front.retry(Some(&pool), RetryDelay::or_default("3"));
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

    Operator::emergency_shell(&front, "unable to import a pool");
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
    Recovery::emergency_shell(&front, "no pool");
    front.with_console(&system, &mut |_| ran += 1);
    assert_eq!((ran, seen.suspended()), (2, 2));
    front.send(Event::StageStarted("Importing pools".into()));
    assert!(seen.sent().is_empty());
}

#[test]
fn late_volumes_are_unlocked_only_with_cryptsetup_and_fail_unless_one_opens() {
    let fake = FakeSystem::new();
    let (mut front, seen) = front(&fake);
    assert_eq!(Recovery::unlock_late(&front), Err(BootError::NoCryptsetup));
    assert_eq!(Operator::unlock_late(&front), Late::Nothing);
    front.unlock_luks(&fake.system());

    fake.environment.set("PATH", "/sbin");
    fake.files.file_with_mode("/sbin/cryptsetup", "", 0o755);
    assert_eq!(
        Recovery::unlock_late(&front),
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
    Conf::parse(&options.conf("/zfsbootmenu"))
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
    let log = Shared::new(Log::default());
    let seen = Seen(log.clone());
    let screen = std::cell::RefCell::new(Some(Screen(log)));
    let open = |snapshot: Snapshot, _: Theme| -> io::Result<Box<dyn StartupScreen>> {
        assert_eq!(snapshot.startup, stages());
        Ok(Box::new(screen.take().expect("one view")))
    };
    let no_menu = |_: Theme| -> io::Result<Box<dyn MenuScreen>> { panic!("not resumed") };
    let theme = Theme::new(Capabilities::FULL);
    let modes = (Modes::default(), Some(0));
    let screens = (theme, &open as Open<'_>, &no_menu as OpenMenu<'_>);
    let status = run_on(
        &startup,
        (fake.system(), &conf()),
        modes,
        screens,
        (None, None),
    );
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
    let screens = (theme, &failed as Open<'_>, &no_menu as OpenMenu<'_>);
    assert_eq!(
        run_on(
            &startup,
            (fake.system(), &conf()),
            modes,
            screens,
            (None, None)
        ),
        None
    );
}

#[test]
fn the_session_s_menu_goes_on_from_the_view_and_counts_down_the_first_time() {
    let fake = machine();
    fake.files.file("/dev/zfs", "");
    let startup = Startup::new(fake.system(), &conf());
    let log = Shared::new(Log::default());
    let seen = Seen(log.clone());
    let screen = std::cell::RefCell::new(Some(Screen(log)));
    let open = |_: Snapshot, _: Theme| -> io::Result<Box<dyn StartupScreen>> {
        Ok(Box::new(screen.take().expect("one view")))
    };
    let no_menu = |_: Theme| -> io::Result<Box<dyn MenuScreen>> { panic!("not resumed") };
    let console = Modes {
        console: true,
        ..Modes::default()
    };
    let screens = (
        Theme::new(Capabilities::FULL),
        &open as Open<'_>,
        &no_menu as OpenMenu<'_>,
    );

    let status = run_on(
        &startup,
        (fake.system(), &conf()),
        (console, Some(2)),
        screens,
        (None, None),
    );

    assert_eq!(status, Some(0));
    assert!(seen.is_closed());
    let rounds = seen.0.borrow().rounds.clone();
    assert_eq!(
        rounds,
        [
            zbm_app::model::Timeout::Seconds(10),
            zbm_app::model::Timeout::Wait
        ]
    );
    // No menu program: the menu ran in this process.
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("/bin/zfsbootmenu"))
    );

    // Resumed, the menu opens at once, waiting for the user.
    let log = Shared::new(Log::default());
    let resumed = Seen(log.clone());
    let menu = std::cell::RefCell::new(Some(Rounds(log)));
    let open_menu = |_: Theme| -> io::Result<Box<dyn MenuScreen>> {
        Ok(Box::new(menu.take().expect("one screen")))
    };
    let no_view =
        |_: Snapshot, _: Theme| -> io::Result<Box<dyn StartupScreen>> { panic!("resumed") };
    let resume = Modes {
        resume: true,
        ..console
    };
    let screens = (
        Theme::new(Capabilities::FULL),
        &no_view as Open<'_>,
        &open_menu as OpenMenu<'_>,
    );
    let status = run_on(
        &startup,
        (fake.system(), &conf()),
        (resume, Some(1)),
        screens,
        (None, None),
    );
    assert_eq!(status, Some(0));
    assert_eq!(resumed.0.borrow().rounds, [zbm_app::model::Timeout::Wait]);
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

#[test]
fn a_terminal_too_small_for_the_view_keeps_the_plain_console() {
    assert!(fits((80, 25)) && fits((52, 20)));
    assert!(!fits((51, 25)) && !fits((80, 19)));
}
