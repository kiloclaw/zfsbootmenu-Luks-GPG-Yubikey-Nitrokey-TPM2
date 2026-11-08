//! How the menu shows: as a screen, or as lines for screen readers
//! (ROADMAP.md UI8, ADR 0011), and the classic menu kept for a release (ADR
//! 0016, point 10).

use super::super::{Doc, OptionSpec, Reader, Source, Value};

/// `zbm.menu`: the menu as a screen, as lines, or the classic menu.
pub static MENU: OptionSpec = OptionSpec {
    name: "zbm.menu",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["lines", "screen", "classic"]),
    default: None,
    summary: "Lines for screen readers and simple terminals, a screen, or the classic menu",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.menu=<lines|screen|classic>**",
        body: MENU_DOC,
    },
    deprecation: None,
    examples: &["zbm.menu=lines", "zbm.menu=screen", "zbm.menu=classic"],
};

const MENU_DOC: &str = r"
In images built with the Rust runtime, the menu is the workspace: boot environments, snapshots,
pools and the log on the tabs of one screen. **1** to **4** choose a tab and **Tab** the next;
**j** and **k** or the arrows move, **l** or **Right** goes into the kernels of the boot environment
selected and **h**, **Left** or **Escape** back out; **Enter** boots what is selected, **!** opens
the recovery tools, the shell first, **/** filters and **?** lists the keys of the view.
**The Menu's Keys** below lists every key of each view.

A rollback (**r** on the Snapshots tab), a rewind to the checkpoint, a forced import and the
destroy of snapshots (**Delete** on the Snapshots tab: those marked with **Space**, or else the one
selected) open a review page first: what the change loses, frees and keeps, read from the machine,
its steps, its undo or that it has none, and a safer action where there is one. A change that a
clone, a hold or another system using the pool would stop is refused there, before any pool is
imported read/write; any other is made once the target's name is typed, and **Escape** changes
nothing.

**zbm.menu=lines** shows the menu as lines a screen reader reads in order, rather than as a
screen redrawn in place: each view prints its title, its items numbered, what happened last and
the dialog over it with its choices numbered, and prints again only when that changed. Type a
number to choose an item, a number and a key to use that key on it (*2 l*, the kernels of the
second), one of the keys the view names, *ok*, *back* or *help*, or a tab's name (*boot*,
*snapshots*, *pools*, *log*); in a field that takes text, the line is the text, and a blank line
goes back. Passphrases are typed without echo. The lines show by themselves when **TERM** is
*dumb* or unset, and when the *speakup* screen reader is loaded; **zbm.menu=screen** shows the
screen even then.

**zbm.menu=classic** keeps the classic menu, its screens and its keys (**k** the kernels, **s**
the snapshots, **p** the pools, **l** the log), for this release only: a rollback, a rewind or a
forced import asks for the target's name in an alert instead of the review page, and no snapshot
is destroyed. The lines still show by themselves where they would, with the classic keys (*2 k*).
";
