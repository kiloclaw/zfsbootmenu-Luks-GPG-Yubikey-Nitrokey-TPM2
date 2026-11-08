//! The wireframes of the classic menu (`zbm.menu=classic`), which the
//! workspace leaves as they are.

use zbm_display::keys::Key;

use super::{Step, Wireframe};
use crate::menu::demo::DemoOptions;

/// The demo in the classic menu.
fn classic() -> DemoOptions {
    DemoOptions {
        classic: true,
        ..DemoOptions::default()
    }
}

/// The demo after a firmware update: the TPM keeps a volume's key.
fn tpm_changed() -> DemoOptions {
    DemoOptions {
        tpm_changed: true,
        ..classic()
    }
}

/// The demo with the default used up its boot tries, or a one-time boot.
fn health(failing: bool, next_boot: bool) -> DemoOptions {
    DemoOptions {
        failing,
        next_boot,
        ..classic()
    }
}

fn options(locked: bool, foreign: bool) -> DemoOptions {
    DemoOptions {
        locked,
        foreign,
        ..classic()
    }
}

/// The wireframes of naming a clone and a snapshot.
pub(super) fn naming_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let frame = |name, keys| Wireframe {
        name,
        options: options(false, false),
        ticks: 50,
        keys,
    };
    vec![
        frame(
            "clone-name",
            &[Char('s'), Char('o'), Code(Key::Home), Char('c'), Text("-2")],
        ),
        frame("snapshot-name", &[Char('n')]),
    ]
}

/// The wireframes of unlocking and of the packages since a snapshot.
pub(super) fn unlock_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Ctrl, Start, Text};
    let frame = |name, options, keys| Wireframe {
        name,
        options,
        ticks: 50,
        keys,
    };
    vec![
        frame("unlock-tpm-changed", tpm_changed(), &[]),
        frame(
            "unlock-wrong",
            options(true, false),
            &[Code(Key::Down), Text("dmeo"), Code(Key::Enter)],
        ),
        frame(
            "unlock-layout",
            options(true, false),
            &[Code(Key::Down), Ctrl('k'), Text("Y[rich")],
        ),
        frame(
            "reseal-confirm",
            tpm_changed(),
            &[Text("demo"), Code(Key::Enter)],
        ),
        frame(
            "pools-tpm-changed",
            tpm_changed(),
            &[
                Text("demo"),
                Code(Key::Enter),
                Code(Key::Escape),
                Start,
                Char('p'),
            ],
        ),
        frame(
            "diff-packages",
            options(false, false),
            &[
                Char('s'),
                Char('o'),
                Code(Key::Home),
                Char('f'),
                Code(Key::Right),
            ],
        ),
    ]
}

/// The wireframes of the views, and of boot health.
pub(super) fn first_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let plain = || options(false, false);
    let frame = |name, options, ticks, keys| Wireframe {
        name,
        options,
        ticks,
        keys,
    };
    vec![
        frame("startup", plain(), 5, &[]),
        frame("main", plain(), 50, &[]),
        frame("main-filter", plain(), 50, &[Char('/'), Text("ta")]),
        frame("kernels", plain(), 50, &[Char('k')]),
        frame(
            "command-line",
            plain(),
            50,
            &[Char('e'), Text(" systemd.unit=rescue.target")],
        ),
        frame("pools", plain(), 50, &[Char('p')]),
        frame("unlock-pin", options(true, false), 50, &[Text("12")]),
        frame("import-problem", options(false, true), 50, &[]),
        frame(
            "import-failed",
            options(false, true),
            50,
            &[Code(Key::Enter)],
        ),
        frame(
            "palette",
            plain(),
            50,
            &[Char('s'), Code(Key::Escape), Char(':'), Text("2026-09")],
        ),
        frame("recovery", plain(), 50, &[Char('!')]),
        frame("log", plain(), 50, &[Char('l')]),
        frame("help", plain(), 50, &[Char('?')]),
        frame("power", plain(), 50, &[Char('x')]),
        frame("notification", plain(), 50, &[Code(Key::Down), Char('d')]),
        frame("main-fallback", health(true, false), 50, &[]),
        frame("main-next-boot", health(false, true), 50, &[]),
        frame(
            "may-not-boot",
            plain(),
            50,
            &[Code(Key::Down), Code(Key::Down), Code(Key::Enter)],
        ),
    ]
}

/// The wireframes of the snapshots view, sorted newest first.
pub(super) fn snapshot_wireframes() -> Vec<Wireframe> {
    use Step::{Char, Code, Text};
    let frame = |name, keys| Wireframe {
        name,
        options: options(false, false),
        ticks: 50,
        keys,
    };
    vec![
        frame("snapshots", &[Char('s'), Char('o'), Code(Key::Home)]),
        frame(
            "snapshot-actions",
            &[Char('s'), Char('o'), Code(Key::Home), Code(Key::Enter)],
        ),
        frame(
            "rollback-confirm",
            &[Char('s'), Char('o'), Code(Key::Home), Char('r'), Text("vo")],
        ),
        frame("diff", &[Char('s'), Char('o'), Code(Key::Home), Char('f')]),
        frame(
            "diff-marked",
            &[
                Char('s'),
                Char('o'),
                Code(Key::Home),
                Char('f'),
                Char(' '),
                Code(Key::Down),
                Code(Key::Down),
                Code(Key::Down),
                Char(' '),
            ],
        ),
        frame(
            "restore-confirm",
            &[
                Char('s'),
                Char('o'),
                Code(Key::Home),
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
