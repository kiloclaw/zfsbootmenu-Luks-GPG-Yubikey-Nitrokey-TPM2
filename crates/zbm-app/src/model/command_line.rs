//! The [`CommandLine`] of a boot environment, and its form on the wire:
//! a snapshot from a peer older than its expansion still reads, with the
//! line as it was shown before.

use zbm_types::Untrusted;

use super::Origin;

/// The command line of a boot environment.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "Sent")]
pub struct CommandLine {
    /// The command line.
    pub value: Untrusted<String>,
    /// Where it was set.
    pub origin: Origin,
    /// The command line with `%{parent}` expanded, as the environment
    /// boots with it: an inherited line is its ancestor's expansion, so
    /// the parent's arguments are in it once; the value itself when it
    /// holds no `%{parent}`.
    pub expanded: Untrusted<String>,
}

/// A [`CommandLine`] as a peer sends it: one older than the expansion
/// sends none, and the value stands for it.
#[derive(serde::Deserialize)]
struct Sent {
    value: Untrusted<String>,
    origin: Origin,
    expanded: Option<Untrusted<String>>,
}

impl From<Sent> for CommandLine {
    fn from(sent: Sent) -> CommandLine {
        CommandLine {
            expanded: sent.expanded.unwrap_or_else(|| sent.value.clone()),
            value: sent.value,
            origin: sent.origin,
        }
    }
}
