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
In images built with the Rust runtime, shows the menu as lines a screen reader reads in order,
rather than as a screen redrawn in place: each view prints its title, its items numbered, what
happened last and the dialog over it with its choices numbered, and prints again only when that
changed. Type a number to choose an item, a number and a key to use that key on it (*2 k*, the
kernels of the second), one of the keys the view names, or *ok*, *back* and *help*; in a field
that takes text, the line is the text, and a blank line goes back. Passphrases are typed without
echo. The lines show by themselves when **TERM** is *dumb* or unset, and when the *speakup* screen
reader is loaded; **zbm.menu=screen** shows the screen even then.

A later menu, the workspace, puts boot environments, snapshots, pools and the log on tabs of one
screen and reads other keys. Once it becomes the menu, **zbm.menu=classic** keeps the menu of
this release, its screens and its keys, for one more release; the lines still show by themselves
where they would. Until then the menu is the classic one, and *classic* changes nothing.
";
