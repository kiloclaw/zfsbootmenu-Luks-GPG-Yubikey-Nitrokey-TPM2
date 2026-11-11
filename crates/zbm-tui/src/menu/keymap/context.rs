//! Where the focus is, which says which bindings of the key map apply:
//! the views of the menu, its dialogs and fields, and the pager of the
//! recovery shell, outside the menu.

/// Where the focus is: which bindings apply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Context {
    /// The start-up view.
    Startup,
    /// The main view.
    Main,
    /// The snapshots of an environment.
    Snapshots,
    /// The kernels of an environment.
    Kernels,
    /// Pools and LUKS volumes.
    Pools,
    /// Recovery tools.
    Recovery,
    /// The log.
    Log,
    /// Help.
    Help,
    /// The files changed since a snapshot.
    Diff,
    /// The packages changed since a snapshot.
    Packages,
    /// A choice among actions (the snapshot actions, power).
    Choose,
    /// A field that takes text: a list's filter, the editor of a boot's
    /// command line, a secret, a name, a typed confirmation.
    Text,
    /// The command line (`:`): its query takes text, and it completes,
    /// deletes words and shows the entries chosen before.
    Command,
    /// The review page of a change: its field takes the target's name, and
    /// `Tab` and `Shift-Tab` move among the field and the buttons.
    Review,
    /// The pager of the recovery shell, outside the menu: its lines scroll,
    /// `/` filters them, and `q` leaves it as `Esc` does.
    Pager,
}
