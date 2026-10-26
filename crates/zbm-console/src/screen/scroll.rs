//! A scrolled screen moved rather than drawn again. The terminal says only
//! that the whole screen changed when it scrolls; the console keeps a hash
//! of each row as it drew it, finds the shift of those rows that matches
//! the most rows of the screen now, moves their pixels, and draws only the
//! rows that still differ.

/// How the rows drawn are moved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shift {
    /// Not at all.
    None,
    /// Up by this many rows: the screen scrolled forward.
    Up(usize),
    /// Down by this many rows: the screen scrolled back.
    Down(usize),
}

/// How many rows of `new` match the rows `old` held `by` rows below (up)
/// or above (down) them.
fn matches(old: &[Option<u64>], new: &[u64], by: usize, up: bool) -> usize {
    let pairs = |(was, now): (&Option<u64>, &u64)| *was == Some(*now);
    if up {
        old.iter()
            .skip(by)
            .zip(new)
            .filter(|&pair| pairs(pair))
            .count()
    } else {
        old.iter()
            .zip(new.iter().skip(by))
            .filter(|&pair| pairs(pair))
            .count()
    }
}

/// The shift of the rows drawn, `old` (their hashes, `None` where the row
/// must be drawn anyway), that makes the most of them match the rows of
/// the screen now, `new`; [`Shift::None`] unless a shift matches more rows
/// than staying.
pub fn best_shift(old: &[Option<u64>], new: &[u64]) -> Shift {
    let mut best = (matches(old, new, 0, true), Shift::None);
    for by in 1..new.len() {
        for up in [true, false] {
            let count = matches(old, new, by, up);
            if count > best.0 {
                best = (count, if up { Shift::Up(by) } else { Shift::Down(by) });
            }
        }
    }
    best.1
}

/// Moves the hashes of the rows drawn as their pixels were moved: the rows
/// uncovered are unknown.
pub fn shift_hashes(rows: &mut Vec<Option<u64>>, shift: Shift) {
    let count = rows.len();
    match shift {
        Shift::None => {}
        Shift::Up(by) => {
            rows.drain(..by.min(count));
            rows.resize(count, None);
        }
        Shift::Down(by) => {
            rows.truncate(count.saturating_sub(by));
            rows.splice(0..0, std::iter::repeat_n(None, count - rows.len()));
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn drawn(rows: &[u64]) -> Vec<Option<u64>> {
        rows.iter().copied().map(Some).collect()
    }

    #[test]
    fn a_screen_that_scrolled_is_moved() {
        // Two lines came at the bottom, and the prompt's row changed.
        let old = drawn(&[1, 2, 3, 4, 5]);
        assert_eq!(best_shift(&old, &[3, 4, 9, 6, 7]), Shift::Up(2));
        // Back by one, as a reverse index does.
        assert_eq!(best_shift(&old, &[0, 1, 2, 3, 4]), Shift::Down(1));
        // Nothing moved, or nothing matches.
        assert_eq!(best_shift(&old, &[1, 2, 3, 4, 6]), Shift::None);
        assert_eq!(best_shift(&old, &[7, 8, 9, 10, 11]), Shift::None);
        // Rows that must be drawn anyway never match.
        assert_eq!(best_shift(&[None; 5], &[1, 2, 3, 4, 5]), Shift::None);
        assert_eq!(best_shift(&[], &[]), Shift::None);
    }

    #[test]
    fn the_hashes_move_with_the_pixels() {
        let mut rows = drawn(&[1, 2, 3, 4]);
        shift_hashes(&mut rows, Shift::Up(1));
        assert_eq!(rows, [Some(2), Some(3), Some(4), None]);
        shift_hashes(&mut rows, Shift::Down(2));
        assert_eq!(rows, [None, None, Some(2), Some(3)]);
        shift_hashes(&mut rows, Shift::None);
        shift_hashes(&mut rows, Shift::Up(9));
        assert_eq!(rows, [None; 4]);
        let mut rows = drawn(&[1, 2]);
        shift_hashes(&mut rows, Shift::Down(5));
        assert_eq!(rows, [None, None]);
    }
}
