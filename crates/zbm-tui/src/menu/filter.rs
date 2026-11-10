//! Fuzzy matching with `nucleo-matcher`, for the filter of every list and
//! the command line: a list's filter has smart case, a query with a
//! capital matching only capitals there, and the command line ignores
//! case, so that Caps Lock changes nothing it finds.

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

/// The items that `query` matches, best first, with smart case; every item
/// in order when the query is empty. Equal scores keep the order of the
/// items.
pub fn rank<'a>(items: impl IntoIterator<Item = &'a str>, query: &str) -> Vec<Ranked> {
    let items = items.into_iter().map(|item| (item, None));
    rank_named(items, query, CaseMatching::Smart)
}

/// The items that `query` matches, each by the best of its names: the one
/// shown, and the others it is also known by, one a line, as a command's
/// name and target, and its English ones in another language (rule 4 of
/// decision 18). The positions are the shown name's, and none when
/// another name matched better, as they mark what is drawn. Best first;
/// every item in order when the query is empty; equal scores keep the
/// order of the items. `case` says how the query's capitals match.
pub fn rank_named<'a>(
    items: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    query: &str,
    case: CaseMatching,
) -> Vec<Ranked> {
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
    let pattern = Pattern::parse(query, case, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buffer = Vec::new();
    let mut scored: Vec<(u32, Ranked)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(index, (shown, other))| {
            let mut indices = Vec::new();
            let haystack = Utf32Str::new(shown, &mut buffer);
            let shown_score = pattern.indices(haystack, &mut matcher, &mut indices);
            let other_score = other
                .into_iter()
                .flat_map(str::lines)
                .fold(None, |best, other| {
                    let score = pattern.score(Utf32Str::new(other, &mut buffer), &mut matcher);
                    best.max(score)
                });
            let score = match (shown_score, other_score) {
                (Some(shown), Some(other)) if other > shown => {
                    indices.clear();
                    other
                }
                (Some(shown), _) => shown,
                (None, other) => {
                    indices.clear();
                    other?
                }
            };
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
    fn the_better_of_two_names_ranks_an_item() {
        let items = [
            ("ѕһеӏӏ", Some("Recovery shell")),
            ("роԝег оғғ", Some("Power off")),
            ("Power off now", None),
        ];

        let ranked = rank_named(items, "power", CaseMatching::Smart);

        let indexes: Vec<usize> = ranked.iter().map(|ranked| ranked.index).collect();
        assert_eq!(indexes, [1, 2]);
        assert_eq!(ranked[0].positions, Vec::<usize>::new());
        assert_eq!(ranked[1].positions, [0, 1, 2, 3, 4]);
        let shown_best = rank_named(
            [("Power off", Some("p-o-w-e-r"))],
            "power",
            CaseMatching::Smart,
        );
        assert_eq!(shown_best[0].positions, [0, 1, 2, 3, 4]);
        let tied = rank_named(
            [("Power off", Some("Power off"))],
            "power",
            CaseMatching::Smart,
        );
        assert_eq!(tied[0].positions, [0, 1, 2, 3, 4]);
        let none = rank_named([("a", Some("b"))], "c", CaseMatching::Smart);
        assert_eq!(none, Vec::new());
        // Each line of the other names is a name of its own: a query
        // matches one of them whole, not parts of two.
        let lines = [("x", Some("boot void\nboot arch"))];
        assert_eq!(rank_named(lines, "boot arch", CaseMatching::Smart).len(), 1);
        assert_eq!(
            rank_named(lines, "voidboot", CaseMatching::Smart),
            Vec::new()
        );
    }

    #[test]
    fn the_command_line_ignores_case_and_the_filter_keeps_smart_case() {
        let items = [("Reboot", None), ("reboot the firmware", None)];
        let found = |query, case| -> Vec<usize> {
            let ranked = rank_named(items, query, case);
            ranked.into_iter().map(|ranked| ranked.index).collect()
        };

        assert_eq!(found("REBOOT", CaseMatching::Ignore), [0, 1]);
        assert_eq!(found("REBOOT", CaseMatching::Smart), Vec::<usize>::new());
        assert_eq!(names(&["Reboot", "reboot the firmware"], "Reb"), ["Reboot"]);
        assert_eq!(
            rank_named(items, "REB", CaseMatching::Ignore)[0].positions,
            [0, 1, 2]
        );
    }

    #[test]
    fn positions_are_the_matched_characters() {
        assert_eq!(
            rank(["tank/ROOT/arch"], "arch")[0].positions,
            [10, 11, 12, 13]
        );
    }
}
