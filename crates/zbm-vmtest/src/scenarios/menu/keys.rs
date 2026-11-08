//! `menu-keys`: the menu's keys on the six keyboard layouts of ADR 0007's
//! check of decision 18, `us`, `de`, `fr`, `gb`, `ru` and `us(dvorak)`, on
//! the console on the display, driven as `menu-display` drives it. Keys
//! are pressed by their place on QEMU's keyboard ([`Key::Qemu`]): the
//! layout the console types with gives their characters, and the menu
//! counts each as rule 3 of `docs/dev/menu-design/keymap.md` says. A
//! character the map binds counts as itself; one it binds nowhere as the
//! US character of its key, at the level Shift gives; Caps Lock gives a
//! capital, which is its letter (rule 2).
//!
//! One boot of the console's image, which has the six layouts, on fresh
//! copies of the pristine disks, with `menu-display`'s command line and
//! the console's status line (`zbm.console_status=on`), which names the
//! layout the keyboards type with as the image does: `us(dvorak)`. The
//! console starts with the image's first, `us`; Super+Space, which the
//! console takes before the menu, gives the next in the image's order.
//! Each round waits for the status line to name its layout, and with Caps
//! Lock on for it to say `Caps`, then presses its keys and waits for the
//! view each brings:
//!
//! - `us`, `gb`: `2`, `1`, `l` and `shift-slash` type 2, 1, l and `?`: the
//!   Snapshots tab, the Boot tab, the kernels and the help;
//! - `de`: the same keys; `shift-slash` types `_`, which the map binds
//!   nowhere: it counts as `?`;
//! - `fr`: `2` types `é` and `1` `&`, which count as 2 and 1; `slash`
//!   types `!`, which the map binds: the recovery tools; `shift-slash`
//!   types `§`, which counts as `?`;
//! - `fr` with Caps Lock: `2` types `É`, whose letter `é` counts as 2;
//! - `ru`: `2` and `1` type digits; `l` types `д`, which counts as l;
//!   `shift-slash` types `,`, which counts as `?`; in the Snapshots tab,
//!   `c` types `с`, which counts as c: the clone's name;
//! - `ru` with Caps Lock: `l` types `Д`, whose letter `д` counts as l;
//! - `us(dvorak)`: `p` types `l` and `l` types `n`, which the map binds:
//!   the kernels and a new snapshot; `shift-bracket_left` types `?`.
//!
//! When Super+Space does not reach the console under QEMU, [`SUPER_SPACE`]
//! set to false boots once per layout instead, each with
//! `zbm.keyboard_layout=<name>`: about 12 minutes rather than 4.

use crate::error::{Error, Result};
use crate::images::zbm_image;
use crate::record::Test;
use crate::scenarios::display::{LAYOUTS, Monitor, sendkey, spec};
use crate::scenarios::runtime::colours::screendump;
use crate::vm::Vm;

use super::super::drive::{Display, Driver, Key};
use super::display::{APPEND, on_display, start};
use super::steps::{self, Act, Await, Step};

/// Whether the layouts are switched with Super+Space in one boot; else
/// each layout boots on its own, chosen on the command line.
const SUPER_SPACE: bool = true;

/// The console's status line, which names the layout.
const STATUS: &str = "zbm.console_status=on";

/// How long the status line may take to name a layout, in seconds.
const STATUS_PATIENCE: u64 = 30;

/// Super+Space: the image's next layout.
const NEXT_LAYOUT: Key = Key::Qemu("meta_l-spc");

/// Caps Lock, which each press turns on or off.
const CAPS_LOCK: Key = Key::Qemu("caps_lock");

/// Escape.
const ESCAPE: Act = Act::Press(Key::Escape);

/// The kernels of the environment selected in the Boot tab: the inspector
/// of its default kernel.
const KERNELS: &str = "Environment tmenu/ROOT/be";

/// The help, and a key the help of the Boot tab lists: that of another
/// view does not.
const HELP: &str = "THIS VIEW";
const BOOT_HELP: &[&str] = &["l, Right Kernels"];

/// The key QEMU names `name`.
const fn qemu(name: &'static str) -> Act {
    Act::Press(Key::Qemu(name))
}

/// Escape, which leaves a view or a dialog for the Boot tab; the next
/// round's first view shows where it went.
const ESCAPED: Step = Step::new("Escape: the Boot tab", &[ESCAPE], Await::Nothing);

