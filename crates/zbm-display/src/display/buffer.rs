//! The dumb buffers the display scans out: each with its framebuffer and
//! its memory, mapped from the start to the end (C72), in `XRGB8888`, or
//! `RGB565` when the driver takes no other (C65); one, or two flipped on
//! vertical blank (C74).

use std::io;
use std::os::fd::AsFd as _;

use drm::buffer::{Buffer as _, DrmFourcc};
use drm::control::dumbbuffer::DumbBuffer;
use drm::control::{Device as _, framebuffer};
use zbm_sys::drm::Mapped;

use super::{Card, context};
use crate::frame::{Area, Frame};

/// How the pixels of a buffer are laid out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// 32 bits: 8 unused, then red, green and blue, little-endian.
    Xrgb8888,
    /// 16 bits: 5 of red, 6 of green, 5 of blue, little-endian.
    Rgb565,
}

impl Format {
    /// The formats tried, in order.
    const ALL: [Format; 2] = [Format::Xrgb8888, Format::Rgb565];

    /// The DRM name, the bits per pixel and the colour depth.
    fn drm(self) -> (DrmFourcc, u32, u32) {
        match self {
            Format::Xrgb8888 => (DrmFourcc::Xrgb8888, 32, 24),
            Format::Rgb565 => (DrmFourcc::Rgb565, 16, 16),
        }
    }

    /// The bytes of a pixel.
    pub fn bytes(self) -> usize {
        match self {
            Format::Xrgb8888 => 4,
            Format::Rgb565 => 2,
        }
    }
}

/// Copies `area` of `frame` into a buffer of `format` pixels whose rows are
/// `pitch` bytes apart.
pub fn copy_area_as(
    frame: &Frame,
    area: Area,
    (buffer, pitch, format): (&mut [u8], usize, Format),
) {
    let (Ok(x), Ok(width)) = (usize::try_from(area.x), usize::try_from(area.width)) else {
        return;
    };
    let bytes = format.bytes();
    for y in area.y..area.y.saturating_add(area.height) {
        let Some(row) = frame.row(y).and_then(|row| row.get(x..x + width)) else {
            continue;
        };
        let Some(start) = usize::try_from(y).ok().map(|y| y * pitch + x * bytes) else {
            continue;
        };
        let Some(target) = buffer.get_mut(start..start + width * bytes) else {
            continue;
        };
        match format {
            Format::Xrgb8888 => {
                for (out, pixel) in target.chunks_exact_mut(4).zip(row) {
                    out.copy_from_slice(&pixel.to_le_bytes());
                }
            }
            Format::Rgb565 => {
                for (out, &pixel) in target.chunks_exact_mut(2).zip(row) {
                    out.copy_from_slice(&rgb565(pixel).to_le_bytes());
                }
            }
        }
    }
}

/// An `XRGB8888` pixel as `RGB565`.
pub fn rgb565(pixel: u32) -> u16 {
    let [_, red, green, blue] = pixel.to_be_bytes();
    (u16::from(red) >> 3) << 11 | (u16::from(green) >> 2) << 5 | u16::from(blue) >> 3
}

/// One buffer: its memory, the framebuffer that shows it, mapped.
#[derive(Debug)]
pub(super) struct Buffer {
    dumb: DumbBuffer,
    pub(super) framebuffer: framebuffer::Handle,
    mapping: Mapped,
    pitch: usize,
}

