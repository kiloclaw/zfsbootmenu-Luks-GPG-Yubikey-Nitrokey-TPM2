//! What the start-up stages need from the person at the console, through
//! the program's front end ([`Front`]): the plain console of the legacy
//! start-up program ([`ConsoleFront`]), or the start-up view of the menu,
//! which `zbm` builds on `zbm-tui` (REFACTOR-ROADMAP.md, R5 step 4).

use zbm_app::model::Event;
use zbm_boot::import::{self, ImportSettings, Imported};
use zbm_boot::prompt::{Answer, tput};
use zbm_boot::recovery::Recovery;
use zbm_platform::System;
use zbm_state::State;

use crate::shell::{Attempt, Shell, zluks_unlock};
use crate::stages::devices;
use crate::step::NoRecovery;

/// The front end of the start-up stages.
pub trait Front {
    /// A stage started or ended, for a front end that shows the stages.
    fn send(&mut self, _event: Event) {}

    /// The recovery shell and the late unlocking of LUKS volumes.
    fn recovery(&mut self) -> &mut dyn Recovery;

    /// Unlocks the LUKS volumes, the stage `45-luks-unlock`, with
    /// `system`'s programs.
    fn unlock_luks(&mut self, system: &System<'_>);

    /// Says that devices of `zbm.wait_for` are missing and counts `delay`
    /// seconds down: [`Answer::Continue`] looks again, [`Answer::Escape`]
    /// stops waiting.
    fn devices_missing(&mut self, system: &System<'_>, delay: i64) -> Answer;

    /// Runs `run`, such as hooks, with the console: a front end that draws
    /// on it hands it over, and gives `run` its own system, whose programs
    /// have the console.
    fn with_console(&mut self, system: &System<'_>, run: &mut dyn FnMut(&System<'_>)) {
        run(system);
    }

    /// The stage `50-import-pools`, with the front end's prompts and
    /// recovery shell.
    fn import(
        &mut self,
        system: &System<'_>,
        state: &State<'_>,
        settings: &ImportSettings,
    ) -> Imported;

    /// The stages are over: a front end that draws on the console gives it
    /// back, for the countdown and the menu.
    fn finish(&mut self) {}
}

/// The plain console: the prompts of the legacy start-up program, `zluks
/// unlock` in a process of its own, and the recovery shell.
pub struct ConsoleFront<'s, 'a> {
    shell: Shell<'s, 'a>,
}

impl<'s, 'a> ConsoleFront<'s, 'a> {
    /// The console of `system`, with the state directory `state`.
    pub fn new(system: System<'a>, state: &'s State<'a>) -> ConsoleFront<'s, 'a> {
        ConsoleFront {
            shell: Shell::new(system, state),
        }
    }
}

impl Front for ConsoleFront<'_, '_> {
    fn recovery(&mut self) -> &mut dyn Recovery {
        &mut self.shell
    }

    /// `zluks unlock` in a process of its own, as the legacy stage unlocks
    /// in a subshell, when the image has `cryptsetup`; a hook may have
    /// provided key files or unlocked volumes itself.
    fn unlock_luks(&mut self, system: &System<'_>) {
        if system.command_exists("cryptsetup") {
            let _ = zluks_unlock(system, Attempt::First);
            tput(system, &["clear"]);
        }
    }

    fn devices_missing(&mut self, system: &System<'_>, delay: i64) -> Answer {
        devices::missing_prompt(system, delay)
    }

    fn import(
        &mut self,
        system: &System<'_>,
        state: &State<'_>,
        settings: &ImportSettings,
    ) -> Imported {
        import::stage(system, state, settings, &mut self.shell)
    }
}

/// For code that runs without a console, such as tests and examples:
/// nothing is unlocked, nobody waits for devices, and there is no shell.
impl Front for NoRecovery {
    fn recovery(&mut self) -> &mut dyn Recovery {
        self
    }

    fn unlock_luks(&mut self, _system: &System<'_>) {}

    fn devices_missing(&mut self, _system: &System<'_>, _delay: i64) -> Answer {
        Answer::Escape
    }

    fn import(
        &mut self,
        system: &System<'_>,
        state: &State<'_>,
        settings: &ImportSettings,
    ) -> Imported {
        import::stage(system, state, settings, self)
    }
}

#[cfg(test)]
mod tests;