/// The keys of `us`, `de` and `gb`, at their US places.
const SAME_PLACES: &[Step] = &[
    Step::new(
        "the key of 2: the Snapshots tab",
        &[qemu("2")],
        Await::Text("[2 Snapshots]"),
    ),
    Step::new(
        "the key of 1: the Boot tab",
        &[qemu("1")],
        Await::Text("[1 Boot]"),
    ),
    Step::new(
        "the key of l: the kernels",
        &[qemu("l")],
        Await::Text(KERNELS),
    ),
    Step::new(
        "Escape, then the key of ? (_ on de): the help of the Boot tab",
        &[ESCAPE, qemu("shift-slash")],
        Await::Text(HELP),
    )
    .showing(BOOT_HELP),
    ESCAPED,
];

/// The keys of `fr`.
const FRENCH: &[Step] = &[
    Step::new(
        "the key of 2, é: the Snapshots tab",
        &[qemu("2")],
        Await::Text("[2 Snapshots]"),
    ),
    Step::new(
        "the key of 1, &: the Boot tab",
        &[qemu("1")],
        Await::Text("[1 Boot]"),
    ),
    Step::new(
        "the key of /, !: the recovery tools",
        &[qemu("slash")],
        Await::Text("RECOVERY TOOLS"),
    )
    .showing(&["Recovery Shell"]),
    Step::new(
        "Escape, then the key of ?, §: the help of the Boot tab",
        &[ESCAPE, qemu("shift-slash")],
        Await::Text(HELP),
    )
    .showing(BOOT_HELP),
    ESCAPED,
];

/// The keys of `fr` with Caps Lock on.
const FRENCH_CAPS: &[Step] = &[
    Step::new(
        "the key of 2, É: the Snapshots tab",
        &[qemu("2")],
        Await::Text("[2 Snapshots]"),
    ),
    Step::new(
        "the key of 1, &: the Boot tab",
        &[qemu("1")],
        Await::Text("[1 Boot]"),
    ),
];

/// The keys of `ru`.
const RUSSIAN: &[Step] = &[
    Step::new(
        "the key of 2: the Snapshots tab",
        &[qemu("2")],
        Await::Text("[2 Snapshots]"),
    ),
    Step::new(
        "the key of 1: the Boot tab",
        &[qemu("1")],
        Await::Text("[1 Boot]"),
    ),
    Step::new(
        "the key of l, д: the kernels",
        &[qemu("l")],
        Await::Text(KERNELS),
    ),
    Step::new(
        "Escape, then the key of ?, ',': the help of the Boot tab",
        &[ESCAPE, qemu("shift-slash")],
        Await::Text(HELP),
    )
    .showing(BOOT_HELP),
    Step::new(
        "Escape, then the key of 2: the Snapshots tab",
        &[ESCAPE, qemu("2")],
        Await::Text("[2 Snapshots]"),
    ),
    Step::new(
        "the key of c, с: clone five",
        &[Act::Select("five"), qemu("c")],
        Await::Text("Clone five"),
    )
    .showing(&["Name: tmenu/ROOT/be_NEW"]),
    Step::new(
        "Escape, then the key of 1: the Boot tab",
        &[ESCAPE, qemu("1")],
        Await::Text("[1 Boot]"),
    ),
];

/// The keys of `ru` with Caps Lock on.
const RUSSIAN_CAPS: &[Step] = &[
    Step::new(
        "the key of l, Д: the kernels",
        &[qemu("l")],
        Await::Text(KERNELS),
    ),
    Step::new(
        "Escape, then the key of ?, ',': the help of the Boot tab",
        &[ESCAPE, qemu("shift-slash")],
        Await::Text(HELP),
    )
    .showing(BOOT_HELP),
    ESCAPED,
];

/// The keys of `us(dvorak)`.
const DVORAK: &[Step] = &[
    Step::new(
        "the key of p, l: the kernels",
        &[qemu("p")],
        Await::Text(KERNELS),
    ),
    Step::new(
        "Escape, then the key of l, n: a new snapshot",
        &[ESCAPE, qemu("l")],
        Await::Text("New Snapshot of tmenu/ROOT/be"),
    ),
    Step::new(
        "Escape, then the key of [ with Shift, ?: the help of the Boot tab",
        &[ESCAPE, qemu("shift-bracket_left")],
        Await::Text(HELP),
    )
    .showing(BOOT_HELP),
    ESCAPED,
];

/// A round of the test: keys pressed on a layout.
#[derive(Clone, Copy, Debug)]
struct Round {
    /// The layout, as the image names it.
    layout: &'static str,
    /// Whether Caps Lock is on.
    caps_lock: bool,
    /// The keys, and the views they bring.
    steps: &'static [Step],
}

impl Round {
    /// The round of `steps` on `layout`.
    const fn on(layout: &'static str, steps: &'static [Step]) -> Round {
        Round {
            layout,
            caps_lock: false,
            steps,
        }
    }

    /// The round, with Caps Lock on.
    const fn with_caps_lock(self) -> Round {
        Round {
            caps_lock: true,
            ..self
        }
    }
}

