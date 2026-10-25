//! `zfsbootmenu`: the menu of the Rust runtime, at `/bin/zfsbootmenu`
//! (REFACTOR-ROADMAP.md, R5 step 3). The start-up program runs it after the
//! countdown, as the legacy loop runs `bin/zfsbootmenu`, and users run it
//! over SSH or from the recovery shell. The program around the menu is
//! `zbm_startup::program::menu`; the menu is `zbm_tui::menu` on the real
//! machine.

use std::io;
use std::path::Path;

use zbm_app::{App, BootSettings, Recovery};
use zbm_config::conf::Conf;
use zbm_config::options::ZbmOptions;
use zbm_platform::{Context, System};
use zbm_startup::preinit;
use zbm_startup::program::Startup;
use zbm_startup::program::menu::Menu;
use zbm_sys::Signal;
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::machine::{Countdown, RealMachine};
use zbm_tui::menu::session::Session;
use zbm_tui::menu::terminal;
use zbm_tui::menu::theme::Theme;

use crate::applets::on_console;
use crate::{Io, layouts};

/// The size to draw in when the terminal reports none.
pub(crate) const FALLBACK_SIZE: (u16, u16) = (80, 24);

/// Runs the menu until it ends; why it ended.
type Run = fn(Session<RealMachine<'_, '_>>) -> io::Result<String>;

/// The menu on the terminal.
fn on_terminal(session: Session<RealMachine<'_, '_>>) -> io::Result<String> {
    terminal::run(session, terminal::size(FALLBACK_SIZE), None)
}

/// The menu over the machine `system` reaches.
struct Terminal<'s, 'a> {
    system: System<'a>,
    options: &'s ZbmOptions,
    base: &'s Path,
    run: Run,
    countdown: Countdown,
    /// Why the menu could not run, if it could not.
    error: Option<String>,
}

/// Whether the menu must end: another instance asked it to (`SIGUSR1` to
/// it, or `SIGTERM` when it ran no child), or its terminal went away
/// (`SIGHUP`, a dropped SSH session). It then returns and gives up the
/// `active` lock, as the legacy EXIT trap did.
fn is_taken_over() -> bool {
    matches!(
        zbm_sys::received_signal(),
        Some(Signal::User1 | Signal::Terminate | Signal::Hangup)
    )
}

impl Menu for Terminal<'_, '_> {
    fn show(&mut self, settings: &BootSettings, recovery: &mut dyn Recovery) {
        let app = App::new(Context::new(self.system))
            .with_options(self.options)
            .with_base(self.base)
            .with_layouts(Path::new(layouts::DIRECTORY))
            .with_boot(settings);
        let stop = is_taken_over;
        let machine = match RealMachine::new(app, self.system, recovery, &stop, self.countdown) {
            Ok(machine) => machine,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let environment = self.system.environment;
        let capabilities = Capabilities::detect(|name| environment.var(name));
        let session = Session::on(machine, Theme::new(capabilities));
        if let Err(error) = (self.run)(session) {
            self.error = Some(error.to_string());
        }
    }
}

/// The menu program on `system` with the configuration `conf`, the menu
/// run by `run`: its exit status, or why the menu could not run.
fn menu(system: System<'_>, conf: &Conf, run: Run, countdown: Countdown) -> Result<i32, String> {
    let options = ZbmOptions::from_conf(conf);
    let base = preinit::base(conf);
    let mut menu = Terminal {
        system,
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
    // trap '' SIGINT; the USR1 trap of a takeover
    let _ = zbm_sys::ignore_signal(Signal::Interrupt);
    let _ = zbm_sys::record_signals(&[Signal::User1, Signal::Terminate, Signal::Hangup]);
    on_console(io, |system, conf, io| {
        menu(system, conf, on_terminal, countdown).unwrap_or_else(|error| {
            let _ = writeln!(io.stderr, "zfsbootmenu: {error}");
            zbm_startup::program::menu::FAILED
        })
    })
}

#[cfg(test)]
mod tests;
