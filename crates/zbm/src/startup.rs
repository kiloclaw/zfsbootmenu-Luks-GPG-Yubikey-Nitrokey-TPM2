//! Start-up in the menu (ROADMAP.md, R5 step 4): the start-up
//! stages on the menu's start-up view.
//!
//! [`MenuFront`] is the stages' front end ([`Front`]) on the start-up
//! view ([`StartupScreen`]). It shows the stages as they run, and answers
//! what they ask with the view's dialogs: the secrets and devices of LUKS
//! volumes (its [`zbm_platform::interaction::Interaction`], in `prompts`), the
//! retries of the import stage ([`Operator`]) and the devices
//! `zbm.wait_for` names. LUKS volumes are unlocked in this program, where
//! their secrets are typed: secrets stay in locked memory and are wiped
//! when the unlocking run ends (see "Secrets" in `crates/DESIGN.md`). Hooks
//! and the recovery shell get the console, which the view hands over; the
//! stages' other programs run off the console ([`QuietRunner`]), what they
//! print going to the log. Without a terminal that can run the menu,
//! start-up keeps the plain console.

mod prompts;

use std::io;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use zbm_app::model::{Action, Event, Method, Question, Snapshot, Stage, StageState, UnlockOutcome};
use zbm_app::{App, BootError, Recovery};
use zbm_boot::prompt::retry_delay;
use zbm_config::conf::Conf;
use zbm_config::options::ZbmOptions;
use zbm_luks::unlocker::{Luks, Settings};
use zbm_platform::{Context, NoConsole, QuietRunner, System};
use zbm_pool::{ImportSettings, Imported, Late, Operator, RetryDelay};
use zbm_startup::front::Front;
use zbm_startup::program::menu::has_menu;
use zbm_startup::program::{Modes, Startup};
use zbm_startup::shell::Shell;
use zbm_state::State;
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::startup::StartupScreen;
use zbm_tui::menu::term_keys::{CONSOLE_ESCAPE_WAIT, SERIAL_ESCAPE_WAIT};
use zbm_tui::menu::terminal;
use zbm_tui::menu::theme::Theme;
use zbm_types::Answer;
use zbm_types::PoolName;

use crate::layouts;

/// How long a question waits when nothing says, as the console prompts
/// wait: 30 seconds.
const DEFAULT_DELAY: i64 = 30;

/// The start-up stages' front end on the menu's start-up view.
pub struct MenuFront<'a> {
    /// The view; `None` once start-up is over, or when the terminal failed.
    view: Mutex<Option<Box<dyn StartupScreen + 'a>>>,
    /// The whole system: hooks and the recovery shell get its console.
    system: System<'a>,
    /// The system the stages run their programs on, off the console.
    quiet: System<'a>,
    /// The state directory.
    state: State<'a>,
    /// The settings of LUKS unlocking.
    luks: Settings,
    /// What unlocking said went wrong since the last request: why the next
    /// one asks.
    refused: Mutex<Vec<String>>,
    /// The method the last request asked for.
    last: Mutex<Option<Method>>,
}

