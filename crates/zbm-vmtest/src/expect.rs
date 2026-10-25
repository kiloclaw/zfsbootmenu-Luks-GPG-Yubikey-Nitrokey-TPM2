//! Matching patterns against console text, as `pexpect` does: among several
//! patterns, the match that starts first wins, and the first pattern among
//! those that start at the same place.
//!
//! The text grows while a test waits. Searching all of it again after each
//! read would be quadratic on long boots, so a search starts [`WINDOW`]
//! bytes before the end of the text already searched: a match shorter than
//! that, ending in the new text, starts within the window, and a match
//! within the text already searched would have been found then.

use regex::Regex;

use crate::error::{Error, Result};

/// How far back into the text already searched a search starts.
pub const WINDOW: usize = 16 * 1024;

/// A match of one of several patterns.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Found {
    /// Which pattern matched.
    pub index: usize,
    /// Where the match starts in the text.
    pub start: usize,
    /// Where it ends.
    pub end: usize,
    /// The whole match, then each group (`None` when a group did not take
    /// part).
    pub groups: Vec<Option<String>>,
}

/// The first match of any of `patterns` in `text` that starts at `from` or
/// later.
pub fn earliest(patterns: &[Regex], text: &str, from: usize) -> Option<Found> {
    let mut best: Option<Found> = None;
    for (index, pattern) in patterns.iter().enumerate() {
        let Some(captures) = pattern.captures_at(text, from) else {
            continue;
        };
        let whole = captures.get(0)?;
        if best
            .as_ref()
            .is_some_and(|best| best.start <= whole.start())
        {
            continue;
        }
        let groups = captures
            .iter()
            .map(|group| group.map(|group| group.as_str().to_owned()))
            .collect();
        best = Some(Found {
            index,
            start: whole.start(),
            end: whole.end(),
            groups,
        });
    }
    best
}

/// The prefix of a pattern that is matched against the screen the console
/// shows rather than against its text. A full-screen program redraws only
/// the cells that change, so its words may be incomplete in the text: a
/// letter that was on the screen at that place already is not sent again.
pub const SCREEN: &str = "screen:";

/// What a pattern is matched against: the console's text, or its screen.
#[derive(Clone, Debug)]
pub enum Matcher {
    /// The text, as it arrived.
    Text(Regex),
    /// The screen ([`SCREEN`]).
    Screen(Regex),
}

impl Matcher {
    /// `pattern` compiled, for the screen when it starts with [`SCREEN`].
    ///
    /// # Errors
    ///
    /// [`Error::Pattern`] when it is not a valid regular expression.
    pub fn new(pattern: &str) -> Result<Matcher> {
        Ok(match pattern.strip_prefix(SCREEN) {
            Some(pattern) => Matcher::Screen(regex(pattern)?),
            None => Matcher::Text(regex(pattern)?),
        })
    }
}

/// The first match of a text pattern of `matchers` in `text` that starts
/// at `from` or later; its index is that of the matcher.
pub fn earliest_text(matchers: &[Matcher], text: &str, from: usize) -> Option<Found> {
    let (indexes, patterns): (Vec<usize>, Vec<Regex>) = matchers
        .iter()
        .enumerate()
        .filter_map(|(index, matcher)| match matcher {
            Matcher::Text(regex) => Some((index, regex.clone())),
            Matcher::Screen(_) => None,
        })
        .unzip();
    let mut found = earliest(&patterns, text, from)?;
    found.index = indexes.get(found.index).copied()?;
    Some(found)
}

/// The first screen pattern of `matchers` that `screen` matches.
pub fn on_screen(matchers: &[Matcher], screen: &str) -> Option<Found> {
    matchers.iter().enumerate().find_map(|(index, matcher)| {
        let Matcher::Screen(regex) = matcher else {
            return None;
        };
        let captures = regex.captures(screen)?;
        Some(Found {
            index,
            start: 0,
            end: 0,
            groups: captures
                .iter()
                .map(|group| group.map(|group| group.as_str().to_owned()))
                .collect(),
        })
    })
}

/// `pattern` compiled.
///
/// # Errors
///
/// [`Error::Pattern`] when it is not a valid regular expression.
pub fn regex(pattern: &str) -> Result<Regex> {
    Regex::new(pattern).map_err(|source| Error::Pattern {
        pattern: pattern.to_owned(),
        source,
    })
}

/// Where a search of `text` starts when the text up to `searched` was
/// searched before: [`WINDOW`] bytes back, on a character boundary.
pub fn search_start(text: &str, searched: usize) -> usize {
    let mut start = searched.min(text.len()).saturating_sub(WINDOW);
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn patterns(sources: &[&str]) -> Vec<Regex> {
        sources
            .iter()
            .map(|source| Regex::new(source).unwrap())
            .collect()
    }

    #[test]
    fn the_match_that_starts_first_wins() {
        let patterns = patterns(&[r"RC=(\d+)\r*\n", r"Enter passphrase for [^\n]*: "]);
        let text = "x Enter passphrase for /dev/vda: RC=0\r\n";

        let found = earliest(&patterns, text, 0).unwrap();

        assert_eq!((found.index, found.start), (1, 2));
        assert_eq!(&text[found.end..], "RC=0\r\n");
    }

    #[test]
    fn the_first_pattern_wins_a_tie() {
        let found = earliest(&patterns(&["ab", "a"]), "zab", 0).unwrap();

        assert_eq!((found.index, found.start, found.end), (0, 1, 3));
    }

    #[test]
    fn groups_are_captured() {
        let pattern = r"ZBMTEST-TEARDOWN dm=\[(.*?)\] pools=\[(.*?)\]( x)?";
        let text = "ZBMTEST-TEARDOWN dm=[] pools=[tank]";

        let found = earliest(&patterns(&[pattern]), text, 0).unwrap();

        assert_eq!(
            found.groups,
            vec![
                Some(text.to_owned()),
                Some(String::new()),
                Some("tank".to_owned()),
                None
            ]
        );
    }

    #[test]
    fn nothing_found_before_the_start() {
        assert_eq!(earliest(&patterns(&["a"]), "a b", 1), None);
    }

    #[test]
    fn a_search_restarts_a_window_back_on_a_character_boundary() {
        let text = format!("{}é{}", "x".repeat(10), "y".repeat(WINDOW));

        assert_eq!(search_start(&text, 5), 0);
        assert_eq!(search_start(&text, text.len() - 1), 10);
        assert_eq!(search_start(&text, text.len()), 12);
        assert_eq!(search_start(&text, text.len() + 50), 12);
    }
}
