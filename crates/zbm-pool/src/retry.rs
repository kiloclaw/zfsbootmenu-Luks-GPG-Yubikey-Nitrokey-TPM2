//! The retry loop of the import stage, as a state machine.
//!
//! [`ImportLoop`] holds what the loop knows between attempts: the pool it
//! tries (the preferred one, or every pool) and how many attempts failed. The
//! stage tells it what an attempt found, and it answers what to do next. The
//! transitions do no I/O, so every path of the loop is a unit test.
//!
//! ```
//! use zbm_pool::retry::{AfterImport, Found, ImportLoop, Next};
//! use zbm_pool::{ImportSettings, PoolRequirement};
//! use zbm_types::PoolName;
//!
//! let settings = ImportSettings {
//!     prefer_pool: "tank".parse::<PoolName>().ok(),
//!     requirement: PoolRequirement::Optional,
//!     ..ImportSettings::default()
//! };
//! let mut retry = ImportLoop::new(&settings);
//! // The preferred pool was imported: now import the others.
//! assert_eq!(retry.imported(Found::Pools), AfterImport::Next(Next::Again));
//! assert_eq!(retry.target(), None);
//! assert_eq!(retry.imported(Found::Pools), AfterImport::Next(Next::Done));
//! ```

use zbm_types::{ImportPolicy, PoolName, PoolRequirement};

use crate::settings::ImportSettings;

/// What an import attempt found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Found {
    /// At least one pool is imported.
    Pools,
    /// No pool is imported.
    Nothing,
}

/// Whether host ID matching imported a pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Matched {
    /// A pool was imported with the host ID of its last host.
    Pool,
    /// No pool could be.
    Nothing,
}

/// What the stage does after an import attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AfterImport {
    /// Try to import a pool of another host with that host's ID, then tell
    /// the loop with [`ImportLoop::matched`].
    MatchHostid,
    /// What to do next.
    Next(Next),
}

/// What the stage does next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Next {
    /// Stop: the pools that should be imported are.
    Done,
    /// Import again at once, with the loop's (possibly changed) target.
    Again,
    /// Attempt `attempt` failed: unlock LUKS volumes that appeared late, or
    /// else ask the user whether to wait and retry or to open the recovery
    /// shell; then import again.
    Failed {
        /// The number of failed attempts, from 1.
        attempt: u64,
    },
}

/// The state of the import loop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportLoop {
    policy: ImportPolicy,
    requirement: PoolRequirement,
    target: Option<PoolName>,
    failed: u64,
}

impl ImportLoop {
    /// The loop at its start: it tries the preferred pool, if there is one.
    pub fn new(settings: &ImportSettings) -> ImportLoop {
        ImportLoop {
            policy: settings.policy,
            requirement: settings.requirement,
            target: settings.prefer_pool.clone(),
            failed: 0,
        }
    }

    /// The pool the next attempt imports; `None` for every pool.
    pub fn target(&self) -> Option<&PoolName> {
        self.target.as_ref()
    }

    /// The number of failed attempts so far.
    pub fn failures(&self) -> u64 {
        self.failed
    }

    /// The transition after an import attempt that found `found`.
    pub fn imported(&mut self, found: Found) -> AfterImport {
        match found {
            Found::Pools if self.requirement == PoolRequirement::Only => {
                AfterImport::Next(Next::Done)
            }
            // A single pool was imported: try the others too.
            Found::Pools if self.target.take().is_some() => AfterImport::Next(Next::Again),
            Found::Pools => AfterImport::Next(Next::Done),
            Found::Nothing if self.policy == ImportPolicy::Hostid => AfterImport::MatchHostid,
            Found::Nothing => AfterImport::Next(self.not_found()),
        }
    }

    /// The transition after host ID matching, which
    /// [`AfterImport::MatchHostid`] asked for.
    pub fn matched(&mut self, matched: Matched) -> Next {
        match matched {
            Matched::Pool if self.target.is_some() && self.requirement == PoolRequirement::Only => {
                Next::Done
            }
            // Pick up the other pools of this host ID.
            Matched::Pool => {
                self.target = None;
                Next::Again
            }
            Matched::Nothing => self.not_found(),
        }
    }

