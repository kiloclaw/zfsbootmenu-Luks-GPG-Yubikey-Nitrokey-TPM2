//! The keys of the command line (`:`) beyond those of a field, in both
//! maps (ADR 0007, its keys table): `Tab` completes the query, `Ctrl-W`
//! deletes its last word, `Ctrl-R` shows the entries chosen before. The
//! toolbar leaves them out, as it does the other keys of a field, and
//! `zfsbootmenu(7)` lists them with those: help is a view's, and `F1`
//! opens none from a field or the command line. `Ctrl-K` is not bound
//! there: the command line's query is never translated, and `Ctrl-R` keeps
//! showing what is typed in the other fields.

use zbm_display::keys::Key;

use super::{Binding, CTRL, Context, Footer, Intent, NONE, Scope, bind};

/// The command line.
const COMMAND: &[Context] = &[Context::Command];

/// The command line's own bindings, in the order of the help view.
pub(super) const COMMAND_LINE: &[Binding] = &[
    bind(
        Key::Tab,
        NONE,
        Scope::Only(COMMAND),
        Intent::Complete,
        Footer::Hidden,
    ),
    bind(
        Key::Char('w'),
        CTRL,
        Scope::Only(COMMAND),
        Intent::DeleteWord,
        Footer::Hidden,
    ),
    bind(
        Key::Char('r'),
        CTRL,
        Scope::Only(COMMAND),
        Intent::History,
        Footer::Hidden,
    ),
];
