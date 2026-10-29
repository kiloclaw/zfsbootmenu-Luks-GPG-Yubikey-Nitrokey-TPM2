//! The lists of the menu: their selection and filter.

use crate::menu::filter::Ranked;

/// A list's selection and filter.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListState {
    /// The filter text, while filtering or when one was kept.
    pub filter: String,
    /// Whether the filter field has the focus.
    pub filtering: bool,
    /// The items the filter matches, best first, as indexes of the list.
    pub matches: Vec<Ranked>,
    /// The index in `matches` of the selection.
    pub selected: usize,
}

impl ListState {
    /// The index in the list of the selection, if anything matches.
    pub fn selected_item(&self) -> Option<usize> {
        self.matches.get(self.selected).map(|ranked| ranked.index)
    }
}

/// Matches a list's filter against its items again, keeping the selection
/// in range.
pub fn refilter<'a>(list: &mut ListState, items: impl IntoIterator<Item = &'a str>) {
    list.matches = crate::menu::filter::rank(items, &list.filter);
    list.selected = list.selected.min(list.matches.len().saturating_sub(1));
}

/// Selects the match whose item is called `name`, if one is.
pub(super) fn keep<T>(
    list: &mut ListState,
    items: &[T],
    name: Option<String>,
    label: impl Fn(&T) -> String,
) {
    let Some(name) = name else {
        return;
    };
    let found = list.matches.iter().position(|ranked| {
        items
            .get(ranked.index)
            .is_some_and(|item| label(item) == name)
    });
    if let Some(position) = found {
        list.selected = position;
    }
}
