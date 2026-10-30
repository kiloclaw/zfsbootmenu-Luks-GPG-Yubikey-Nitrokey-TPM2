//! Presenting a frame (C73, C74): into the one buffer shown, its damage
//! marked dirty; or into the buffer hidden, with what the frame before drew
//! there too, then flipped to on vertical blank. A driver that does not
//! flip shows that buffer, which keeps being drawn into alone. The DRM
//! calls are behind [`Scanout`], which the tests fake.

use std::io;

use super::swap::Swap;
use crate::frame::{Area, Frame};

/// What presenting needs of a display.
pub(super) trait Scanout {
    /// Copies `area` of `frame` into buffer `index`.
    fn copy(&mut self, index: usize, frame: &Frame, area: Area);

    /// Marks `areas` of buffer `index` dirty, for the drivers that copy
    /// what is marked (virtio-gpu, simpledrm's shadow buffer).
    fn dirty(&mut self, index: usize, areas: &[Area]);

    /// Flips to buffer `index` on the next vertical blank.
    ///
    /// # Errors
    ///
    /// When the driver does not flip.
    fn flip(&mut self, index: usize) -> io::Result<()>;

    /// Shows buffer `index` at once.
    fn show(&mut self, index: usize);

    /// Waits, a while at most, for the flip asked last.
    fn wait_flip(&mut self);
}

/// Which buffer is shown and what the other lacks, and whether two are
/// flipped.
#[derive(Clone, Copy, Debug)]
pub(super) struct Presenter {
    swap: Swap,
    is_flipping: bool,
}

impl Presenter {
    /// Presenting into `count` buffers, the first shown.
    pub(super) fn new(count: usize) -> Presenter {
        Presenter {
            swap: Swap::new(),
            is_flipping: count > 1,
        }
    }

    /// Whether it flips two buffers.
    pub(super) fn is_flipping(&self) -> bool {
        self.is_flipping
    }

    /// Two buffers are no longer flipped, as when showing the first failed.
    pub(super) fn stop_flipping(&mut self) {
        self.is_flipping = false;
    }

    /// Presents what `frame` drew since it was last presented on `scanout`.
    pub(super) fn present(&mut self, scanout: &mut dyn Scanout, frame: &mut Frame) {
        if frame.damage().is_empty() {
            return;
        }
        if self.is_flipping {
            self.flipped(scanout, frame);
        } else {
            let front = self.swap.front();
            for &area in frame.damage() {
                scanout.copy(front, frame, area);
            }
            scanout.dirty(front, frame.damage());
        }
        frame.clear_damage();
    }

    /// Draws into the buffer hidden and flips to it, or shows it when the
    /// driver does not flip.
    fn flipped(&mut self, scanout: &mut dyn Scanout, frame: &Frame) {
        scanout.wait_flip();
        let back = self.swap.back();
        for &area in self.swap.areas(frame.damage()) {
            scanout.copy(back, frame, area);
        }
        if scanout.flip(back).is_err() {
            scanout.show(back);
            self.is_flipping = false;
        }
        self.swap.swapped(frame.damage());
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::frame::Rgb;

    /// Buffers in memory, and what was asked of them.
    struct FakeScanout {
        buffers: Vec<Frame>,
        dirty: Vec<(usize, usize)>,
        flips: Vec<usize>,
        shown: Vec<usize>,
        waits: usize,
        refuses_flips: bool,
    }

    impl FakeScanout {
        fn new(count: usize) -> FakeScanout {
            FakeScanout {
                buffers: (0..count).map(|_| Frame::new(4, 2)).collect(),
                dirty: Vec::new(),
                flips: Vec::new(),
                shown: Vec::new(),
                waits: 0,
                refuses_flips: false,
            }
        }
    }

    impl Scanout for FakeScanout {
        fn copy(&mut self, index: usize, frame: &Frame, area: Area) {
            let pixels: Vec<u32> = (area.y..area.y + area.height)
                .filter_map(|y| frame.row(y))
                .flat_map(|row| row.iter().skip(area.x as usize).take(area.width as usize))
                .copied()
                .collect();
            self.buffers[index].blit(area, &pixels);
        }

        fn dirty(&mut self, index: usize, areas: &[Area]) {
            self.dirty.push((index, areas.len()));
        }

        fn flip(&mut self, index: usize) -> io::Result<()> {
            if self.refuses_flips {
                return Err(io::ErrorKind::Unsupported.into());
            }
            self.flips.push(index);
            Ok(())
        }

        fn show(&mut self, index: usize) {
            self.shown.push(index);
        }

        fn wait_flip(&mut self) {
            self.waits += 1;
        }
    }

    fn cell(x: u32) -> Area {
        Area {
            x,
            y: 0,
            width: 1,
            height: 1,
        }
    }

    fn pixels(frame: &Frame) -> Vec<u32> {
        (0..2)
            .flat_map(|y| frame.row(y).unwrap().to_vec())
            .collect()
    }

    #[test]
    fn one_buffer_gets_the_damage_marked_dirty() {
        let mut scanout = FakeScanout::new(1);
        let mut presenter = Presenter::new(scanout.buffers.len());
        let mut frame = Frame::new(4, 2);
        frame.take_damage();
        frame.fill(cell(1), Rgb(9, 9, 9));
        frame.fill(cell(3), Rgb(9, 9, 9));

        presenter.present(&mut scanout, &mut frame);
        presenter.present(&mut scanout, &mut frame);

        assert!(!presenter.is_flipping());
        assert_eq!(pixels(&scanout.buffers[0]), pixels(&frame));
        // Twice drawn, once presented: nothing new the second time.
        assert_eq!(scanout.dirty, [(0, 2)]);
    }

    #[test]
    fn two_buffers_flip_and_each_shows_the_whole_frame() {
        let mut scanout = FakeScanout::new(2);
        let mut presenter = Presenter::new(scanout.buffers.len());
        let mut frame = Frame::new(4, 2);
        for x in 0..3 {
            frame.fill(cell(x), Rgb(1, 2, u8::try_from(x).unwrap()));
            presenter.present(&mut scanout, &mut frame);
            let shown = scanout.flips.last().copied().unwrap();
            assert_eq!(pixels(&scanout.buffers[shown]), pixels(&frame), "frame {x}");
        }

        assert!(presenter.is_flipping());
        assert_eq!(scanout.flips, [1, 0, 1]);
        assert_eq!(scanout.waits, 3);
        assert!(scanout.dirty.is_empty());
    }

    #[test]
    fn a_driver_that_does_not_flip_shows_the_buffer_and_keeps_to_it() {
        let mut scanout = FakeScanout::new(2);
        scanout.refuses_flips = true;
        let mut presenter = Presenter::new(scanout.buffers.len());
        let mut frame = Frame::new(4, 2);

        presenter.present(&mut scanout, &mut frame);
        assert_eq!(scanout.shown, [1]);
        assert!(!presenter.is_flipping());
        frame.fill(cell(2), Rgb(7, 7, 7));
        presenter.present(&mut scanout, &mut frame);

        // Drawn into the buffer shown since.
        assert_eq!(pixels(&scanout.buffers[1]), pixels(&frame));
        assert_eq!(scanout.dirty, [(1, 1)]);
        let mut stopped = Presenter::new(2);
        stopped.stop_flipping();
        assert!(!stopped.is_flipping());
    }
}
