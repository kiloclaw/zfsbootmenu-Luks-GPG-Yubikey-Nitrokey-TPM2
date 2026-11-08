//! The wireframes of the workspace (ADR 0007).

use zbm_display::keys::Key;

use super::{Step, Wireframe};
use crate::menu::demo::DemoOptions;

/// The demo in the workspace (ADR 0007).
fn workspace() -> DemoOptions {
    DemoOptions {
        classic: false,
        ..DemoOptions::default()
    }
}

/// The wireframes of the workspace: the Boot tab, its environments
/// grouped by pool, and the kernels column it goes into, of an environment
/// of each pool, under the path; the inspector wrapping a long command
/// line; what needs attention once the scan, which waits for a
/// key to stop the countdown, found the pools start-up left alone, one of
/// them another host's; the Pools tab as the tree of the pools, their
/// vdevs and devices and the volumes, `tank` selected, and the pool of
/// another host the scan found; the kernel's messages about the disk
/// `tank` lost, `Enter` on it in the tree; the Log tab with the boot history
/// `zbm.save_logs` kept, and without the option; and help, under the ZFS
/// version; the Snapshots tab as the timeline of the default environment,
/// its months folded and the oldest selected, and while the machine reads
/// them; start-up as the Boot tab ([`startup_wireframes`]); and the
/// review pages of changes ([`review_wireframes`]).
pub(super) fn workspace_wireframes() -> Vec<Wireframe> {
    let mut all = startup_wireframes();
    all.extend(review_wireframes());
    all.extend(view_wireframes());
    all
}

/// The views of the workspace's tabs, and help ([`workspace_wireframes`]).
fn view_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Tick, Unanswered};
    let frame = |name, keys| Wireframe {
        name,
        options: workspace(),
        ticks: 50,
        keys,
    };
    let exported = DemoOptions {
        exported: true,
        ..workspace()
    };
    let no_history = DemoOptions {
        save_logs: false,
        ..workspace()
    };
    let long_command_line = DemoOptions {
        long_command_line: true,
        ..workspace()
    };
    vec![
        frame("workspace", &[]),
        frame("workspace-kernels", &[Code(Key::Down), Char('l')]),
        frame(
            "workspace-columns",
            &[
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Right),
                Code(Key::Down),
            ],
        ),
        Wireframe {
            name: "workspace-inspector-wrap",
            options: long_command_line,
            ticks: 50,
            keys: &[Code(Key::Down), Code(Key::Down), Code(Key::Right)],
        },
        frame("workspace-snapshots", &[Char('2'), Code(Key::End)]),
        frame("workspace-snapshots-reading", &[Unanswered('2')]),
        frame(
            "workspace-pools",
            &[
                Char('3'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
            ],
        ),
        Wireframe {
            name: "workspace-pools-foreign",
            options: exported.clone(),
            ticks: 50,
            keys: &[
                Code(Key::Escape),
                Tick,
                Char('3'),
                Code(Key::End),
                Code(Key::Up),
                Code(Key::Up),
                Code(Key::Up),
            ],
        },
        frame("workspace-help", &[Char('?')]),
        Wireframe {
            name: "workspace-attention",
            options: exported,
            ticks: 50,
            keys: &[Code(Key::Escape), Tick],
        },
        frame(
            "workspace-device-log",
            &[
                Char('3'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Enter),
            ],
        ),
        frame("log-history", &[Char('4')]),
        Wireframe {
            name: "log-no-history",
            options: no_history,
            ticks: 50,
            keys: &[Char('4')],
        },
    ]
}

/// Start-up as the Boot tab, its third stage running, and the pool it
/// could not import, another host's, asking what to do over it.
fn startup_wireframes() -> Vec<Wireframe> {
    let foreign = DemoOptions {
        foreign: true,
        ..workspace()
    };
    vec![
        Wireframe {
            name: "workspace-startup",
            options: workspace(),
            ticks: 5,
            keys: &[],
        },
        Wireframe {
            name: "workspace-import-problem",
            options: foreign,
            ticks: 50,
            keys: &[],
        },
    ]
}

/// The review pages of the changes that lose what cannot be brought back
/// (ADR 0016, points 12 to 15): the rollback to the daily snapshot of 20
/// October, the environment's name typed in part; the rollback to that of
/// 17 October, which a clone and a hold of newer snapshots stop; the rewind
/// of `tank` to its checkpoint, from the imports of `i`; the forced
/// import of the pool of another host start-up could not import; and the
/// destroys ([`destroy_wireframes`]).
fn review_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let frame = |name, keys| Wireframe {
        name,
        options: workspace(),
        ticks: 50,
        keys,
    };
    let foreign = DemoOptions {
        foreign: true,
        ..workspace()
    };
    let mut all = vec![
        frame(
            "workspace-rollback-review",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char('r'),
                Text("vo"),
            ],
        ),
        frame(
            "workspace-rollback-refused",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char('r'),
            ],
        ),
        frame(
            "workspace-rewind-review",
            &[
                Char('3'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char('i'),
                Code(Key::Down),
                Code(Key::Enter),
            ],
        ),
        Wireframe {
            name: "workspace-force-import-review",
            options: foreign,
            ticks: 50,
            keys: &[
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Enter),
                Text("back"),
            ],
        },
    ];
    all.extend(destroy_wireframes());
    all
}

/// The destroys of snapshots, the workspace's alone: the actions of the
/// daily snapshot of 20 October, `Enter` on it, among them its destroy;
/// the review page of the destroy of it and of that of 21 October, both
/// marked, the environment's name typed in part; and that of the destroy
/// of the snapshot of 18 October, which its clone stops.
fn destroy_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let frame = |name, keys| Wireframe {
        name,
        options: workspace(),
        ticks: 50,
        keys,
    };
    vec![
        frame(
            "workspace-snapshot-actions",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Enter),
            ],
        ),
        frame(
            "workspace-destroy-review",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char(' '),
                Code(Key::Up),
                Char(' '),
                Code(Key::Delete),
                Text("vo"),
            ],
        ),
        frame(
            "workspace-destroy-refused",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Delete),
            ],
        ),
    ]
}
