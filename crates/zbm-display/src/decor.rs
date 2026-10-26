//! The lines drawn over a cell's character: underlines of five kinds and a
//! line through it, as SGR 4, 4:2 to 4:5, 21 and 9 ask for, in the colour
//! of SGR 58 or the character's.

use crate::frame::{Area, Frame, Rgb};

/// How a cell is underlined.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Underline {
    /// Not at all.
    #[default]
    None,
    /// One straight line.
    Single,
    /// Two straight lines.
    Double,
    /// A wave.
    Curly,
    /// Dots.
    Dotted,
    /// Dashes.
    Dashed,
}

/// The lines over a cell, and their colour.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Decoration {
    /// The underline.
    pub underline: Underline,
    /// The underline's colour; the character's when `None`.
    pub underline_colour: Option<Rgb>,
    /// Whether a line goes through the character.
    pub strikeout: bool,
}

impl Decoration {
    /// A single underline in the character's colour, or none.
    pub fn underlined(on: bool) -> Decoration {
        Decoration {
            underline: if on {
                Underline::Single
            } else {
                Underline::None
            },
            ..Decoration::default()
        }
    }
}

/// Where a font puts the lines of a cell: their thickness, and the top
/// row of the underline and of the strikeout, from the cell's top. Without
/// a row, the underline sits at the cell's bottom and the strikeout across
/// its middle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Lines {
    /// How thick the lines are, in pixels.
    pub thickness: u32,
    /// The underline's top row.
    pub underline: Option<u32>,
    /// The strikeout's top row.
    pub strikeout: Option<u32>,
}

impl Lines {
    /// Lines `thickness` pixels thick, where the cell puts them.
    pub fn thick(thickness: u32) -> Lines {
        Lines {
            thickness,
            ..Lines::default()
        }
    }
}

/// Draws the lines of `decoration` over `area`, as `lines` places them,
/// the character in `fg`.
pub fn draw(frame: &mut Frame, area: Area, decoration: Decoration, fg: Rgb, lines: Lines) {
    let thickness = lines.thickness.max(1);
    let under = decoration.underline_colour.unwrap_or(fg);
    let (width, height) = (area.width, area.height);
    let lowest = height.saturating_sub(thickness);
    let top = lines.underline.map_or(lowest, |row| row.min(lowest));
    let pen = Pen {
        area,
        thickness,
        colour: under,
    };
    match decoration.underline {
        Underline::None => {}
        Underline::Single => pen.line(frame, top, |_| true),
        Underline::Double => {
            pen.line(frame, top, |_| true);
            // The second line below the first when there is room.
            let second = if top + 3 * thickness <= height {
                top + 2 * thickness
            } else {
                top.saturating_sub(2 * thickness)
            };
            pen.line(frame, second, |_| true);
        }
        Underline::Dotted => pen.line(frame, top, |x| (x / thickness).is_multiple_of(2)),
        // A gap of a quarter of the cell, across the cell's edges.
        Underline::Dashed => pen.line(frame, top, |x| x >= width / 8 && x < width - width / 8),
        Underline::Curly => curl(frame, area, under, (top, thickness)),
    }
    if decoration.strikeout {
        let pen = Pen { colour: fg, ..pen };
        let middle = (height / 2).saturating_sub(thickness / 2);
        pen.line(frame, lines.strikeout.unwrap_or(middle).min(lowest), |_| {
            true
        });
    }
}

/// What a line of a cell is drawn with.
#[derive(Clone, Copy)]
struct Pen {
    area: Area,
    thickness: u32,
    colour: Rgb,
}

impl Pen {
    /// A line across the cell from row `top`, at the columns `lit` takes.
    fn line(self, frame: &mut Frame, top: u32, lit: impl Fn(u32) -> bool) {
        let Pen {
            area,
            thickness,
            colour,
        } = self;
        for y in top..top.saturating_add(thickness).min(area.height) {
            for x in (0..area.width).filter(|&x| lit(x)) {
                frame.set(area.x + x, area.y + y, colour);
            }
        }
    }
}

