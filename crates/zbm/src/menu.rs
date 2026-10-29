//! `zfsbootmenu`: the menu of the Rust runtime, at `/bin/zfsbootmenu`
//! (ROADMAP.md, R5 step 3). The start-up program runs it after the
//! countdown, as the legacy loop runs `bin/zfsbootmenu`, and users run it
//! over SSH or from the recovery shell. The program around the menu is
//! `zbm_startup::program::menu`; the menu is `zbm_tui::menu` on the real
//! machine.

use std::io;
use std::path::Path;
use std::time::Duration;

use zbm_app::service::MachineService;
use zbm_app::{App, BootSettings, Recovery};
use zbm_config::conf::Conf;
use zbm_config::options::ZbmOptions;
use zbm_platform::{Context, System};
use zbm_startup::preinit;
use zbm_startup::program::Startup;
use zbm_startup::program::menu::Menu;
use zbm_sys::{Signal, SignalSet, Signals};
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::machine::{Countdown, RealMachine};
use zbm_tui::menu::session::Session;
use zbm_tui::menu::terminal;
use zbm_tui::menu::theme::Theme;

use crate::applets::on_console;
use crate::{Io, layouts};

/// The size to draw in when the terminal reports none.
pub(crate) const FALLBACK_SIZE: (u16, u16) = (80, 24);

/// Runs the menu until it ends, the rest of a key's sequence taking up to
/// the duration given; why it ended.
type Run = fn(Session<RealMachine<'_, '_>>, Duration) -> io::Result<String>;

/// The menu on the terminal.
fn on_terminal(session: Session<RealMachine<'_, '_>>, escape_wait: Duration) -> io::Result<String> {
    terminal::run(session, terminal::size(FALLBACK_SIZE), None, escape_wait)
}

/// The menu over the machine `system` reaches.
struct Terminal<'s, 'a> {
    system: System<'a>,
    /// The signals of a takeover, when they could be blocked.
    signals: Option<&'s Signals>,
    options: &'s ZbmOptions,
    base: &'s Path,
    run: Run,
    countdown: Countdown,
    /// Why the menu could not run, if it could not.
    error: Option<String>,
}

/// The signals that end the menu: another instance asks it to (`SIGUSR1`
/// to it, or `SIGTERM` when it ran no child), or its terminal went away
/// (`SIGHUP`, a dropped SSH session). It then returns and gives up the
/// `active` lock, as the legacy EXIT trap did.
const TAKEOVER: SignalSet = SignalSet::of(&[Signal::User1, Signal::Terminate, Signal::Hangup]);

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
        let signals = self.signals;
        let stop = || signals.is_some_and(|signals| is_taken_over(signals.received()));
        let machine = match RealMachine::new(app, self.system, recovery, &stop, self.countdown) {
            Ok(machine) => machine,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let environment = self.system.environment;
        let capabilities = Capabilities::detect(|name| environment.var(name));
        let escape_wait = crate::startup::escape_wait(&self.options.control_term);
        // The service performs the actions that leave the console alone on
        // a worker, while the menu draws; it ends with the menu.
        let service = MachineService::new(app, self.system);
        let ran = std::thread::scope(|scope| {
            let machine = match service.start(scope) {
                Ok(handle) => machine.with_service(handle),
                Err(_) => machine,
            };
            (self.run)(Session::on(machine, Theme::new(capabilities)), escape_wait)
        });
        if let Err(error) = ran {
            self.error = Some(error.to_string());
        }
    }
}

/// The menu program on `system` with the configuration `conf`, the menu
/// run by `run`, ended by `signals`: its exit status, or why the menu
/// could not run.
fn menu(
    system: System<'_>,
    conf: &Conf,
    (run, countdown): (Run, Countdown),
    signals: Option<&Signals>,
) -> Result<i32, String> {
    let options = ZbmOptions::from_conf(conf);
    let base = preinit::base(conf);
    let mut menu = Terminal {
        system,
        signals,
        options: &options,
        base: &base,
        run,
        countdown,
        error: None,
    };
    let status = Startup::new(system, conf).menu(&mut menu);
    menu.error.map_or(Ok(status), Err)
}

/// `zfsbootmenu`: the menu. With `--countdown`, which the start-up program
/// passes at the end of start-up, it counts down to booting the
/// environment start-up chose. The exit status is 0 when the menu ended, 1
/// when the machine could not be read or the terminal used, 2 for an
/// unknown option.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let countdown = match args {
        [] => Countdown::Skip,
        [flag] if flag == "--countdown" => Countdown::Run,
        _ => {
            let _ = writeln!(io.stderr, "zfsbootmenu: unknown options {}", args.join(" "));
            return crate::USAGE;
        }
    };
    // trap '' SIGINT; the USR1 trap of a takeover, whose signals wait to be
    // read, first, before any thread starts; and SIGQUIT, which Bash ignores
    let signals = Signals::block(TAKEOVER).ok();
    let _ = zbm_sys::ignore_signal(Signal::Interrupt);
    let _ = zbm_sys::discard_signal(Signal::Quit);
    on_console(io, |system, conf, io| {
        let run = (on_terminal as Run, countdown);
        menu(system, conf, run, signals.as_ref()).unwrap_or_else(|error| {
            let _ = writeln!(io.stderr, "zfsbootmenu: {error}");
            zbm_startup::program::menu::FAILED
        })
    })
}

#[cfg(test)]
mod tests;
