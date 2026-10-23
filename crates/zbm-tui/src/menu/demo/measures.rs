//! The measures of the core tasks (`docs/dev/menu-design/users-and-tasks.md`,
//! *Measures*): each task's shortest path on the demo machine, counted in key
//! presses less the text typed, against its budget. A change to the menu that
//! makes a task longer fails here; one that makes it shorter updates
//! `measured` and the table in the design.

use pretty_assertions::assert_eq;
use ratatui::crossterm::event::KeyCode;

use super::{DemoOptions, Session};
use crate::menu::caps::Capabilities;
use crate::menu::theme::Theme;
use crate::menu::world::{Action, VolumeState};

/// A step of a task.
#[derive(Clone, Copy)]
enum Step {
    /// A key press, counted.
    Key(KeyCode),
    /// Text typed: a secret, a name, filter letters; not counted.
    Text(&'static str),
}

/// A core task.
struct Task {
    name: &'static str,
    options: DemoOptions,
    steps: &'static [Step],
    /// The keys it takes now.
    measured: u32,
    /// The budget of the design.
    budget: u32,
    /// Whether the task got done.
    done: fn(&Session) -> bool,
}

fn options(locked: bool, foreign: bool) -> DemoOptions {
    DemoOptions {
        locked,
        foreign,
        ..DemoOptions::default()
    }
}

fn boots(session: &Session, environment: &str) -> bool {
    matches!(
        session.actions.as_slice(),
        [Action::Boot { environment: e, .. }] if e.as_str() == environment
    )
}

/// The core tasks T1 to T4.
fn first_tasks() -> Vec<Task> {
    use KeyCode::{Char, Down, Enter};
    use Step::{Key, Text};
    vec![
        Task {
            name: "T1 boot the default",
            options: options(false, false),
            steps: &[],
            measured: 0,
            budget: 0,
            done: |s| boots(s, "zroot/ROOT/void"),
        },
        Task {
            name: "T2 boot another environment",
            options: options(false, false),
            steps: &[Key(Down), Key(Enter)],
            measured: 2,
            budget: 4,
            done: |s| boots(s, "tank/ROOT/arch"),
        },
        Task {
            name: "T2 boot another environment, by filter",
            options: options(false, false),
            steps: &[Key(Char('/')), Text("ubu"), Key(Enter)],
            measured: 2,
            budget: 4,
            done: |s| boots(s, "zroot/ROOT/ubuntu"),
        },
        Task {
            name: "T3 boot another kernel",
            options: options(false, false),
            steps: &[Key(Char('k')), Key(Down), Key(Enter)],
            measured: 3,
            budget: 5,
            done: |s| {
                matches!(
                    s.actions.as_slice(),
                    [Action::Boot { kernel: Some(k), .. }] if k == "6.12.8_1"
                )
            },
        },
        Task {
            name: "T4 boot the newest snapshot",
            options: options(false, false),
            steps: &[Key(Char('s')), Key(Char('o')), Key(Char('b'))],
            measured: 3,
            budget: 6,
            done: |s| matches!(s.actions.as_slice(), [Action::BootSnapshot(_)]),
        },
        Task {
            name: "T4 roll back to the newest snapshot",
            options: options(false, false),
            steps: &[
                Key(Char('s')),
                Key(Char('o')),
                Key(Char('r')),
                Text("void"),
                Key(Enter),
            ],
            measured: 4,
            budget: 6,
            done: |s| matches!(s.actions.as_slice(), [Action::Rollback { .. }]),
        },
    ]
}

/// The core tasks T5 to T8.
fn more_tasks() -> Vec<Task> {
    use KeyCode::{Char, Down, Enter};
    use Step::{Key, Text};
    vec![
        Task {
            name: "T5 unlock a volume, from its prompt",
            options: options(true, false),
            steps: &[Text("1234"), Key(Enter)],
            measured: 1,
            budget: 1,
            done: |s| {
                let volumes = &s.model().snapshot.volumes;
                volumes
                    .first()
                    .is_some_and(|v| v.state == VolumeState::Open)
            },
        },
        Task {
            name: "T6 edit the command line once",
            options: options(false, false),
            steps: &[Key(Char('e')), Text(" single"), Key(Enter)],
            measured: 2,
            budget: 3,
            done: |s| {
                matches!(
                    s.actions.as_slice(),
                    [Action::Boot { command_line: Some(line), .. }] if line.ends_with(" single")
                )
            },
        },
        Task {
            name: "T7 import a foreign pool, from the problem",
            options: options(false, true),
            steps: &[Key(Down), Key(Enter)],
            measured: 2,
            budget: 2,
            done: |s| s.model().snapshot.pools.iter().all(|pool| pool.imported),
        },
        Task {
            name: "T8 restore a file from the newest snapshot",
            options: options(false, false),
            steps: &[
                Key(Char('s')),
                Key(Char('o')),
                Key(Char('f')),
                Key(Enter),
                Text("void"),
                Key(Enter),
            ],
            measured: 5,
            budget: 6,
            done: |s| {
                s.actions
                    .iter()
                    .any(|a| matches!(a, Action::RestoreFiles { .. }))
            },
        },
    ]
}

/// Runs `task` from power-on; returns the session and the keys counted.
fn run(task: &Task) -> (Session, u32) {
    let mut session = Session::new(&task.options, Theme::new(Capabilities::FULL));
    session.start();
    let mut typed = 0;
    for step in task.steps {
        match *step {
            Step::Key(code) => session.key(code),
            Step::Text(text) => {
                session.type_text(text);
                typed += u32::try_from(text.chars().count()).unwrap();
            }
        }
    }
    session.start();
    if task.steps.is_empty() {
        for _ in 0..10 {
            session.tick(1000);
        }
    }
    let keys = session.keys() - typed;
    (session, keys)
}

#[test]
fn every_core_task_is_done_within_its_budget() {
    for task in first_tasks().into_iter().chain(more_tasks()) {
        let (session, keys) = run(&task);

        assert!((task.done)(&session), "{}: not done", task.name);
        assert_eq!((task.name, keys), (task.name, task.measured));
        assert!(task.measured <= task.budget, "{}: over budget", task.name);
    }
}