impl Buffer {
    /// A black buffer of `size` pixels in `format`.
    fn new(card: &Card, size: (u32, u32), format: Format) -> io::Result<Buffer> {
        let (fourcc, bpp, depth) = format.drm();
        let dumb = card
            .create_dumb_buffer(size, fourcc, bpp)
            .map_err(context("dumb buffer"))?;
        let made = (|| {
            let framebuffer = card
                .add_framebuffer(&dumb, depth, bpp)
                .map_err(context("framebuffer"))?;
            let pitch = dumb.pitch();
            let length = usize::try_from(u64::from(pitch) * u64::from(size.1))
                .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))?;
            match Mapped::dumb_buffer(card.as_fd(), dumb.handle().into(), length) {
                Ok(mapping) => Ok((framebuffer, mapping, pitch)),
                Err(error) => {
                    let _ = card.destroy_framebuffer(framebuffer);
                    Err(context("map")(error))
                }
            }
        })();
        match made {
            Ok((framebuffer, mapping, pitch)) => Ok(Buffer {
                dumb,
                framebuffer,
                mapping,
                pitch: usize::try_from(pitch).unwrap_or(0),
            }),
            Err(error) => {
                let _ = card.destroy_dumb_buffer(dumb);
                Err(error)
            }
        }
    }

    /// Copies `area` of `frame` in.
    pub(super) fn copy(&mut self, frame: &Frame, area: Area, format: Format) {
        copy_area_as(frame, area, (self.mapping.bytes_mut(), self.pitch, format));
    }

    /// Unmaps the buffer, then frees it.
    fn destroy(self, card: &Card) {
        drop(self.mapping);
        let _ = card.destroy_framebuffer(self.framebuffer);
        let _ = card.destroy_dumb_buffer(self.dumb);
    }
}

/// The buffers of a display, in one format: the one shown first, and a
/// second to draw into while the other is shown, when there is one.
#[derive(Debug)]
pub(super) struct Buffers {
    pub(super) list: Vec<Buffer>,
    pub(super) format: Format,
}

impl Buffers {
    /// One buffer of `size`, in the first format the card takes, and a
    /// second when `double` and the card has the memory.
    pub(super) fn new(card: &Card, size: (u32, u32), double: bool) -> io::Result<Buffers> {
        let mut refused = None;
        for format in Format::ALL {
            match Buffer::new(card, size, format) {
                Ok(first) => {
                    let mut list = vec![first];
                    if double && let Ok(second) = Buffer::new(card, size, format) {
                        list.push(second);
                    }
                    return Ok(Buffers { list, format });
                }
                Err(error) => refused = Some(error),
            }
        }
        Err(refused.unwrap_or_else(|| io::ErrorKind::Unsupported.into()))
    }

    /// Frees them all.
    pub(super) fn destroy(self, card: &Card) {
        for buffer in self.list {
            buffer.destroy(card);
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::frame::Rgb;

    #[test]
    fn pixels_are_packed_into_rgb565() {
        assert_eq!(rgb565(Rgb(255, 255, 255).xrgb()), 0xffff);
        assert_eq!(rgb565(Rgb(255, 0, 0).xrgb()), 0xf800);
        assert_eq!(rgb565(Rgb(0, 255, 0).xrgb()), 0x07e0);
        assert_eq!(rgb565(Rgb(0, 0, 255).xrgb()), 0x001f);
        assert_eq!(rgb565(Rgb(8, 4, 8).xrgb()), 0x0821);
    }

    #[test]
    fn an_area_is_copied_in_the_buffer_s_format() {
        let mut frame = Frame::new(3, 2);
        frame.fill(
            Area {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            },
            Rgb(255, 0, 0),
        );
        let area = Area {
            x: 1,
            y: 1,
            width: 2,
            height: 1,
        };
        let mut wide = vec![0; 24];
        copy_area_as(&frame, area, (&mut wide, 12, Format::Xrgb8888));
        assert_eq!(&wide[16..24], &[0, 0, 255, 0, 0, 0, 255, 0]);
        let mut narrow = vec![0; 12];
        copy_area_as(&frame, area, (&mut narrow, 6, Format::Rgb565));
        assert_eq!(narrow, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0xf8, 0, 0xf8]);
        assert_eq!((Format::Xrgb8888.bytes(), Format::Rgb565.bytes()), (4, 2));
    }
}
