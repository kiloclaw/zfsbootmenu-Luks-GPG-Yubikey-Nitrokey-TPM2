//! The session's side of a remote menu (ROADMAP.md, PM10; ADR 0002): a
//! menu in a process of its own, as `zfsbootmenu` over SSH, asks the
//! session's socket for one (`zbm_app::session::Request::Remote`), hands it
//! its terminal, and the session runs the menu program around it on that
//! terminal (the lock, the setup hooks, the prompts), serving the menu's
//! requests (`zbm_app::remote`) instead of drawing. The actions that take
//! the console run here, on the menu's terminal: their programs read and
//! write it ([`TerminalRunner`]), in sessions of their own.

use std::fs::File;
use std::io;
use std::path::Path;

use zbm_app::model::Action;
use zbm_app::remote::{self, Served};
use zbm_app::service::MachineService;
use zbm_app::session::Handover;
use zbm_app::{App, Application as _, BootSettings, Recovery, Runtime, Through};
use zbm_config::conf::Conf;
use zbm_config::options::ZbmOptions;
use zbm_platform::event::CancelToken;
use zbm_platform::{Context, Runner, System, Terminal, TerminalRunner};
use zbm_startup::preinit;
use zbm_startup::program::Startup;
use zbm_startup::program::menu::Menu;
use zbm_sys::socket::Connection;

/// Why the session tells a remote menu it ended without serving it: the
/// menu program stopped before its menu, as when start-up was not over
/// and the user did not have it run.
const NOT_SERVED: &str = "the menu program ended before its menu";

/// What a remote menu is served with: the session's whole system, whose
/// console and programs go to the menu's terminal instead, and its
/// configuration.
pub(crate) struct Remotes<'s, 'a> {
    /// The session's system.
    pub system: System<'a>,
    /// The image's configuration.
    pub conf: &'s Conf,
    /// What the menus share with the session's socket.
    pub handover: &'s Handover,
    /// Cancelled when the session ends.
    pub until: &'s CancelToken,
    /// What runs the programs of a remote menu's actions: on its terminal
    /// ([`TerminalRunner`]) when `None`, as in the image; tests give their
    /// fake.
    pub runner: Option<&'a dyn Runner>,
}

impl Remotes<'_, '_> {
    /// Serves the menu that asked for one on `peer`, until it ends.
    pub fn serve(&self, peer: &Connection) {
        let _span = tracing::info_span!("remote").entered();
        if let Err(error) = self.serve_attached(peer) {
            zbm_trace::warn!("the remote menu ended: {error}"; %error);
        }
    }

    /// Takes the menu's terminal, and runs the menu program on it.
    fn serve_attached(&self, peer: &Connection) -> io::Result<()> {
        let attached = remote::attach(peer)?;
        let on_terminal;
        let runner: &dyn Runner = if let Some(runner) = self.runner {
            runner
        } else {
            let (input, output) = (attached.input.try_clone()?, attached.output.try_clone()?);
            on_terminal = TerminalRunner::new(input, output);
            &on_terminal
        };
        let terminal = Terminal::new(File::from(attached.input), File::from(attached.output));
        let system = System {
            runner,
            console: &terminal,
            ..self.system
        };
        zbm_trace::info!("a remote menu attached");
        let mut menu = RemoteMenu {
            peer,
            system,
            conf: self.conf,
            handover: self.handover,
            until: self.until,
            served: None,
        };
        self.handover.enter();
        let _ = Startup::new(system, self.conf).menu(&mut menu);
        // The menu let go of the `active` lock.
        self.handover.leave();
        match menu.served {
            Some(result) => result,
            None => remote::ToMenu::Refused(NOT_SERVED.to_owned()).send(peer),
        }
    }
}

/// The menu program's menu, served to the menu on `peer`.
struct RemoteMenu<'s, 'a> {
    peer: &'s Connection,
    /// The system on the menu's terminal.
    system: System<'a>,
    conf: &'s Conf,
    handover: &'s Handover,
    until: &'s CancelToken,
    /// How serving the menu ended, once it did.
    served: Option<io::Result<()>>,
}

impl Menu for RemoteMenu<'_, '_> {
    fn show(&mut self, settings: &BootSettings, recovery: &dyn Recovery) {
        let options = ZbmOptions::from_conf(self.conf);
        let base = preinit::base(self.conf);
        let runtime = Runtime::new(options, &base).with_boot(settings.clone());
        let app = App::new(Context::new(self.system), &runtime);
        let system = self.system;
        let console = |action: &Action| app.apply(action, Through::Console(system, recovery));
        let (handover, until) = (self.handover, self.until);
        let stop = || handover.is_asked() || until.is_cancelled();
        let served = Served {
            service: MachineService::new(app, system),
            console: &console,
            stop: &stop,
        };
        self.served = Some(remote::serve(self.peer, &served));
    }

    /// The session's own: it would ask itself.
    fn session_socket(&self) -> Option<&Path> {
        None
    }
}

#[cfg(test)]
mod tests;
