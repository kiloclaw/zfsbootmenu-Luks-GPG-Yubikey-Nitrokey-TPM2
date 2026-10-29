//! The menu program, `/bin/zfsbootmenu`: what runs around the menu, as the
//! legacy `bin/zfsbootmenu` does before it draws the list. It waits for
//! start-up, takes the menu over from another instance, holds the `active`
//! lock, runs the `setup.d` hooks, then hands over to the menu of the front
//! end ([`Menu`]). See "Concurrent instances and takeover" in
//! `crates/DESIGN.md`.

use std::path::Path;

use zbm_boot::boot::BootSettings;
use zbm_boot::hooks;
use zbm_boot::prompt::{Answer, TimedPrompt, colorize, tput};
use zbm_boot::recovery::Recovery;
use zbm_platform::Signal;
use zbm_platform::System;
use zbm_platform::process::deepest_child;
use zbm_types::HookStage;

use super::{Modes, Startup};

/// Where the image installs the menu program.
pub const MENU: &str = "/bin/zfsbootmenu";

/// The exit status of the menu program when it could not read the machine
/// or use the terminal.
pub const FAILED: i32 = 1;

/// Whether the image has the menu program.
pub fn has_menu(system: &System<'_>) -> bool {
    system
        .files
        .metadata(Path::new(MENU))
        .is_ok_and(|metadata| metadata.is_executable())
}

/// The menu in the start-up program's own process, the session (ROADMAP.md,
/// PM9): it goes on with the terminal and the model start-up showed its
/// stages on, and the recovery loop runs it each round.
pub trait SessionMenu {
    /// Runs the menu, around what the menu program does first
    /// ([`Startup::menu`]), counting down to booting the default boot
    /// environment first when `countdown`, until it ends.
    fn run(&mut self, startup: &Startup<'_>, countdown: bool);
}

/// The menu of a front end.
pub trait Menu {
    /// Runs the menu until it ends: it boots with `settings`, and opens the
    /// recovery shell with `recovery`.
    fn show(&mut self, settings: &BootSettings, recovery: &dyn Recovery);
}

impl Startup<'_> {
    /// Runs the menu program around `menu`; the exit status of
    /// `bin/zfsbootmenu`.
    pub fn menu(&self, menu: &mut dyn Menu) -> i32 {
        let system = &self.system;
        if !system.files.exists(Path::new("/dev/zfs")) {
            self.shell()
                .emergency_shell("/dev/zfs missing, check that kernel modules are loaded");
        }
        let _ = self.state.create();
        if !self.is_initialized_now() {
            return 0;
        }
        if self.state.is_active() {
            self.take_over();
        }
        // Held until the menu ends; the kernel lets go of it if this process
        // dies, so no lock is left behind.
        let _active = loop {
            if !self.has_waited_for_the_other() {
                return 0;
            }
            match self.state.take_active(system.processes.id()) {
                Ok(Some(active)) => break Some(active),
                // Another menu took it meanwhile: wait for that one.
                Ok(None) => {}
                Err(error) => {
                    zbm_trace::error!("{error}"; %error);
                    break None;
                }
            }
        };
        zbm_trace::debug!("holding {base}/active.lock"; base = %self.state.base().display());

        // The variables of `/etc/zfsbootmenu.conf`, as the legacy menu's
        // hooks got them from the script that sourced it.
        let mut env = self.conf_vars.clone();
        if let Some(bootfs) = self.state.bootfs() {
            zbm_trace::debug!("setting BOOTFS to {bootfs}"; %bootfs);
            env.push(("BOOTFS".to_owned(), bootfs));
        }
        tput(system, &["clear"]);
        let pairs: Vec<(&str, &str)> = env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        hooks::run_stage(system, &self.hooks, HookStage::Setup, false, &pairs);
        // Clear the screen before a possible passphrase prompt.
        tput(system, &["clear"]);

        let settings = BootSettings {
            set_hostid: self.options.set_hostid,
            hooks: self.hooks.clone(),
            env,
            save_logs: self.save_logs(),
            command_line: None,
            import_policy: self.options.import_policy,
        };
        menu.show(&settings, &self.shell());
        0
    }

    /// Waits until start-up is over, running it when the user asks; whether
    /// it is, rather than the user aborting.
    fn is_initialized_now(&self) -> bool {
        let system = &self.system;
        while !self.state.is_initialized() {
            let answer = TimedPrompt::new()
                .delay(-1)
                .message(colorize(
                    "red",
                    "ZFSBootMenu must be initialized to continue",
                ))
                .on_return("initialize")
                .on_escape("abort     ")
                .run(system);
            tput(system, &["cnorm"]);
            tput(system, &["clear"]);
            if answer == Answer::Escape {
                zbm_trace::debug!("aborted while waiting for initialization");
                return false;
            }
            if !self.state.is_initialized() {
                self.run(Modes::default(), None);
            }
        }
        true
    }

    /// Asks the menu holding the lock to end: `SIGUSR1` to it, then `SIGTERM`
    /// to its deepest child, so that what it waits for returns.
    fn take_over(&self) {
        let Some(pid) = self.state.active_pid() else {
            return;
        };
        let processes = self.system.processes;
        zbm_trace::info!("Stopping active zfsbootmenu with a PID of {pid}"; %pid);
        zbm_trace::debug!("sending USR1 to {pid}"; %pid);
        if processes.signal(pid, Signal::User1).is_err() {
            zbm_trace::info!("no process {pid}: waiting for the lock to be let go"; %pid);
            return;
        }
        let child = deepest_child(self.system.files, pid);
        zbm_trace::debug!("killing child process {child}"; %child);
        let _ = processes.signal(child, Signal::Terminate);
    }

    /// Waits while another instance holds the lock; whether it let go,
    /// rather than the user cancelling.
    fn has_waited_for_the_other(&self) -> bool {
        let system = &self.system;
        while self.state.is_active() {
            let answer = TimedPrompt::new()
                .delay(1)
                .prompt(|_| "Waiting for other ZFSBootMenu instance to terminate".to_owned())
                .on_escape("to cancel")
                .run(system);
            if answer == Answer::Escape {
                let base = self.state.base().display();
                zbm_trace::debug!("exited while waiting to own {base}/active"; %base);
                tput(system, &["cnorm"]);
                tput(system, &["clear"]);
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests;
