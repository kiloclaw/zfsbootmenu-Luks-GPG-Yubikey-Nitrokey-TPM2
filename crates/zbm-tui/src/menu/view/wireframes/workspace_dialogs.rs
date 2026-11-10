//! The wireframes of the workspace's other views and dialogs (ADR 0016
//! point 16): those the classic menu has wireframes of, reached with the
//! workspace's keys, and the details of a pool.

use zbm_display::keys::Key;

use super::workspace::workspace;
use super::{Step, Wireframe};
use crate::menu::demo::DemoOptions;

/// The workspace's other views and dialogs: those of the Boot tab and the
/// views over it ([`boot_dialog_wireframes`]); those of the Pools tab
/// ([`pool_dialog_wireframes`]); those that name a snapshot or a clone
/// ([`naming_dialog_wireframes`]); the changes since a snapshot
/// ([`diff_dialog_wireframes`]); and those of unlocking
/// ([`unlock_dialog_wireframes`]).
pub(super) fn workspace_dialog_wireframes() -> Vec<Wireframe> {
    let mut all = boot_dialog_wireframes();
    all.extend(pool_dialog_wireframes());
    all.extend(naming_dialog_wireframes());
    all.extend(diff_dialog_wireframes());
    all.extend(unlock_dialog_wireframes());
    all
}

/// The search field filtered by `ta`; the command line edited for one
/// boot; the palette finding snapshots by date in the Snapshots tab, and
/// finding `pools` on the Boot tab, under its actions, items and help
/// (with the query empty, the actions alone fill the frame); a locked
/// menu's Boot tab, whose toolbar marks what the lock keeps, and its
/// palette over the Snapshots tab, with the keys and costs; the power
/// dialog; the recovery tools; the outcome of setting a default on
/// the notification line; help for the Snapshots tab; and the Log tab
/// during start-up, `Shift-Tab` from the Boot tab.
fn boot_dialog_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let frame = |name, keys| Wireframe {
        name,
        options: workspace(),
        ticks: 50,
        keys,
    };
    vec![
        frame("workspace-filter", &[Char('/'), Text("ta")]),
        frame(
            "workspace-command-line",
            &[Char('e'), Text(" systemd.unit=rescue.target")],
        ),
        frame(
            "workspace-palette",
            &[Char('2'), Char(':'), Text("2026-10")],
        ),
        frame("workspace-commands", &[Char(':'), Text("pools")]),
        Wireframe {
            name: "workspace-locked",
            options: locked_menu(),
            ticks: 50,
            keys: &[],
        },
        Wireframe {
            name: "workspace-commands-locked",
            options: locked_menu(),
            ticks: 50,
            keys: &[Char('2'), Char(':')],
        },
        frame("workspace-power", &[Char('x')]),
        frame("workspace-recovery", &[Char('!')]),
        frame("workspace-notification", &[Code(Key::Down), Char('d')]),
        frame("workspace-help-snapshots", &[Char('2'), Char('?')]),
        Wireframe {
            name: "workspace-log-startup",
            options: workspace(),
            ticks: 5,
            keys: &[Code(Key::BackTab)],
        },
    ]
}

/// The workspace's demo with the menu locked (ADR 0010).
fn locked_menu() -> DemoOptions {
    DemoOptions {
        menu_locked: true,
        ..workspace()
    }
}

/// The imports of `archive`, another host's pool the scan found, `i` on it
/// in the Pools tab; its details, `Enter` on it; and the error after a
/// plain retry of the pool start-up could not import.
fn pool_dialog_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Tick};
    let exported = DemoOptions {
        exported: true,
        ..workspace()
    };
    let foreign = DemoOptions {
        foreign: true,
        ..workspace()
    };
    vec![
        Wireframe {
            name: "workspace-import-choices",
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
                Char('i'),
            ],
        },
        Wireframe {
            name: "workspace-pool-details",
            options: exported,
            ticks: 50,
            keys: &[
                Code(Key::Escape),
                Tick,
                Char('3'),
                Code(Key::End),
                Code(Key::Up),
                Code(Key::Up),
                Code(Key::Up),
                Code(Key::Enter),
            ],
        },
        Wireframe {
            name: "workspace-import-failed",
            options: foreign,
            ticks: 50,
            keys: &[Code(Key::Enter)],
        },
    ]
}

/// The dialogs that name: a new snapshot of the default environment, `n`
/// on the Boot tab; and, in the Snapshots tab, the daily snapshot of 20
/// October selected, its clone; and the second question of a clone
/// promoted, its name typed.
fn naming_dialog_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let frame = |name, keys| Wireframe {
        name,
        options: workspace(),
        ticks: 50,
        keys,
    };
    vec![
        frame(
            "workspace-clone-name",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char('c'),
                Text("-2"),
            ],
        ),
        frame("workspace-snapshot-name", &[Char('n')]),
        frame(
            "workspace-promote-confirm",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char('m'),
                Text("-2"),
                Code(Key::Enter),
            ],
        ),
    ]
}

/// In the Snapshots tab, the daily snapshot of 20 October selected: the
/// changes since it; two of them marked; and the restore's alert.
fn diff_dialog_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let frame = |name, keys| Wireframe {
        name,
        options: workspace(),
        ticks: 50,
        keys,
    };
    vec![
        frame(
            "workspace-diff",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char('f'),
            ],
        ),
        frame(
            "workspace-diff-marked",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char('f'),
                Char(' '),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char(' '),
            ],
        ),
        frame(
            "workspace-restore-confirm",
            &[
                Char('2'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char('f'),
                Char(' '),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char(' '),
                Code(Key::Enter),
                Text("vo"),
            ],
        ),
    ]
}

/// Unlocking over start-up as the Boot tab: a TPM2 PIN, two digits typed;
/// a wrong passphrase; typing with the German layout, `Ctrl-K` switching
/// to it, what is typed not shown; the passphrase asked
/// after a firmware update; the alert that seals the key again; and the
/// volume whose key is out of date, selected in the Pools tab once the
/// alert was declined.
fn unlock_dialog_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Ctrl, Start, Text};
    let frame = |name, options, keys| Wireframe {
        name,
        options,
        ticks: 50,
        keys,
    };
    let locked = || DemoOptions {
        locked: true,
        ..workspace()
    };
    let tpm_changed = || DemoOptions {
        tpm_changed: true,
        ..workspace()
    };
    vec![
        frame("workspace-unlock-pin", locked(), &[Text("12")]),
        frame(
            "workspace-unlock-wrong",
            locked(),
            &[Code(Key::Down), Text("dmeo"), Code(Key::Enter)],
        ),
        frame(
            "workspace-unlock-layout",
            locked(),
            &[Code(Key::Down), Ctrl('k'), Text("Y[rich")],
        ),
        frame("workspace-unlock-tpm-changed", tpm_changed(), &[]),
        frame(
            "workspace-reseal-confirm",
            tpm_changed(),
            &[Text("demo"), Code(Key::Enter)],
        ),
        frame(
            "workspace-pools-tpm-changed",
            tpm_changed(),
            &[
                Text("demo"),
                Code(Key::Enter),
                Code(Key::Escape),
                Start,
                Char('3'),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
            ],
        ),
    ]
}
