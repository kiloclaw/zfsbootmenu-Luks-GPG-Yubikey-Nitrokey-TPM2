//! The wireframes of the workspace (ADR 0007), which also show over a
//! serial line at 80×24, in colour and in ASCII.

use zbm_display::keys::Key;

use super::{Step, Wireframe};
use crate::menu::demo::DemoOptions;

/// The demo in the workspace (ADR 0007).
fn workspace() -> DemoOptions {
    DemoOptions {
        workspace: true,
        ..DemoOptions::default()
    }
}

/// The wireframes of the workspace: the Boot tab, its environments
/// grouped by pool, and the kernels column it goes into, of an environment
/// of each pool, under the path; the inspector wrapping a long command
/// line; what needs attention once the scan, which waits for a
/// key to stop the countdown, found the pools start-up left alone, one of
/// them another host's; the kernel's messages about the disk `tank`
/// lost, chosen in its pool's dialog; the Log tab with the boot history
/// `zbm.save_logs` kept, and without the option; and help, under the ZFS
/// version; the Snapshots tab as the timeline of the default environment,
/// its months folded and the oldest selected, and while the machine reads
/// them.
pub(super) fn workspace_wireframes() -> Vec<Wireframe> {
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
        frame("workspace-pools", &[Char('3')]),
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
                Code(Key::Enter),
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
