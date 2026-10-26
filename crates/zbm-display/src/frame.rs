//! Pixels in memory, which the display shows once they are presented.

/// A colour as red, green and blue.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Black.
    pub const BLACK: Rgb = Rgb(0, 0, 0);

    /// The colour as a pixel of the `XRGB8888` format, which every DRM
    /// driver takes for dumb buffers.
    pub fn xrgb(self) -> u32 {
        u32::from(self.0) << 16 | u32::from(self.1) << 8 | u32::from(self.2)
    }

    /// The colour of an `XRGB8888` pixel.
    pub fn from_xrgb(pixel: u32) -> Rgb {
        let [_, red, green, blue] = pixel.to_be_bytes();
        Rgb(red, green, blue)
    }
}

/// A rectangle of pixels: from `x`, `y`, `width` wide and `height` high.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Area {
    /// The left edge.
    pub x: u32,
    /// The top edge.
    pub y: u32,
    /// The width.
    pub width: u32,
    /// The height.
    pub height: u32,
}

impl Area {
    /// The smallest area that holds both.
    fn union(self, other: Area) -> Area {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Area {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
}

/// An image of `width` by `height` pixels, row by row, with the area drawn
/// since it was last presented.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    damage: Option<Area>,
}

impl Frame {
    /// A black frame, all of it to be presented.
    pub fn new(width: u32, height: u32) -> Frame {
        let count = usize::try_from(u64::from(width) * u64::from(height)).unwrap_or(0);
        Frame {
            width,
            height,
            pixels: vec![0; count],
            damage: Some(Area {
                x: 0,
                y: 0,
                width,
                height,
            }),
        }
    }

    /// The size, as (width, height) in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The colour at `x`, `y`, if the frame has that pixel.
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgb> {
        self.index(x, y)
            .and_then(|index| self.pixels.get(index))
            .map(|&pixel| Rgb::from_xrgb(pixel))
    }

    /// The pixels of row `y`, as `XRGB8888`.
    pub fn row(&self, y: u32) -> Option<&[u32]> {
        let start = self.index(0, y)?;
        let width = usize::try_from(self.width).ok()?;
        self.pixels.get(start..start + width)
    }

    fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height)
            .then(|| usize::try_from(u64::from(y) * u64::from(self.width) + u64::from(x)).ok())
            .flatten()
    }

    /// Paints `area`, clipped to the frame, in `colour`.
    pub fn fill(&mut self, area: Area, colour: Rgb) {
        let right = area.x.saturating_add(area.width).min(self.width);
        let bottom = area.y.saturating_add(area.height).min(self.height);
        for y in area.y..bottom {
            for x in area.x..right {
                self.set(x, y, colour);
            }
        }
        self.touch(area);
    }

    /// Sets one pixel, if the frame has it; the caller records the damage.
    pub(crate) fn set(&mut self, x: u32, y: u32, colour: Rgb) {
        if let Some(pixel) = self
            .index(x, y)
            .and_then(|index| self.pixels.get_mut(index))
        {
            *pixel = colour.xrgb();
        }
    }

    /// Moves the rows from `top`, `height` of them, by `by` rows, up when
    /// `up` and down otherwise, within those rows: a scrolled screen is
    /// moved rather than drawn again. The rows the move uncovers keep
    /// their pixels, for the caller to draw.
    pub fn shift_rows(&mut self, top: u32, height: u32, by: u32, up: bool) {
        let height = height.min(self.height.saturating_sub(top));
        if by == 0 || by >= height {
            return;
        }
        let row = |rows: u32| usize::try_from(u64::from(rows) * u64::from(self.width)).ok();
        let (Some(start), Some(length), Some(moved)) = (row(top), row(height), row(by)) else {
            return;
        };
        let Some(rows) = self.pixels.get_mut(start..start + length) else {
            return;
        };
        if up {
            rows.copy_within(moved.., 0);
        } else {
            rows.copy_within(..length - moved, moved);
        }
        self.touch(Area {
            x: 0,
            y: top,
            width: self.width,
            height,
        });
    }

    /// Records that `area` changed.
    pub(crate) fn touch(&mut self, area: Area) {
        let clipped = Area {
            width: area.width.min(self.width.saturating_sub(area.x)),
            height: area.height.min(self.height.saturating_sub(area.y)),
            ..area
        };
        if clipped.width == 0 || clipped.height == 0 {
            return;
        }
        self.damage = Some(self.damage.map_or(clipped, |damage| damage.union(clipped)));
    }

    /// The area drawn since the last call, which presenting copies to the
    /// display; `None` when nothing changed.
    pub fn take_damage(&mut self) -> Option<Area> {
        self.damage.take()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn colours_are_xrgb_pixels() {
        let colour = Rgb(0x12, 0x34, 0x56);

        assert_eq!(colour.xrgb(), 0x0012_3456);
        assert_eq!(Rgb::from_xrgb(0xff12_3456), colour);
    }

    #[test]
    fn a_new_frame_is_black_and_all_to_be_presented() {
        let mut frame = Frame::new(4, 3);

        assert_eq!(frame.size(), (4, 3));
        assert_eq!(frame.pixel(3, 2), Some(Rgb::BLACK));
        assert_eq!(frame.pixel(4, 0), None);
        assert_eq!(frame.row(3), None);
        let all = Area {
            x: 0,
            y: 0,
            width: 4,
            height: 3,
        };
        assert_eq!(frame.take_damage(), Some(all));
        assert_eq!(frame.take_damage(), None);
    }

    #[test]
    fn fills_are_clipped_and_their_damage_grows_to_hold_them() {
        let mut frame = Frame::new(4, 3);
        frame.take_damage();
        let red = Rgb(255, 0, 0);

        frame.fill(
            Area {
                x: 3,
                y: 1,
                width: 5,
                height: 1,
            },
            red,
        );
        frame.fill(
            Area {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            red,
        );

        assert_eq!(frame.row(1), Some(&[0, 0, 0, 0x00ff_0000][..]));
        assert_eq!(frame.pixel(0, 0), Some(red));
        let damage = Area {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
        };
        assert_eq!(frame.take_damage(), Some(damage));
        frame.touch(Area {
            x: 9,
            y: 9,
            width: 2,
            height: 2,
        });
        assert_eq!(frame.take_damage(), None);
    }

    #[test]
    fn rows_are_moved_within_their_band() {
        let mut frame = Frame::new(2, 5);
        for y in 0..5 {
            frame.fill(
                Area {
                    x: 0,
                    y,
                    width: 2,
                    height: 1,
                },
                Rgb(u8::try_from(y).unwrap(), 0, 0),
            );
        }
        frame.take_damage();
        let column =
            |frame: &Frame| -> Vec<u8> { (0..5).map(|y| frame.pixel(1, y).unwrap().0).collect() };

        frame.shift_rows(1, 3, 1, true);
        assert_eq!(column(&frame), [0, 2, 3, 3, 4]);
        assert_eq!(
            frame.take_damage().map(|area| (area.y, area.height)),
            Some((1, 3))
        );
        frame.shift_rows(0, 5, 2, false);
        assert_eq!(column(&frame), [0, 2, 0, 2, 3]);
        // Nothing to move, or more than there is.
        frame.shift_rows(0, 5, 0, true);
        frame.shift_rows(3, 9, 2, true);
        assert_eq!(frame.take_damage().map(|area| area.height), Some(5));
        assert_eq!(column(&frame), [0, 2, 0, 2, 3]);
    }
}
