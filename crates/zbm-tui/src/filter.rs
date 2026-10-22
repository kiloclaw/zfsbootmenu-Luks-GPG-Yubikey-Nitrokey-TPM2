//! Matching the filter text against the entries, with `nucleo-matcher`.
//!
//! The matcher of Nucleo, without Nucleo's thread pool: a boot menu's lists
//! are short, so matching runs inside [`update`](crate::update), which stays
//! pure and deterministic.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::model::{Entry, Match};

/// The entries that `text` matches, best first; all entries in order when
/// `text` is empty. Equal scores keep the order of the entries.
pub(crate) fn matches(entries: &[Entry], text: &str) -> Vec<Match> {
    if text.is_empty() {
        return (0..entries.len())
            .map(|entry| Match {
                entry,
                positions: Vec::new(),
            })
            .collect();
    }
    let pattern = Pattern::parse(text, CaseMatching::Smart, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buffer = Vec::new();
    let mut scored: Vec<(u32, Match)> = entries
        .iter()
        .enumerate()
        .filter_map(|(entry, candidate)| {
            let haystack = Utf32Str::new(candidate.name.as_str(), &mut buffer);
            let mut indices = Vec::new();
            let score = pattern.indices(haystack, &mut matcher, &mut indices)?;
            indices.sort_unstable();
            indices.dedup();
            let positions = indices
                .into_iter()
                .filter_map(|index| usize::try_from(index).ok())
                .collect();
            Some((score, Match { entry, positions }))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.entry.cmp(&b.1.entry)));
    scored.into_iter().map(|(_, found)| found).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_app::Encryption;

    fn entries(names: &[&str]) -> Vec<Entry> {
        names
            .iter()
            .map(|name| Entry {
                name: name.parse().unwrap(),
                is_default: false,
                encryption: Encryption::None,
            })
            .collect()
    }

    fn matched_names(names: &[&str], text: &str) -> Vec<String> {
        let entries = entries(names);
        matches(&entries, text)
            .iter()
            .map(|found| entries[found.entry].name.to_string())
            .collect()
    }

    #[test]
    fn an_empty_filter_keeps_every_entry_in_order() {
        assert_eq!(matched_names(&["b/x", "a/y"], ""), ["b/x", "a/y"]);
    }

    #[test]
    fn a_fuzzy_filter_keeps_only_matching_entries() {
        let names = ["zroot/ROOT/void", "zroot/ROOT/ubuntu", "tank/ROOT/arch"];

        assert_eq!(matched_names(&names, "ubu"), ["zroot/ROOT/ubuntu"]);
        assert_eq!(matched_names(&names, "zrv"), ["zroot/ROOT/void"]);
        assert_eq!(matched_names(&names, "xyz"), Vec::<String>::new());
    }

    #[test]
    fn lowercase_text_matches_any_case_and_capitals_match_exactly() {
        let names = ["tank/ROOT/arch", "tank/root/old"];

        assert_eq!(matched_names(&names, "root").len(), 2);
        assert_eq!(matched_names(&names, "ROOT"), ["tank/ROOT/arch"]);
    }

    #[test]
    fn matched_positions_are_the_characters_to_highlight() {
        let entries = entries(&["zroot/ROOT/void"]);

        let found = matches(&entries, "void");

        assert_eq!(found[0].positions, [11, 12, 13, 14]);
    }
}
