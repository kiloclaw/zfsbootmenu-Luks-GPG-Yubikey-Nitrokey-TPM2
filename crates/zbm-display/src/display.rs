//! The display, through DRM: a dumb buffer, which needs no graphics driver
//! of its own (`simpledrm`, on the firmware's framebuffer, has one), shown
//! on the chosen output; and the output given back as it was.

#![allow(
    clippy::disallowed_methods,
    reason = "the DRM device is this module's system interface: it opens it and lists /dev/dri"
)]

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};

use drm::buffer::DrmFourcc;
use drm::control::dumbbuffer::DumbBuffer;
use drm::control::{
    ClipRect, Device as ControlDevice, ModeTypeFlags, ResourceHandles, connector, crtc, framebuffer,
};

use crate::frame::{Area, Frame};
use crate::output::{ConnectorDesc, Link, ModeDesc, choose};

/// What frames are presented on.
pub trait Screen {
    /// The size, as (width, height) in pixels.
    fn size(&self) -> (u32, u32);

    /// Shows what was drawn on `frame` since it was last presented.
    ///
    /// # Errors
    ///
    /// When the frame cannot be shown.
    fn present(&mut self, frame: &mut Frame) -> io::Result<()>;
}

/// The first DRM card below `dev` (`/dev/dri/card0`, ...).
pub fn first_card(dev: &Path) -> Option<PathBuf> {
    let dri = dev.join("dri");
    let mut cards: Vec<String> = std::fs::read_dir(&dri)
        .ok()?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| {
            name.strip_prefix("card")
                .is_some_and(|n| n.parse::<u32>().is_ok())
        })
        .collect();
    cards.sort_by_key(|name| {
        name.trim_start_matches("card")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });
    cards.first().map(|name| dri.join(name))
}

/// Copies `area` of `frame` into a buffer of `XRGB8888` pixels whose rows
/// are `pitch` bytes apart.
pub fn copy_area(frame: &Frame, area: Area, buffer: &mut [u8], pitch: usize) {
    let (Ok(x), Ok(width)) = (usize::try_from(area.x), usize::try_from(area.width)) else {
        return;
    };
    for y in area.y..area.y.saturating_add(area.height) {
        let Some(row) = frame.row(y).and_then(|row| row.get(x..x + width)) else {
            continue;
        };
        let Some(start) = usize::try_from(y).ok().map(|y| y * pitch + x * 4) else {
            continue;
        };
        let Some(target) = buffer.get_mut(start..start + width * 4) else {
            continue;
        };
        for (bytes, pixel) in target.chunks_exact_mut(4).zip(row) {
            bytes.copy_from_slice(&pixel.to_le_bytes());
        }
    }
}

/// An open DRM card.
#[derive(Debug)]
struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl ControlDevice for Card {}

/// The display: an output of a DRM card showing a dumb buffer, until it is
/// dropped, which gives the output back to the kernel console.
#[derive(Debug)]
pub struct Display {
    card: Card,
    connector: connector::Handle,
    /// The CRTC as the kernel console left it, restored when dropped.
    saved: crtc::Info,
    buffer: Option<DumbBuffer>,
    framebuffer: framebuffer::Handle,
    size: (u32, u32),
}

/// An error with what was being done.
fn context(what: &str) -> impl FnOnce(io::Error) -> io::Error + '_ {
    move |error| io::Error::new(error.kind(), format!("{what}: {error}"))
}

impl Display {
    /// Takes the output of the card at `path` that [`choose`] picks, the
    /// one the kernel console shows in the mode it shows, and shows a black
    /// buffer on it.
    ///
    /// # Errors
    ///
    /// When the card cannot be opened, has no output with a display, or
    /// refuses the buffer or the mode (another program is the DRM master).
    pub fn open(path: &Path) -> io::Result<Display> {
        let file = OpenOptions::new().read(true).write(true).open(path);
        let card = Card(file.map_err(context(&path.display().to_string()))?);
        // The first program to open a card is its master already; asking
        // again fails then, and is needed after another master left.
        let _ = drm::Device::acquire_master_lock(&card);
        let resources = card.resource_handles().map_err(context("resources"))?;
        let (connector, mode, crtc) = output(&card, &resources)?;
        let (width, height) = mode.size();
        let size = (u32::from(width), u32::from(height));
        let saved = card.get_crtc(crtc).map_err(context("CRTC"))?;
        let buffer = card
            .create_dumb_buffer(size, DrmFourcc::Xrgb8888, 32)
            .map_err(context("dumb buffer"))?;
        let framebuffer = card
            .add_framebuffer(&buffer, 24, 32)
            .map_err(context("framebuffer"))?;
        let display = Display {
            card,
            connector,
            saved,
            buffer: Some(buffer),
            framebuffer,
            size,
        };
        display
            .card
            .set_crtc(crtc, Some(framebuffer), (0, 0), &[connector], Some(mode))
            .map_err(context("mode"))?;
        Ok(display)
    }
}

