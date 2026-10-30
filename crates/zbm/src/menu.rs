//! `zfsbootmenu`: the menu of the Rust runtime, at `/bin/zfsbootmenu`
//! (ROADMAP.md, R5 step 3), as users run it over SSH or from the recovery
//! shell, and as the start-up program runs it in its own process, the
//! session (`InSession`, PM9), where the legacy loop runs
//! `bin/zfsbootmenu`. Over SSH it takes the session's menu over by the
//! session's socket (PM10). The program around the menu is
//! `zbm_startup::program::menu`; the menu is `zbm_tui::menu` on the real
//! machine.

use std::io;
use std::os::fd::{AsFd as _, BorrowedFd};
use std::path::Path;

use zbm_app::service::MachineService;
use zbm_app::session::{self, Handover};
use zbm_app::{App, BootSettings, Recovery, Runtime};
use zbm_config::conf::Conf;
use zbm_config::options::ZbmOptions;
use zbm_platform::{Context, System};
use zbm_startup::preinit;
use zbm_startup::program::Startup;
use zbm_startup::program::menu::{Menu, SessionMenu};
use zbm_sys::{Signal, SignalSet, Signals};
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::machine::{Countdown, RealMachine, RemoteMachine};
use zbm_tui::menu::screen::MenuScreen;
use zbm_tui::menu::session::Session;
use zbm_tui::menu::terminal;
use zbm_tui::menu::theme::Theme;

use crate::Io;
use crate::applets::on_console;

/// The size to draw in when the terminal reports none.
pub(crate) const FALLBACK_SIZE: (u16, u16) = (80, 24);

/// Runs the menu on a machine until it ends; why it ended.
type Show<'s> = &'s mut dyn FnMut(RealMachine<'_, '_>) -> io::Result<String>;

/// The menu over the machine `system` reaches.
struct Terminal<'s, 'a> {
    system: System<'a>,
    /// The signals of a takeover, when they could be blocked.
    signals: Option<&'s Signals>,
    /// The session's socket asks its menu to end, in the session.
    handover: Option<&'s Handover>,
    /// The session's socket, which another instance asks first.
    session: Option<&'s Path>,
    options: &'s ZbmOptions,
    base: &'s Path,
    show: Show<'s>,
    countdown: Countdown,
    /// Why the menu could not run, if it could not.
    error: Option<String>,
}

/// The signals that end the menu: another instance asks it to (`SIGUSR1`
/// to it, or `SIGTERM` when it ran no child), or its terminal went away
/// (`SIGHUP`, a dropped SSH session). It then returns and gives up the
/// `active` lock, as the legacy EXIT trap did.
const TAKEOVER: SignalSet = SignalSet::of(&[Signal::User1, Signal::Terminate, Signal::Hangup]);

/// The signals that end the session's menu ([`InSession`]). The terminal's
/// hang-up keeps its default action, which ends the start-up program as it
/// always did, for the supervisor to start it again.
pub(crate) const SESSION_TAKEOVER: SignalSet = SignalSet::of(&[Signal::User1, Signal::Terminate]);

/// Whether the menu must end, having `received` these signals.
fn is_taken_over(received: SignalSet) -> bool {
    TAKEOVER.iter().any(|signal| received.contains(signal))
}

