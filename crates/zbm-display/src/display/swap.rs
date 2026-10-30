//! What each of two buffers lacks (C74): the buffer hidden missed what the
//! frame before drew into the other, so a frame copies its own areas and
//! those of the frame before into it before it is shown.

use crate::frame::{Area, DAMAGE_AREAS};

/// Which buffer is shown, and what the frame before drew.
#[derive(Clone, Copy, Debug)]
pub(super) struct Swap {
    front: usize,
    previous: [Area; DAMAGE_AREAS],
    count: usize,
}

impl Swap {
    /// The first buffer shown; nothing drawn yet.
    pub(super) fn new() -> Swap {
        Swap {
            front: 0,
            previous: [Area::default(); DAMAGE_AREAS],
            count: 0,
        }
    }

    /// The buffer shown.
    pub(super) fn front(&self) -> usize {
        self.front
    }

    /// The buffer hidden, which the next frame is drawn into.
    pub(super) fn back(&self) -> usize {
        1 - self.front
    }

    /// The areas to copy into the buffer hidden for a frame that drew
    /// `damage`: those, and the ones of the frame before.
    pub(super) fn areas<'a>(&'a self, damage: &'a [Area]) -> impl Iterator<Item = &'a Area> {
        damage
            .iter()
            .chain(self.previous.get(..self.count).unwrap_or_default())
    }

    /// The buffer hidden is shown now, after a frame that drew `damage`.
    pub(super) fn swapped(&mut self, damage: &[Area]) {
        self.front = self.back();
        self.count = damage.len().min(DAMAGE_AREAS);
        for (kept, &area) in self.previous.iter_mut().zip(damage) {
            *kept = area;
        }
    }
}

/// The drivers of firmware framebuffers, which copy a shadow buffer to the
/// firmware's memory: a flip would copy the whole frame, so they keep one
/// buffer, marked dirty where drawn.
const FIRMWARE_DRIVERS: &[&str] = &["simpledrm", "efidrm", "vesadrm", "ofdrm"];

/// Whether the driver `name` is one of a firmware framebuffer.
pub(super) fn is_firmware_driver(name: &str) -> bool {
    FIRMWARE_DRIVERS.contains(&name)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn area(x: u32) -> Area {
        Area {
            x,
            y: 0,
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn each_buffer_gets_what_the_frame_before_drew_into_the_other() {
        let mut swap = Swap::new();
        assert_eq!((swap.front(), swap.back()), (0, 1));
        let first = [area(1)];
        assert_eq!(swap.areas(&first).copied().collect::<Vec<_>>(), first);
        swap.swapped(&first);
        assert_eq!((swap.front(), swap.back()), (1, 0));

        // Buffer 0 missed the first frame's area.
        let second = [area(2), area(3)];
        let copied: Vec<Area> = swap.areas(&second).copied().collect();
        assert_eq!(copied, [area(2), area(3), area(1)]);
        swap.swapped(&second);
        assert_eq!(swap.front(), 0);
        let copied: Vec<Area> = swap.areas(&[]).copied().collect();
        assert_eq!(copied, second);
    }

    #[test]
    fn firmware_drivers_keep_one_buffer() {
        assert!(is_firmware_driver("simpledrm"));
        assert!(!is_firmware_driver("virtio_gpu"));
        assert!(!is_firmware_driver("i915"));
    }
}
