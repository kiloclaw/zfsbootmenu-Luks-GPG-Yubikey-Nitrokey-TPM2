//! What a program that failed said: the last lines of its standard error,
//! where tools put their errors and scripts the step that failed, bounded
//! so that a program that prints much cannot fill the log.

/// The most lines kept.
pub(crate) const LINES: usize = 4;

/// The most characters kept of each line.
pub(crate) const WIDTH: usize = 200;

/// The last [`LINES`] lines of `text` that are not blank, oldest first, each
/// trimmed and cut to [`WIDTH`] characters (`...` marks a cut), joined with
/// ` | `; `None` when every line is blank.
pub(crate) fn last_lines(text: &str) -> Option<String> {
    let mut lines: Vec<String> = text
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(LINES)
        .map(|line| match line.char_indices().nth(WIDTH) {
            Some((end, _)) => format!("{}...", line.get(..end).unwrap_or(line)),
            None => line.to_owned(),
        })
        .collect();
    lines.reverse();
    (!lines.is_empty()).then(|| lines.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_last_lines_are_kept_oldest_first() {
        let text = "one\n\n  two  \nthree\nfour\nfive\n\n";
        assert_eq!(
            last_lines(text).as_deref(),
            Some("two | three | four | five")
        );
        assert_eq!(last_lines(" \n\n").as_deref(), None);
        assert_eq!(last_lines("").as_deref(), None);
    }

    #[test]
    fn a_long_line_is_cut_at_a_character() {
        let line = "é".repeat(WIDTH + 5);
        let kept = last_lines(&line).unwrap();
        assert_eq!(kept, format!("{}...", "é".repeat(WIDTH)));
        assert_eq!(last_lines(&"x".repeat(WIDTH)).unwrap(), "x".repeat(WIDTH));
    }
}
