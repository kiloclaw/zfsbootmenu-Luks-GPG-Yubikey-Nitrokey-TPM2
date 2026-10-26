//! Lines and blocks fill their cells, so that neighbours meet.

use pretty_assertions::assert_eq;

use super::*;

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

#[test]
fn light_lines_cross_the_cell_through_its_centre() {
    assert_eq!(
        picture('─', 8, 6),
        [
            "........", "........", "........", "########", "........", "........"
        ]
    );
    assert_eq!(picture('│', 8, 3), ["....#...", "....#...", "....#..."]);
    assert_eq!(
        picture('┌', 8, 6),
        [
            "........", "........", "........", "....####", "....#...", "....#..."
        ]
    );
    assert_eq!(
        picture('┘', 8, 6),
        [
            "....#...", "....#...", "....#...", "#####...", "........", "........"
        ]
    );
    assert_eq!(
        picture('┼', 8, 4),
        ["....#...", "....#...", "########", "....#..."]
    );
    // Rounded corners are drawn square, dashed lines solid.
    assert_eq!(picture('╭', 8, 6), picture('┌', 8, 6));
    assert_eq!(picture('┄', 8, 6), picture('─', 8, 6));
}

#[test]
fn heavy_and_double_lines_are_thicker() {
    assert_eq!(
        picture('━', 8, 6),
        [
            "........", "........", "########", "########", "........", "........"
        ]
    );
    assert_eq!(
        picture('═', 8, 6),
        [
            "........", "........", "########", "........", "########", "........"
        ]
    );
    assert_eq!(picture('║', 8, 2), ["...#.#..", "...#.#.."]);
    // A double corner closes on its outer lines.
    let corner = picture('╔', 8, 8);
    assert_eq!(corner[3], "...#####");
    assert_eq!(corner[7], "...#.#..");
    // Lines get thicker with the cell.
    assert_eq!(picture('│', 16, 1), [".......##......."]);
    // A half line alone stops at the centre.
    assert_eq!(picture('╴', 8, 3), ["........", "#####...", "........"]);
}

#[test]
fn blocks_fill_their_part_of_the_cell() {
    assert_eq!(picture('█', 4, 2), ["####", "####"]);
    assert_eq!(picture('▀', 4, 4), ["####", "####", "....", "...."]);
    assert_eq!(picture('▄', 4, 4), ["....", "....", "####", "####"]);
    assert_eq!(picture('▁', 4, 8)[7], "####");
    assert_eq!(picture('▁', 4, 8)[6], "....");
    assert_eq!(picture('▌', 4, 1), ["##.."]);
    assert_eq!(picture('▏', 8, 1), ["#......."]);
    assert_eq!(picture('▐', 4, 1), ["..##"]);
    assert_eq!(picture('▔', 4, 8)[0], "####");
    assert_eq!(picture('▕', 8, 1), [".......#"]);
    assert_eq!(picture('▟', 4, 4), ["..##", "..##", "####", "####"]);
    assert_eq!(picture('▚', 4, 2), ["##..", "..##"]);
    assert_eq!(mask('░', 2, 1), Some(vec![64, 64]));
    assert_eq!(mask('▓', 1, 1), Some(vec![192]));
}

#[test]
fn other_characters_are_left_to_the_font() {
    assert_eq!(mask('a', 8, 16), None);
    assert_eq!(mask('\u{2577}', 8, 16).map(|mask| mask.len()), Some(128));
    assert_eq!(mask('■', 8, 16), None);
    assert_eq!(mask('─', 0, 0), Some(Vec::new()));
}
