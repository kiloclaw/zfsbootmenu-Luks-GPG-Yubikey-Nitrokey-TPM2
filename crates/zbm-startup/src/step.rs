//! The start-up program as a list of steps.
//!
//! Each start-up stage is a [`Step`]: it knows which [`Stage`] it is and runs
//! against a [`StepContext`]. [`run_steps`] runs them in order, skips the
//! stages an earlier run finished (unless forced), and records each stage
//! when it ends, as the legacy `zfsbootmenu-init` does with its `init.d`
//! scripts.
//!
//! ```
//! use zbm_startup::step::{Rerun, Step, StepContext, run_steps};
//! use zbm_state::State;
//! use zbm_testkit::fakes::FakeSystem;
//! use zbm_types::Stage;
//!
//! struct Hello;
//!
//! impl Step for Hello {
//!     fn stage(&self) -> Stage {
//!         Stage::HostId
//!     }
//!     fn run(&self, _context: &mut StepContext<'_, '_>) {}
//! }
//!
//! let fake = FakeSystem::new();
//! fake.files.dir("/zfsbootmenu");
//! let system = fake.system();
//! let state = State::new(&fake.files, "/zfsbootmenu");
//! let mut front = zbm_startup::step::NoRecovery;
//! let mut context = StepContext::new(&system, &state, &mut front);
//! let steps: Vec<Box<dyn Step>> = vec![Box::new(Hello)];
//! run_steps(&steps, &mut context, Rerun::Pending);
//! assert_eq!(state.finished_stages(), ["20-hostid"]);
//! ```

use zbm_app::model::{Event, StageState};
use zbm_boot::error::BootError;
use zbm_boot::recovery::Recovery;
use zbm_platform::System;
use zbm_state::State;
use zbm_types::{DatasetName, Stage};

use crate::front::Front;

/// What a step runs with: the system, the state directory, the front end,
/// and what earlier steps found.
pub struct StepContext<'c, 'a> {
    /// The system the stages' programs run on.
    pub system: &'c System<'a>,
    /// The state directory.
    pub state: &'c State<'a>,
    /// What the stages need from the person at the console: prompts, the
    /// recovery shell, LUKS unlocking, and where their events go.
    pub front: &'c mut dyn Front,
    /// The default boot environment, once pool import found one.
    pub bootfs: Option<DatasetName>,
}

impl<'c, 'a> StepContext<'c, 'a> {
    /// A context with nothing found yet.
    pub fn new(
        system: &'c System<'a>,
        state: &'c State<'a>,
        front: &'c mut dyn Front,
    ) -> StepContext<'c, 'a> {
        StepContext {
            system,
            state,
            front,
            bootfs: None,
        }
    }
}

/// One start-up stage.
pub trait Step {
    /// The stage this step is.
    fn stage(&self) -> Stage;

    /// Runs the stage. A stage does not fail: what goes wrong is logged, or
    /// handled in the recovery shell.
    fn run(&self, context: &mut StepContext<'_, '_>);
}

/// Which steps run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Rerun {
    /// Only those that no earlier run finished.
    #[default]
    Pending,
    /// Every one (`ZFSBOOTMENU_FORCE_INIT`).
    All,
}

/// Runs `steps` in order, skipping finished ones unless `rerun` is
/// [`Rerun::All`], and records each stage as finished.
pub fn run_steps(steps: &[Box<dyn Step + '_>], context: &mut StepContext<'_, '_>, rerun: Rerun) {
    for step in steps {
        let stage = step.stage();
        let finished = context
            .state
            .finished_stages()
            .iter()
            .any(|done| done == stage.as_str());
        if finished && rerun == Rerun::Pending {
            continue;
        }
        zbm_trace::info!("running init stage {stage}");
        let name = zbm_app::stage_name(stage);
        context.front.send(Event::StageStarted(name.to_owned()));
        step.run(context);
        context.front.send(Event::StageFinished {
            stage: name.to_owned(),
            state: StageState::Done,
        });
        if let Err(error) = context.state.finish_stage(stage.as_str()) {
            zbm_trace::error!("{}", error.to_string());
        }
    }
}

/// Recovery for code that runs without a console, such as tests and
/// examples: nothing to unlock late, and no shell to open.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRecovery;

impl Recovery for NoRecovery {
    fn unlock_late(&mut self) -> Result<(), BootError> {
        Err(BootError::NoCryptsetup)
    }

    fn emergency_shell(&mut self, message: &str) {
        zbm_trace::error!("no recovery shell for: {message}");
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeSystem;

    use super::*;

    /// A step that does nothing.
    struct Nothing(Stage);

    impl Step for Nothing {
        fn stage(&self) -> Stage {
            self.0
        }

        fn run(&self, _context: &mut StepContext<'_, '_>) {}
    }

    #[test]
    fn each_stage_sends_its_start_and_end() {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu");
        let system = fake.system();
        let state = State::new(&fake.files, "/zfsbootmenu");
        let mut front = crate::testing::Recording::default();
        let steps: Vec<Box<dyn Step>> = vec![
            Box::new(Nothing(Stage::Modules)),
            Box::new(Nothing(Stage::ImportPools)),
        ];

        let mut context = StepContext::new(&system, &state, &mut front);
        run_steps(&steps, &mut context, Rerun::Pending);

        assert_eq!(
            front.events,
            [
                Event::StageStarted("Loading kernel modules".into()),
                Event::StageFinished {
                    stage: "Loading kernel modules".into(),
                    state: StageState::Done,
                },
                Event::StageStarted("Importing pools".into()),
                Event::StageFinished {
                    stage: "Importing pools".into(),
                    state: StageState::Done,
                },
            ]
        );
    }
}