    /// Nothing was imported: a preferred pool that is not required gives way
    /// to every pool; otherwise the attempt failed.
    fn not_found(&mut self) -> Next {
        if self.target.is_some() && self.requirement == PoolRequirement::Optional {
            self.target = None;
            return Next::Again;
        }
        self.failed += 1;
        Next::Failed {
            attempt: self.failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn start(policy: ImportPolicy, prefer: &str, requirement: PoolRequirement) -> ImportLoop {
        ImportLoop::new(&ImportSettings {
            policy,
            prefer_pool: prefer.parse::<PoolName>().ok(),
            requirement,
            ..ImportSettings::default()
        })
    }

    /// One case of the table: the loop's start, what attempts find in
    /// order, and what the loop answers to each.
    struct Case {
        policy: ImportPolicy,
        prefer: &'static str,
        requirement: PoolRequirement,
        steps: &'static [(Step, Answer)],
    }

    #[derive(Clone, Copy, Debug)]
    enum Step {
        Import(Found),
        Match(Matched),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Answer {
        Import(AfterImport),
        Match(Next),
    }

    const STRICT: ImportPolicy = ImportPolicy::Strict;
    const HOSTID: ImportPolicy = ImportPolicy::Hostid;
    const OPTIONAL: PoolRequirement = PoolRequirement::Optional;
    const REQUIRED: PoolRequirement = PoolRequirement::Required;
    const ONLY: PoolRequirement = PoolRequirement::Only;
    const POOLS: Step = Step::Import(Found::Pools);
    const NOTHING: Step = Step::Import(Found::Nothing);
    const DONE: Answer = Answer::Import(AfterImport::Next(Next::Done));
    const AGAIN: Answer = Answer::Import(AfterImport::Next(Next::Again));
    const MATCH: Answer = Answer::Import(AfterImport::MatchHostid);

    const fn failed(attempt: u64) -> Answer {
        Answer::Import(AfterImport::Next(Next::Failed { attempt }))
    }

    const CASES: &[Case] = &[
        // Every pool at once.
        Case {
            policy: STRICT,
            prefer: "",
            requirement: OPTIONAL,
            steps: &[(POOLS, DONE)],
        },
        Case {
            policy: STRICT,
            prefer: "",
            requirement: OPTIONAL,
            steps: &[(NOTHING, failed(1)), (NOTHING, failed(2)), (POOLS, DONE)],
        },
        // A preferred pool, then the others.
        Case {
            policy: STRICT,
            prefer: "tank",
            requirement: OPTIONAL,
            steps: &[(POOLS, AGAIN), (POOLS, DONE)],
        },
        // A preferred pool that is missing gives way when not required...
        Case {
            policy: STRICT,
            prefer: "tank",
            requirement: OPTIONAL,
            steps: &[(NOTHING, AGAIN), (NOTHING, failed(1))],
        },
        // ...and is retried when required.
        Case {
            policy: STRICT,
            prefer: "tank",
            requirement: REQUIRED,
            steps: &[(NOTHING, failed(1)), (POOLS, AGAIN), (POOLS, DONE)],
        },
        Case {
            policy: STRICT,
            prefer: "tank",
            requirement: ONLY,
            steps: &[(NOTHING, failed(1)), (POOLS, DONE)],
        },
        // Host ID matching is asked for when nothing was imported.
        Case {
            policy: HOSTID,
            prefer: "",
            requirement: OPTIONAL,
            steps: &[
                (NOTHING, MATCH),
                (
                    Step::Match(Matched::Nothing),
                    Answer::Match(Next::Failed { attempt: 1 }),
                ),
                (NOTHING, MATCH),
                (Step::Match(Matched::Pool), Answer::Match(Next::Again)),
                (POOLS, DONE),
            ],
        },
    ];

    #[test]
    fn every_path_of_the_table() {
        for (index, case) in CASES.iter().enumerate() {
            let mut retry = start(case.policy, case.prefer, case.requirement);
            for (step, (input, expected)) in case.steps.iter().enumerate() {
                let answer = match input {
                    Step::Import(found) => Answer::Import(retry.imported(*found)),
                    Step::Match(matched) => Answer::Match(retry.matched(*matched)),
                };
                assert_eq!(answer, *expected, "case {index}, step {step}");
            }
        }
    }

    #[test]
    fn host_id_matching_picks_up_the_other_pools_unless_only_one_is_wanted() {
        let mut retry = start(HOSTID, "", OPTIONAL);
        assert_eq!(retry.imported(Found::Nothing), AfterImport::MatchHostid);
        assert_eq!(retry.matched(Matched::Pool), Next::Again);
        assert_eq!(retry.target(), None);

        let mut only = start(HOSTID, "tank", ONLY);
        assert_eq!(only.imported(Found::Nothing), AfterImport::MatchHostid);
        assert_eq!(only.matched(Matched::Pool), Next::Done);

        let mut required = start(HOSTID, "tank", REQUIRED);
        assert_eq!(required.imported(Found::Nothing), AfterImport::MatchHostid);
        assert_eq!(required.matched(Matched::Pool), Next::Again);
        assert_eq!(required.target(), None);

        let mut missing = start(HOSTID, "tank", REQUIRED);
        assert_eq!(missing.imported(Found::Nothing), AfterImport::MatchHostid);
        assert_eq!(
            missing.matched(Matched::Nothing),
            Next::Failed { attempt: 1 }
        );
        assert_eq!(missing.failures(), 1);
        assert_eq!(missing.target().map(PoolName::as_str), Some("tank"));
        assert_eq!(missing.imported(Found::Nothing), AfterImport::MatchHostid);
        assert_eq!(
            missing.matched(Matched::Nothing),
            Next::Failed { attempt: 2 }
        );

        let mut optional = start(HOSTID, "tank", OPTIONAL);
        assert_eq!(optional.imported(Found::Nothing), AfterImport::MatchHostid);
        assert_eq!(optional.matched(Matched::Nothing), Next::Again);
        assert_eq!(optional.target(), None);
    }
}