impl Menu for Terminal<'_, '_> {
    fn show(&mut self, settings: &BootSettings, recovery: &dyn Recovery) {
        let runtime = Runtime::new(self.options.clone(), self.base).with_boot(settings.clone());
        let app = App::new(Context::new(self.system), &runtime);
        let (signals, handover) = (self.signals, self.handover);
        let stop = || {
            signals.is_some_and(|signals| is_taken_over(signals.received()))
                || handover.is_some_and(Handover::is_asked)
        };
        let machine = match RealMachine::new(&app, self.system, recovery, &stop, self.countdown) {
            Ok(machine) => machine,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        // The service performs the actions that leave the console alone on
        // a worker, while the menu draws; it ends with the menu.
        let service = MachineService::new(app, self.system);
        let show = &mut self.show;
        let ran = std::thread::scope(|scope| match service.start(scope) {
            Ok(handle) => show(machine.with_service(handle)),
            Err(_) => show(machine),
        });
        if let Err(error) = ran {
            self.error = Some(error.to_string());
        }
    }

    fn session_socket(&self) -> Option<&Path> {
        self.session
    }
}

/// The menu program on `system` with the configuration `conf`, the menu
/// shown by `show`, counting down as `countdown` says, ended by `signals`:
/// its exit status, or why the menu could not run.
fn menu(
    system: System<'_>,
    conf: &Conf,
    (show, countdown): (Show<'_>, Countdown),
    signals: Option<&Signals>,
) -> Result<i32, String> {
    let options = ZbmOptions::from_conf(conf);
    let base = preinit::base(conf);
    let mut menu = Terminal {
        system,
        signals,
        handover: None,
        session: Some(Path::new(session::SOCKET)),
        options: &options,
        base: &base,
        show,
        countdown,
        error: None,
    };
    let status = Startup::new(system, conf).menu(&mut menu);
    menu.error.map_or(Ok(status), Err)
}

/// The menu in the start-up program, the session (ROADMAP.md, PM9): each
/// round of the recovery loop runs it on `screen`, which went on from the
/// start-up view, around what the menu program does first. Another
/// instance ends it by the session's socket, or by its takeover signals
/// ([`SESSION_TAKEOVER`]), which the program blocks before any thread
/// starts.
pub(crate) struct InSession<'s, 'a> {
    /// The screen the menu goes on with.
    pub screen: Box<dyn MenuScreen + 's>,
    /// The whole system.
    pub system: System<'a>,
    /// The image's configuration.
    pub conf: &'s Conf,
    /// The takeover signals, when they could be blocked.
    pub signals: Option<&'s Signals>,
    /// What the menu shares with the session's socket, when it listens.
    pub handover: Option<&'s Handover>,
}

impl SessionMenu for InSession<'_, '_> {
    fn run(&mut self, startup: &Startup<'_>, countdown: bool) {
        // A takeover meant for a menu that ended is not one for this one.
        if let Some(signals) = self.signals {
            let _ = signals.take();
            for signal in SESSION_TAKEOVER.iter() {
                signals.forget(signal);
            }
        }
        let options = ZbmOptions::from_conf(self.conf);
        let base = preinit::base(self.conf);
        let screen = &mut self.screen;
        let mut show = |machine: RealMachine<'_, '_>| screen.run(machine);
        let mut menu = Terminal {
            system: self.system,
            signals: self.signals,
            handover: self.handover,
            session: None,
            options: &options,
            base: &base,
            show: &mut show,
            countdown: if countdown {
                Countdown::Run
            } else {
                Countdown::Skip
            },
            error: None,
        };
        if let Some(handover) = self.handover {
            // A remote menu the session serves meanwhile ends: the console
            // takes the menu back, as it took it from a menu over SSH by
            // its signals before.
            if !handover.has_yielded(session::YIELD_WAIT) {
                zbm_trace::info!("the remote menu did not end: waiting for it");
            }
            handover.enter();
        }
        let _ = startup.menu(&mut menu);
        // The menu let go of the `active` lock: the instance that asked can
        // take it.
        if let Some(handover) = self.handover {
            handover.leave();
        }
        if let Some(error) = menu.error {
            zbm_trace::error!("the menu could not run: {error}"; %error);
        }
    }
}

/// `zfsbootmenu`: the menu, as users run it over SSH or from the recovery
/// shell; it waits for the user. The exit status is 0 when the menu ended,
/// 1 when the machine could not be read or the terminal used, 2 for an
/// option.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    if !args.is_empty() {
        let _ = writeln!(io.stderr, "zfsbootmenu: unknown options {}", args.join(" "));
        return crate::USAGE;
    }
    // trap '' SIGINT; the USR1 trap of a takeover, whose signals wait to be
    // read, first, before any thread starts; and SIGQUIT, which Bash ignores
    let signals = Signals::block(TAKEOVER).ok();
    let _ = zbm_sys::ignore_signal(Signal::Interrupt);
    let _ = zbm_sys::discard_signal(Signal::Quit);
    on_console(io, |system, conf, io| {
        let environment = system.environment;
        let theme = Theme::new(Capabilities::detect(|name| environment.var(name)));
        let escape_wait = crate::startup::escape_wait(&ZbmOptions::from_conf(conf).control_term);
        // The session's menu, when it serves one; else this process's.
        let stop = || {
            signals
                .as_ref()
                .is_some_and(|signals| is_taken_over(signals.received()))
        };
        let show_remote = |machine: RemoteMachine<'_>| {
            let session = Session::on(machine, theme);
            terminal::run(session, terminal::size(FALLBACK_SIZE), None, escape_wait)
        };
        let (input, output) = (io::stdin(), io::stdout());
        let served = Remote {
            socket: Path::new(session::SOCKET),
            terminal: (input.as_fd(), output.as_fd()),
            stop: &stop,
        };
        if let Some(status) = served.run(show_remote, io) {
            return status;
        }
        let mut show = |machine: RealMachine<'_, '_>| {
            let session = Session::on(machine, theme);
            terminal::run(session, terminal::size(FALLBACK_SIZE), None, escape_wait)
        };
        let show = (&mut show as Show<'_>, Countdown::Skip);
        menu(system, conf, show, signals.as_ref()).unwrap_or_else(|error| {
            let _ = writeln!(io.stderr, "zfsbootmenu: {error}");
            zbm_startup::program::menu::FAILED
        })
    })
}

/// The menu as a client of the session (ROADMAP.md, PM10), which runs the
/// menu program on this terminal and performs the menu's actions.
struct Remote<'s> {
    /// The session's socket.
    socket: &'s Path,
    /// The terminal's input and output, which the session takes.
    terminal: (BorrowedFd<'s>, BorrowedFd<'s>),
    /// Whether the menu is to end.
    stop: &'s dyn Fn() -> bool,
}

impl Remote<'_> {
    /// Shows the menu with `show` on the session's machine: its exit
    /// status; `None` when no session serves one, for the menu in this
    /// process.
    fn run(
        &self,
        show: impl FnOnce(RemoteMachine<'_>) -> io::Result<String>,
        io: &mut Io,
    ) -> Option<i32> {
        let connection = match session::ask_remote(self.socket) {
            Ok(connection) => connection,
            Err(Some(session::Answer::Refused(refusal))) => {
                zbm_trace::info!("the session serves no menu: {refusal}"; %refusal);
                return None;
            }
            Err(_) => return None,
        };
        zbm_trace::info!("the session serves the menu");
        let shown = RemoteMachine::attach(connection, self.terminal, self.stop).and_then(show);
        let status = match shown {
            Ok(_) => 0,
            // Start-up was not over, and the user did not have it run; or
            // the menu was to end before it showed.
            Err(error) if error.to_string() == crate::remote::NOT_SERVED => 0,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => 0,
            Err(error) => {
                let _ = writeln!(io.stderr, "zfsbootmenu: {error}");
                zbm_startup::program::menu::FAILED
            }
        };
        Some(status)
    }
}

#[cfg(test)]
mod tests;
