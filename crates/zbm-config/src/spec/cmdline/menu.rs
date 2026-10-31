//! How the menu shows: as a screen, or as lines for screen readers
//! (ROADMAP.md UI8, ADR 0011).

use super::super::{Doc, OptionSpec, Reader, Source, Value};

/// `zbm.menu`: the menu as a screen, or as lines.
pub static MENU: OptionSpec = OptionSpec {
    name: "zbm.menu",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Choice(&["lines", "screen"]),
    default: None,
    summary: "The menu as lines, for screen readers and simple terminals, or as a screen",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.menu=lines**",
        body: MENU_DOC,
    },
    deprecation: None,
    examples: &["zbm.menu=lines", "zbm.menu=screen"],
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
";