/// The connector, its mode and the CRTC to use, from what the card says.
fn output(
    card: &Card,
    resources: &ResourceHandles,
) -> io::Result<(connector::Handle, drm::control::Mode, crtc::Handle)> {
    let crtcs = resources.crtcs();
    let mut infos = Vec::new();
    let mut descs = Vec::new();
    for &handle in resources.connectors() {
        let info = card
            .get_connector(handle, false)
            .map_err(context("connector"))?;
        let crtc_index = |crtc: crtc::Handle| crtcs.iter().position(|&c| c == crtc);
        let current = info
            .current_encoder()
            .and_then(|encoder| card.get_encoder(encoder).ok()?.crtc());
        let current_crtc = current.and_then(crtc_index);
        // The mode shown now, kept when the connector offers it: the same
        // size and refresh, whatever flags the kernel console's copy has.
        let current_mode = current
            .and_then(|crtc| card.get_crtc(crtc).ok()?.mode())
            .and_then(|shown| {
                info.modes().iter().position(|mode| {
                    mode.size() == shown.size() && mode.vrefresh() == shown.vrefresh()
                })
            });
        let possible_crtcs = info
            .encoders()
            .iter()
            .filter_map(|&encoder| card.get_encoder(encoder).ok())
            .flat_map(|encoder| resources.filter_crtcs(encoder.possible_crtcs()))
            .filter_map(crtc_index)
            .collect();
        descs.push(ConnectorDesc {
            link: match info.state() {
                connector::State::Connected => Link::Connected,
                connector::State::Unknown => Link::Unknown,
                connector::State::Disconnected => Link::Disconnected,
            },
            modes: info.modes().iter().map(|mode| mode_desc(*mode)).collect(),
            current_crtc,
            current_mode,
            possible_crtcs,
        });
        infos.push(info);
    }
    let choice = choose(&descs)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no output with a display"))?;
    let missing = || io::Error::new(io::ErrorKind::NotFound, "no such output");
    let info = infos.get(choice.connector).ok_or_else(missing)?;
    let mode = *info.modes().get(choice.mode).ok_or_else(missing)?;
    let crtc = *crtcs.get(choice.crtc).ok_or_else(missing)?;
    Ok((info.handle(), mode, crtc))
}

fn mode_desc(mode: drm::control::Mode) -> ModeDesc {
    let (width, height) = mode.size();
    ModeDesc {
        width,
        height,
        preferred: mode.mode_type().contains(ModeTypeFlags::PREFERRED),
    }
}

impl Screen for Display {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn present(&mut self, frame: &mut Frame) -> io::Result<()> {
        let Some(area) = frame.take_damage() else {
            return Ok(());
        };
        let Some(buffer) = self.buffer.as_mut() else {
            return Ok(());
        };
        let pitch = usize::try_from(drm::buffer::Buffer::pitch(buffer)).unwrap_or(0);
        let mut mapping = self.card.map_dumb_buffer(buffer).map_err(context("map"))?;
        copy_area(frame, area, mapping.as_mut(), pitch);
        drop(mapping);
        let edge = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
        let clip = ClipRect::new(
            edge(area.x),
            edge(area.y),
            edge(area.x + area.width),
            edge(area.y + area.height),
        );
        // Drivers that scan the buffer out directly do without; the others
        // (virtio-gpu, simpledrm's shadow buffer) copy what is marked dirty.
        let _ = self.card.dirty_framebuffer(self.framebuffer, &[clip]);
        Ok(())
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        let saved = &self.saved;
        let _ = self.card.set_crtc(
            saved.handle(),
            saved.framebuffer(),
            saved.position(),
            &[self.connector],
            saved.mode(),
        );
        let _ = self.card.destroy_framebuffer(self.framebuffer);
        if let Some(buffer) = self.buffer.take() {
            let _ = self.card.destroy_dumb_buffer(buffer);
        }
        let _ = drm::Device::release_master_lock(&self.card);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::frame::Rgb;

    #[test]
    fn areas_are_copied_as_little_endian_pixels_into_the_rows_of_the_buffer() {
        let mut frame = Frame::new(3, 2);
        let area = Area {
            x: 1,
            y: 1,
            width: 2,
            height: 1,
        };
        frame.fill(area, Rgb(1, 2, 3));
        let mut buffer = vec![0_u8; 16 * 2];

        copy_area(&frame, area, &mut buffer, 16);
        copy_area(&frame, Area { y: 5, ..area }, &mut buffer, 16);
        copy_area(&frame, area, &mut [0_u8; 4], 16);

        assert_eq!(&buffer[..16], &[0; 16]);
        assert_eq!(&buffer[16..20], &[0; 4]);
        assert_eq!(&buffer[20..28], &[3, 2, 1, 0, 3, 2, 1, 0]);
    }

    #[test]
    fn the_first_card_is_the_lowest_numbered() {
        let root = std::env::temp_dir().join(format!("zbm-display-dri-{}", std::process::id()));
        std::fs::create_dir_all(root.join("dri")).unwrap();
        for name in ["card10", "card1", "renderD128", "cardx"] {
            std::fs::write(root.join("dri").join(name), "").unwrap();
        }

        assert_eq!(first_card(&root), Some(root.join("dri/card1")));
        assert_eq!(first_card(&root.join("none")), None);
        assert!(Display::open(&root.join("dri/missing")).is_err());
        assert!(Display::open(&root.join("dri/card1")).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
