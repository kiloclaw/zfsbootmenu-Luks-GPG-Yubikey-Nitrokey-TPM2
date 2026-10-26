//! The shapes fill their cells, so that neighbours meet.

use crate::boxes::mask;

/// `c` in a cell of `width` × `height`, as rows of `#` (full), `.` (empty)
/// and `+` (partly covered).
fn picture(c: char, width: u32, height: u32) -> Vec<String> {
    let coverage = mask(c, width, height).unwrap();
    coverage
        .chunks(usize::try_from(width).unwrap())
        .map(|row| {
            row.iter()
                .map(|&value| match value {
                    0 => '.',
                    255 => '#',
                    _ => '+',
                })
                .collect()
        })
        .collect()
}

/// Whether pixel `x`, `y` of `picture` is covered at all.
fn covered(picture: &[String], x: usize, y: usize) -> bool {
    picture[y].as_bytes()[x] != b'.'
}

#[test]
fn diagonals_run_from_corner_to_corner() {
    let rising = picture('╱', 8, 16);
    let falling = picture('╲', 8, 16);
    let cross = picture('╳', 8, 16);
    for (x, y) in [(0, 15), (7, 0), (3, 8)] {
        assert!(covered(&rising, x, y), "{rising:?}");
        assert!(covered(&falling, x, 15 - y), "{falling:?}");
        assert!(covered(&cross, x, y) && covered(&cross, x, 15 - y));
    }
    assert!(!covered(&rising, 0, 0) && !covered(&falling, 7, 0));
    // Edges are smooth: no pixel of a thin diagonal is wholly covered.
    assert!(rising.iter().all(|row| !row.contains('#')));
}

#[test]
fn braille_dots_sit_in_two_columns_of_four() {
    let all = picture('⣿', 8, 16);
    let dots = |picture: &[String]| -> Vec<(usize, usize)> {
        (0..16)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .filter(|&(x, y)| picture[y].as_bytes()[x] == b'#')
            .map(|(x, y)| (x / 4, y / 4))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    assert_eq!(dots(&all).len(), 8);
    // Dot 1 is at the top left, dot 8 at the bottom right.
    assert_eq!(dots(&picture('⠁', 8, 16)), [(0, 0)]);
    assert_eq!(dots(&picture('⢀', 8, 16)), [(1, 3)]);
    assert_eq!(dots(&picture('⠀', 8, 16)), []);
}

#[test]
fn powerline_triangles_fill_the_cell_s_height() {
    let right = picture('\u{e0b0}', 8, 16);
    let left = picture('\u{e0b2}', 8, 16);
    for y in [1, 4, 12] {
        assert!(right[y].starts_with('#') && right[y].ends_with('.'));
        assert!(left[y].ends_with('#') && left[y].starts_with('.'));
    }
    assert!(right[7].starts_with("#######"));
    let arrow = picture('\u{e0b1}', 8, 16);
    assert!(covered(&arrow, 0, 0) && covered(&arrow, 7, 7) && !covered(&arrow, 0, 7));
    let lower_left = picture('\u{e0b8}', 8, 16);
    let upper_right = picture('\u{e0be}', 8, 16);
    assert!(lower_left[15].starts_with("#######") && !covered(&lower_left, 7, 0));
    assert!(upper_right[0].ends_with("#######") && !covered(&upper_right, 0, 15));
    assert_eq!(picture('\u{e0b9}', 8, 16), picture('╲', 8, 16));
    assert_eq!(picture('\u{e0bb}', 8, 16), picture('╱', 8, 16));
    assert!(mask('\u{e0b4}', 8, 16).is_none());
}

#[test]
fn sextants_skip_the_patterns_that_older_characters_have() {
    let thirds = |c| -> Vec<String> {
        let rows = picture(c, 2, 3);
        rows.iter().map(|row| row.replace('.', " ")).collect()
    };
    assert_eq!(thirds('\u{1fb00}'), ["# ", "  ", "  "]);
    assert_eq!(thirds('\u{1fb13}'), ["  ", "# ", "# "]);
    // After the left half (sextants 1, 3, 5) comes sextant 2, 3 and 5's
    // neighbour; the last one is all but the top left.
    assert_eq!(thirds('\u{1fb14}'), [" #", "# ", "# "]);
    assert_eq!(thirds('\u{1fb3b}'), [" #", "##", "##"]);
    assert!(mask('\u{1fb3c}', 2, 3).is_none());
}