impl<'a> MenuFront<'a> {
    /// The front end on `view`: hooks and the recovery shell run on
    /// `system`, unlocking on `quiet`, with the state directory `state` and
    /// the LUKS settings `luks`.
    pub fn new(
        view: Box<dyn StartupScreen + 'a>,
        (system, quiet): (System<'a>, System<'a>),
        state: State<'a>,
        luks: Settings,
    ) -> MenuFront<'a> {
        MenuFront {
            view: Mutex::new(Some(view)),
            system,
            quiet,
            state,
            luks,
            refused: Mutex::new(Vec::new()),
            last: Mutex::new(None),
        }
    }

    /// `show` on the view, if it is open; `None` when it is not, or when
    /// the terminal failed, which closes it.
    fn with_view<T>(
        &self,
        show: impl FnOnce(&mut (dyn StartupScreen + 'a)) -> io::Result<T>,
    ) -> Option<T> {
        let mut view = lock(&self.view);
        match show(view.as_deref_mut()?) {
            Ok(value) => Some(value),
            Err(error) => {
                zbm_trace::error!("the start-up view failed: {error}"; %error);
                *view = None;
                None
            }
        }
    }

    /// Shows `event`.
    fn show(&self, event: Event) {
        let _ = self.with_view(|view| view.send(event));
    }

    /// Asks `question`, going on by itself after `delay` seconds (0: at
    /// once, below 0: never), as the console's timed prompts do. Without the
    /// view, start-up stops waiting.
    fn timed(&self, question: Question, delay: i64) -> Answer {
        if delay == 0 {
            return Answer::Continue;
        }
        let seconds = u64::try_from(delay).ok();
        let event = Event::Asked { question, seconds };
        match self.with_view(|view| view.ask(&event, &mut || None)) {
            Some(Action::Answer(answer)) => answer,
            Some(_) => Answer::Continue,
            None => Answer::Escape,
        }
    }

    /// Runs `run` with the console: the view hands it over first.
    fn hand_over(&self, run: impl FnOnce()) {
        let mut run = Some(run);
        let _ = self.with_view(|view| {
            view.suspend(&mut || {
                if let Some(run) = run.take() {
                    run();
                }
            })
        });
        if let Some(run) = run {
            run();
        }
    }

    /// The recovery shell with `message`, on the console.
    fn shell(&self, message: &str) {
        let mut shell = Shell::new(self.system, &self.state);
        self.hand_over(|| shell.emergency_shell(message));
    }

    /// Unlocks the LUKS volumes with `system`'s programs, answering their
    /// requests with the view, the volumes that appeared late when `late`;
    /// what happened.
    fn unlock(&self, system: &System<'_>, late: bool) -> Vec<Event> {
        if !system.command_exists("cryptsetup") {
            return Vec::new();
        }
        let mut settings = self.luks.clone();
        settings.retry = late;
        let luks = Luks {
            context: Context::new(*system),
            settings: &settings,
            interaction: self,
        };
        let app = App::new(Context::new(*system)).with_luks(luks);
        let events = app.act(&Action::UnlockAll);
        for event in &events {
            if matches!(event, Event::UnlockFinished { .. }) {
                self.show(event.clone());
            }
        }
        events
    }
}

impl Front for MenuFront<'_> {
    fn send(&mut self, event: Event) {
        self.show(event);
    }

    fn recovery(&mut self) -> &mut dyn Recovery {
        self
    }

    fn unlock_luks(&mut self, system: &System<'_>) {
        let _ = self.unlock(system, false);
    }

    fn devices_missing(&mut self, _system: &System<'_>, delay: i64) -> Answer {
        self.timed(Question::MissingDevices, delay)
    }

    fn with_console(&mut self, _system: &System<'_>, run: &mut dyn FnMut(&System<'_>)) {
        let system = self.system;
        self.hand_over(|| run(&system));
    }

    fn import(
        &mut self,
        system: &System<'_>,
        state: &State<'_>,
        settings: &ImportSettings,
    ) -> Imported {
        zbm_pool::stage(&Context::new(*system), state, settings, self)
    }

    fn finish(&mut self) {
        if let Some(view) = lock(&self.view).take()
            && let Err(error) = view.close()
        {
            zbm_trace::error!("the start-up view failed: {error}"; %error);
        }
    }
}

impl Recovery for MenuFront<'_> {
    /// Unlocks the volumes that appeared since the first unlock; it fails
    /// unless it opened one, as `zluks unlock` does as a retry.
    fn unlock_late(&mut self) -> Result<(), BootError> {
        let quiet = self.quiet;
        if !quiet.command_exists("cryptsetup") {
            return Err(BootError::NoCryptsetup);
        }
        let opened = self.unlock(&quiet, true).iter().any(|event| {
            matches!(
                event,
                Event::UnlockFinished {
                    outcome: UnlockOutcome::Opened,
                    ..
                }
            )
        });
        if opened {
            Ok(())
        } else {
            Err(BootError::Zluks { status: 1 })
        }
    }

    fn emergency_shell(&mut self, message: &str) {
        self.shell(message);
    }
}

impl Operator for MenuFront<'_> {
    fn unlock_late(&mut self) -> Late {
        if Recovery::unlock_late(self).is_ok() {
            Late::Unlocked
        } else {
            Late::Nothing
        }
    }

    fn retry(&mut self, target: Option<&PoolName>, delay: RetryDelay) -> Answer {
        let pool = target.map(|target| target.as_str().to_owned());
        self.timed(Question::ImportFailed { pool }, retry_delay(delay))
    }

    fn unsupported_features(&mut self, advice: &str) {
        let advice = advice.to_owned();
        let _ = self.timed(Question::Unsupported { advice }, DEFAULT_DELAY);
    }

    fn emergency_shell(&mut self, message: &str) {
        self.shell(message);
    }
}

