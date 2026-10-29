//! `zfsbootmenu`: the menu of the Rust runtime, at `/bin/zfsbootmenu`
//! (ROADMAP.md, R5 step 3), as users run it over SSH or from the recovery
//! shell, and as the start-up program runs it in its own process, the
//! session (`InSession`, PM9), where the legacy loop runs
//! `bin/zfsbootmenu`. Over SSH it takes the session's menu over by the
//! session's socket (PM10). The program around the menu is
//! `zbm_startup::program::menu`; the menu is `zbm_tui::menu` on the real
//! machine.

use std::io;
use std::path::Path;

use zbm_app::service::MachineService;
use zbm_app::session::{self, Handover};
use zbm_app::{App, BootSettings, Recovery};
use zbm_config::conf::Conf;
use zbm_config::options::ZbmOptions;
use zbm_platform::{Context, System};
use zbm_startup::preinit;
use zbm_startup::program::Startup;
use zbm_startup::program::menu::{Menu, SessionMenu};
use zbm_sys::{Signal, SignalSet, Signals};
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::machine::{Countdown, RealMachine};
use zbm_tui::menu::screen::MenuScreen;
use zbm_tui::menu::session::Session;
use zbm_tui::menu::terminal;
use zbm_tui::menu::theme::Theme;

use crate::applets::on_console;
use crate::{Io, layouts};

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
        let app = App::new(Context::new(self.system))
            .with_options(self.options)
            .with_base(self.base)
            .with_layouts(Path::new(layouts::DIRECTORY))
            .with_boot(settings);
        let (signals, handover) = (self.signals, self.handover);
        let stop = || {
            signals.is_some_and(|signals| is_taken_over(signals.received()))
                || handover.is_some_and(Handover::is_asked)
        };
        let machine = match RealMachine::new(app, self.system, recovery, &stop, self.countdown) {
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

#[cfg(test)]
mod tests;
