//! Where the focus is: the tab of the workspace, the key map context and
//! the list of the current screen.

use super::{Dialog, DiffView, ListState, Model, Screen, Segment};
use crate::menu::keymap::{Context, Tab};

impl Model {
    /// The tab of the workspace the current screen belongs to.
    pub fn tab(&self) -> Tab {
        use Context::{Diff, Packages};
        match self.screen {
            Screen::Snapshots
            | Screen::Diff
            | Screen::Help(Context::Snapshots | Diff | Packages) => Tab::Snapshots,
            Screen::Pools | Screen::Help(Context::Pools) => Tab::Pools,
            Screen::Log | Screen::Help(Context::Log) => Tab::Log,
            _ => Tab::Boot,
        }
    }

    /// The key map context of the focus.
    pub fn context(&self) -> Context {
        match &self.dialog {
            Some(Dialog::Unlock(_) | Dialog::Confirm { .. } | Dialog::Name(_)) => {
                return Context::Text;
            }
            Some(Dialog::Choose { .. } | Dialog::Error { .. } | Dialog::Question(_)) => {
                return Context::Choose;
            }
            None => {}
        }
        if self.palette.is_some()
            || self.editor.is_some()
            || self.list().is_some_and(|l| l.filtering)
        {
            return Context::Text;
        }
        match self.screen {
            Screen::Startup => Context::Startup,
            Screen::Main => Context::Main,
            Screen::Snapshots => Context::Snapshots,
            Screen::Kernels => Context::Kernels,
            Screen::CommandLine => Context::Text,
            Screen::Pools => Context::Pools,
            Screen::Recovery => Context::Recovery,
            Screen::Log => Context::Log,
            Screen::Help(_) => Context::Help,
            Screen::Diff => match self.diff.as_ref().map(|diff| diff.segment) {
                Some(Segment::Packages) => Context::Packages,
                _ => Context::Diff,
            },
        }
    }

    /// The list of the current screen.
    pub fn list(&self) -> Option<&ListState> {
        match self.screen {
            Screen::Main => Some(&self.environments),
            Screen::Snapshots => Some(&self.snapshots),
            Screen::Kernels => Some(&self.kernels),
            Screen::Pools => Some(&self.pools),
            Screen::Recovery => Some(&self.recovery),
            Screen::Diff => self.diff.as_ref().map(DiffView::active_list),
            _ => None,
        }
    }

    /// The list of the current screen, to change.
    pub fn list_mut(&mut self) -> Option<&mut ListState> {
        match self.screen {
            Screen::Main => Some(&mut self.environments),
            Screen::Snapshots => Some(&mut self.snapshots),
            Screen::Kernels => Some(&mut self.kernels),
            Screen::Pools => Some(&mut self.pools),
            Screen::Recovery => Some(&mut self.recovery),
            Screen::Diff => self.diff.as_mut().map(DiffView::active_list_mut),
            _ => None,
        }
    }
}
