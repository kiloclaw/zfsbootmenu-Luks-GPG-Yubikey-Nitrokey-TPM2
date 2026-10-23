//! Fuzzy matching with `nucleo-matcher`, for the filter of every list and
//! the command palette.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// An item that the query matches.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Ranked {
    /// The index of the item.
    pub index: usize,
    /// The positions, in characters, that the query matched.
    pub positions: Vec<usize>,
}

/// The items that `query` matches, best first; every item in order when the
/// query is empty. Equal scores keep the order of the items.
pub fn rank<'a>(items: impl IntoIterator<Item = &'a str>, query: &str) -> Vec<Ranked> {
    if query.is_empty() {
        return items
            .into_iter()
            .enumerate()
            .map(|(index, _)| Ranked {
                index,
                positions: Vec::new(),
            })
            .collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buffer = Vec::new();
    let mut scored: Vec<(u32, Ranked)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let haystack = Utf32Str::new(item, &mut buffer);
            let mut indices = Vec::new();
            let score = pattern.indices(haystack, &mut matcher, &mut indices)?;
            indices.sort_unstable();
            indices.dedup();
            let positions = indices
                .into_iter()
                .filter_map(|i| usize::try_from(i).ok())
                .collect();
            Some((score, Ranked { index, positions }))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.index.cmp(&b.1.index)));
    scored.into_iter().map(|(_, ranked)| ranked).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn names(items: &[&str], query: &str) -> Vec<String> {
        rank(items.iter().copied(), query)
            .into_iter()
            .map(|ranked| items[ranked.index].to_owned())
            .collect()
    }

    #[test]
    fn an_empty_query_keeps_every_item_in_order() {
        assert_eq!(names(&["b", "a"], ""), ["b", "a"]);
    }

    #[test]
    fn a_query_keeps_the_matches_best_first() {
        let items = [
            "zroot/ROOT/void@autosnap-2026-08-31",
            "zroot/ROOT/void@pre-upgrade",
            "tank/ROOT/arch",
        ];

        assert_eq!(names(&items, "pre"), ["zroot/ROOT/void@pre-upgrade"]);
        assert_eq!(
            names(&items, "2026-08"),
            ["zroot/ROOT/void@autosnap-2026-08-31"]
        );
        assert_eq!(names(&items, "xyz"), Vec::<String>::new());
    }

    #[test]
    fn positions_are_the_matched_characters() {
        assert_eq!(
            rank(["tank/ROOT/arch"], "arch")[0].positions,
            [10, 11, 12, 13]
        );
    }
}