/// `mutex`, locked: what it holds stays usable after a panic elsewhere.
fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The stages of start-up, none started yet.
fn stages() -> Vec<Stage> {
    zbm_types::Stage::ALL
        .iter()
        .map(|stage| Stage {
            name: zbm_app::stage_name(*stage).to_owned(),
            state: StageState::Pending,
        })
        .collect()
}

/// Opens the start-up view on a snapshot, drawn with a theme.
type Open<'o> = &'o dyn Fn(Snapshot, Theme) -> io::Result<Box<dyn StartupScreen>>;

/// Runs the start-up program `startup` with `modes`, its stages on the
/// menu's start-up view, when the image has the menu and the terminal can
/// run it: its exit status. `None` when it cannot, for the plain console.
pub fn run_in_menu(
    startup: &Startup<'_>,
    system: System<'_>,
    conf: &Conf,
    modes: Modes,
) -> Option<i32> {
    let environment = system.environment;
    let capabilities = Capabilities::detect(|name| environment.var(name));
    if !capabilities.interactive || !has_menu(&system) {
        return None;
    }
    let escape_wait = escape_wait(&ZbmOptions::from_conf(conf).control_term);
    let open = |snapshot, theme| {
        let size = terminal::size(crate::menu::FALLBACK_SIZE);
        if !fits(size) {
            // Its dialogs would not show, while they wait for an answer.
            return Err(io::Error::other(format!(
                "the terminal ({}x{}) is smaller than the start-up view needs",
                size.0, size.1
            )));
        }
        terminal::startup_view(snapshot, theme, size, escape_wait)
    };
    run_on(
        startup,
        system,
        conf,
        (modes, None),
        (Theme::new(capabilities), &open),
    )
}

/// Whether a terminal of `size` (columns, rows) shows the start-up view:
/// smaller, start-up keeps the plain console.
fn fits((columns, rows): (u16, u16)) -> bool {
    let (width, height) = zbm_tui::menu::view::MINIMUM;
    columns >= width && rows >= height
}

/// How long the rest of a key's sequence may take on the control terminal
/// `control_term`: little on a virtual console, whose keys the kernel
/// writes at once, or the console on the display (`zbm.console=drm`), which
/// runs on one; longer on a serial console, which may be reached through
/// the network.
pub(crate) fn escape_wait(control_term: &str) -> Duration {
    if zbm_startup::preinit::is_virtual_console(control_term) {
        CONSOLE_ESCAPE_WAIT
    } else {
        SERIAL_ESCAPE_WAIT
    }
}

/// [`run_in_menu`] on the view `open` opens with `theme`, the recovery
/// loop limited to `rounds`.
fn run_on(
    startup: &Startup<'_>,
    system: System<'_>,
    conf: &Conf,
    (modes, rounds): (Modes, Option<usize>),
    (theme, open): (Theme, Open<'_>),
) -> Option<i32> {
    let options = ZbmOptions::from_conf(conf);
    let base = zbm_startup::preinit::base(conf);
    let keyboard = App::new(Context::new(system))
        .with_options(&options)
        .with_base(&base)
        .with_layouts(Path::new(layouts::DIRECTORY))
        .keyboard();
    let snapshot = Snapshot {
        startup: stages(),
        keyboard,
        ..Snapshot::default()
    };
    let view = match open(snapshot, theme) {
        Ok(view) => view,
        Err(error) => {
            zbm_trace::error!("unable to show start-up in the menu: {error}"; %error);
            return None;
        }
    };
    let quiet_runner = QuietRunner::new(system.runner);
    let quiet = System {
        runner: &quiet_runner,
        console: &NoConsole,
        ..system
    };
    let state = State::new(system.files, &base);
    let luks = crate::luks::settings(&system, conf);
    let mut front = MenuFront::new(view, (system, quiet), state, luks);
    Some(startup.run_with(modes, rounds, Some((&quiet, &mut front))))
}

#[cfg(test)]
mod tests;
