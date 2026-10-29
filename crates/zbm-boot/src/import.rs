//! Pool import in the start-up program: the stage of [`zbm_pool`], with the
//! console prompts and the recovery shell of the start-up program as its
//! [`Operator`].

use zbm_platform::{Context, System};
use zbm_pool::{Late, Operator, RetryDelay};
use zbm_state::State;
use zbm_types::PoolName;

pub use zbm_pool::import::{Access, Import, Rewind, import_pool, imported_pools};
pub use zbm_pool::text::{tab_fields, tab_lines};
pub use zbm_pool::{ImportSettings, Imported, choose_bootfs};

use crate::prompt::{Answer, TimedPrompt, colorize, retry_delay};
use crate::recovery::Recovery;

/// The operator of the import stage: the countdown prompts of the start-up
/// program, and its recovery shell and late LUKS unlocking.
pub struct Prompts<'s, 'a> {
    system: &'s System<'a>,
    recovery: &'s dyn Recovery,
}

impl<'s, 'a> Prompts<'s, 'a> {
    /// The prompts of `system`, with `recovery`.
    pub fn new(system: &'s System<'a>, recovery: &'s dyn Recovery) -> Prompts<'s, 'a> {
        Prompts { system, recovery }
    }
}

impl Operator for Prompts<'_, '_> {
    fn unlock_late(&self) -> Late {
        if self.recovery.unlock_late().is_ok() {
            Late::Unlocked
        } else {
            Late::Nothing
        }
    }

    fn retry(&self, target: Option<&PoolName>, delay: RetryDelay) -> Answer {
        let pool = colorize("magenta", target.map_or("pool", PoolName::as_str));
        TimedPrompt::new()
            .delay(retry_delay(delay))
            .prompt(move |seconds| {
                let seconds = colorize("yellow", &format!("{seconds:02}"));
                format!("Unable to import {pool}, retrying in {seconds} seconds")
            })
            .on_return("to retry immediately")
            .on_escape("for a recovery shell")
            .run(self.system)
    }

    fn unsupported_features(&self, advice: &str) {
        TimedPrompt::new()
            .message(colorize("red", "Unsupported features detected"))
            .message(colorize("red", advice))
            .run(self.system);
    }

    fn emergency_shell(&self, message: &str) {
        self.recovery.emergency_shell(message);
    }
}

/// The start-up stage `init.d/50-import-pools`, with the start-up program's
/// prompts and `recovery`.
pub fn stage(
    system: &System<'_>,
    state: &State<'_>,
    settings: &ImportSettings,
    recovery: &dyn Recovery,
) -> Imported {
    let context = Context::new(*system);
    zbm_pool::stage(&context, state, settings, &Prompts::new(system, recovery))
}

#[cfg(test)]
mod tests;