/// A wave, one period per cell, from the underline's row `base` up two
/// thicknesses.
fn curl(frame: &mut Frame, area: Area, colour: Rgb, (base, thickness): (u32, u32)) {
    let (width, height) = (area.width.max(2), area.height);
    let rise = 2 * thickness;
    for x in 0..area.width {
        // A triangle wave: up for half the cell, down for the other half.
        let phase = x * 2 * rise / width;
        let offset = if phase < rise {
            phase
        } else {
            2 * rise - phase
        };
        let top = base.saturating_sub(offset);
        for y in top..top.saturating_add(thickness).min(height) {
            frame.set(area.x + x, area.y + y, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const WHITE: Rgb = Rgb(255, 255, 255);
    const RED: Rgb = Rgb(255, 0, 0);

    /// A cell of 8×8 pixels decorated with `decoration`, as text: `#` for
    /// white, `r` for red, `.` for black.
    fn picture(decoration: Decoration) -> Vec<String> {
        let mut frame = Frame::new(8, 8);
        let area = Area {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        draw(&mut frame, area, decoration, WHITE, Lines::thick(1));
        (0..8)
            .map(|y| {
                (0..8)
                    .map(|x| match frame.pixel(x, y) {
                        Some(WHITE) => '#',
                        Some(RED) => 'r',
                        _ => '.',
                    })
                    .collect()
            })
            .collect()
    }

    fn underline(underline: Underline) -> Decoration {
        Decoration {
            underline,
            ..Decoration::default()
        }
    }

    #[test]
    fn underlines_of_each_kind_sit_at_the_bottom() {
        let rows = |kind| picture(underline(kind))[5..].to_vec();

        assert_eq!(rows(Underline::None), ["........"; 3]);
        assert_eq!(
            rows(Underline::Single),
            ["........", "........", "########"]
        );
        assert_eq!(
            rows(Underline::Double),
            ["########", "........", "########"]
        );
        assert_eq!(
            rows(Underline::Dotted),
            ["........", "........", "#.#.#.#."]
        );
        assert_eq!(
            rows(Underline::Dashed),
            ["........", "........", ".######."]
        );
        // Up for half the cell, down for the other half, into the next.
        assert_eq!(rows(Underline::Curly), ["....##..", "..##..##", "##......"]);
        assert_eq!(Decoration::underlined(true), underline(Underline::Single));
        assert_eq!(Decoration::underlined(false), Decoration::default());
    }

    #[test]
    fn strikeout_crosses_the_middle_and_underlines_take_their_colour() {
        let picture = picture(Decoration {
            underline: Underline::Single,
            underline_colour: Some(RED),
            strikeout: true,
        });

        assert_eq!(picture[4], "########");
        assert_eq!(picture[7], "rrrrrrrr");
        assert_eq!(picture[3], "........");
    }

    #[test]
    fn the_font_places_the_lines() {
        let mut frame = Frame::new(8, 8);
        let area = Area {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        let lines = Lines {
            thickness: 1,
            underline: Some(5),
            strikeout: Some(2),
        };
        let decoration = Decoration {
            underline: Underline::Double,
            strikeout: true,
            ..Decoration::default()
        };

        draw(&mut frame, area, decoration, WHITE, lines);

        let lit: Vec<u32> = (0..8)
            .filter(|&y| frame.pixel(0, y) == Some(WHITE))
            .collect();
        // The double underline's second line goes below where there is
        // room, above otherwise.
        assert_eq!(lit, [2, 5, 7]);
        let low = Lines {
            underline: Some(1),
            ..lines
        };
        let mut frame = Frame::new(8, 8);
        draw(&mut frame, area, decoration, WHITE, low);
        let lit: Vec<u32> = (0..8)
            .filter(|&y| frame.pixel(0, y) == Some(WHITE))
            .collect();
        assert_eq!(lit, [1, 2, 3]);
    }
}
