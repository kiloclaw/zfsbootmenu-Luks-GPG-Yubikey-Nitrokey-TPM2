//! The keys of the pager of the recovery shell (ADR 0018, point 15), in
//! both maps: `q` leaves it, as `Esc` and `Ctrl-C` do, and `/` filters its
//! lines, as in a list. The rest reaches it from the table: the arrows,
//! `Ctrl-P` and `Ctrl-N`, `PgUp` and `PgDn`, `Home` and `End` scroll, and
//! in the workspace `j`, `k` and `h` too, which move in every view. The
//! classic map's `j`, `k` and `h` mean Chroot, Kernels and Rewind, so the
//! pager does not bind them: a key keeps one meaning in that map. Neither
//! `F1` nor the tab keys do anything there.

use zbm_display::keys::Key;

use super::{Binding, Context, Footer, Intent, NONE, Scope, bind};

/// The pager.
const PAGER_ONLY: &[Context] = &[Context::Pager];

/// The pager's own bindings, in the order of its help: `q` stays out of the
/// toolbar, which shows `Esc` for leaving.
pub(super) const PAGER: &[Binding] = &[
    bind(
        Key::Char('q'),
        NONE,
        Scope::Only(PAGER_ONLY),
        Intent::Back,
        Footer::Hidden,
    ),
    bind(
        Key::Char('/'),
        NONE,
        Scope::Only(PAGER_ONLY),
        Intent::Filter,
        Footer::Key,
    ),
];