/// The rounds, in the image's order of the layouts, which Super+Space
/// follows.
const ROUNDS: &[Round] = &[
    Round::on("us", SAME_PLACES),
    Round::on("de", SAME_PLACES),
    Round::on("fr", FRENCH),
    Round::on("fr", FRENCH_CAPS).with_caps_lock(),
    Round::on("gb", SAME_PLACES),
    Round::on("ru", RUSSIAN),
    Round::on("ru", RUSSIAN_CAPS).with_caps_lock(),
    Round::on("us(dvorak)", DVORAK),
];

/// A boot of the test.
#[derive(Debug)]
struct Boot {
    /// Its name, which names its mirror.
    label: String,
    /// Its command line.
    append: String,
    /// The layout the console starts with.
    layout: &'static str,
    /// Its rounds, in order.
    rounds: Vec<Round>,
}

/// The boots of the test: one, whose rounds switch the layouts with
/// Super+Space, when `super_space`; else one per layout, which the
/// command line chooses.
fn boots(super_space: bool) -> Vec<Boot> {
    let append = format!("{APPEND} {STATUS}");
    let [first, ..] = LAYOUTS;
    if super_space {
        return vec![Boot {
            label: "keys".into(),
            append,
            layout: first,
            rounds: ROUNDS.to_vec(),
        }];
    }
    LAYOUTS
        .iter()
        .map(|&layout| Boot {
            label: format!("keys-{}", layout.replace('(', "-").replace(')', "")),
            append: format!("{append} zbm.keyboard_layout={layout}"),
            layout,
            rounds: ROUNDS
                .iter()
                .filter(|round| round.layout == layout)
                .copied()
                .collect(),
        })
        .collect()
}

/// Boots once, or once per layout without [`SUPER_SPACE`].
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let image = zbm_image(test.settings(), &spec())?;
    let monitor = Monitor {
        dump: screendump,
        key: sendkey,
    };
    for boot in boots(SUPER_SPACE) {
        let drive = |test: &Test, display: &mut Display<'_>, vm: &mut Vm<'_>| {
            play(test, display, vm, &boot)
        };
        on_display(
            test,
            &image,
            &monitor,
            (&boot.label, &boot.append, &[]),
            &drive,
        )?;
    }
    Ok(())
}

/// Plays the rounds of `boot` once the menu shows. The status line names
/// the layout the console starts with; a round on another layout presses
/// Super+Space first, and one with Caps Lock turns it on, then off.
fn play(test: &Test, display: &mut Display<'_>, vm: &mut Vm<'_>, boot: &Boot) -> Result<()> {
    start(test, display, vm)?;
    let mut layout = boot.layout;
    test.step(&format!("the console types with {layout}"));
    names(test, display, vm, (layout, false))?;
    for round in &boot.rounds {
        if round.layout != layout {
            layout = round.layout;
            test.step(&format!("Super+Space: {layout}"));
            display.press(vm, NEXT_LAYOUT)?;
            names(test, display, vm, (layout, false))?;
        }
        if round.caps_lock {
            test.step("Caps Lock on");
            display.press(vm, CAPS_LOCK)?;
            names(test, display, vm, (layout, true))?;
        }
        steps::run(test, display, vm, round.steps)?;
        if round.caps_lock {
            test.step("Caps Lock off");
            display.press(vm, CAPS_LOCK)?;
            names(test, display, vm, (layout, false))?;
        }
    }
    Ok(())
}

/// Checks that the status line names `layout`, and says Caps Lock is on
/// when `caps_lock`, else not.
///
/// # Errors
///
/// When it does not, within [`STATUS_PATIENCE`]: the keys would be typed
/// on another layout.
fn names(
    test: &Test,
    display: &mut Display<'_>,
    vm: &mut Vm<'_>,
    (layout, caps_lock): (&str, bool),
) -> Result<()> {
    let patience = display.pace.patience.min(STATUS_PATIENCE);
    let shown = display.screen(vm, &status(layout, caps_lock), patience)?;
    let lock = if caps_lock { "on" } else { "off" };
    let what = format!("the status line names {layout}, Caps Lock {lock}");
    test.check(shown.is_some(), &what);
    match shown {
        Some(_) => Ok(()),
        None => Err(Error::Setup(format!("{what}: it does not"))),
    }
}

/// The pattern of the status line that names `layout`, then `Caps` when
/// `caps_lock`, the other locks that may be on, and the time.
fn status(layout: &str, caps_lock: bool) -> String {
    let caps = if caps_lock { "  Caps" } else { "" };
    format!(
        r"(?m)(?:^|\s){}{caps}(?:  Num)?(?:  Scroll)?  \d\d:\d\d UTC\s*$",
        regex::escape(layout)
    )
}

#[cfg(test)]
mod tests;
