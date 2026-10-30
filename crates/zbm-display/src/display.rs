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
use std::time::Duration;

use drm::control::{
    ClipRect, Device as ControlDevice, ModeTypeFlags, PageFlipFlags, ResourceHandles, connector,
    crtc, property,
};

use crate::frame::{Area, DAMAGE_AREAS, Frame};
use buffer::Buffers;
pub use buffer::{Format, copy_area_as};
use present::{Presenter, Scanout};

mod buffer;
pub mod holders;
mod present;
mod swap;

/// The time between two frames at 60 Hz, when the mode says no rate.
pub const FRAME: Duration = Duration::from_micros(16_667);

/// The time between two frames at `hertz`.
fn interval(hertz: u32) -> Duration {
    if hertz == 0 {
        FRAME
    } else {
        Duration::from_secs(1) / hertz
    }
}
use crate::output::{ConnectorDesc, Link, ModeDesc, choose_mode};

/// What frames are presented on.
pub trait Screen {
    /// The size, as (width, height) in pixels.
    fn size(&self) -> (u32, u32);

    /// The time between two frames of the display: no more frames than its
    /// refresh rate are presented (C75).
    fn interval(&self) -> Duration {
        FRAME
    }

    /// Turns the display off, as blanking does after a while (C64), or on
    /// again; nothing where it cannot be.
    fn set_power(&mut self, on: bool) {
        let _ = on;
    }

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
    copy_area_as(frame, area, (buffer, pitch, Format::Xrgb8888));
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

/// How a display is shown.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// A mode of this size (width, height) when the output has one
    /// (`zbm.console_mode`, C60); the one shown now otherwise.
    pub mode: Option<(u16, u16)>,
    /// One buffer, marked dirty where drawn, rather than two flipped on
    /// vertical blank (C74).
    pub single: bool,
}

/// How long a flip is waited for before the buffer is drawn into anyway.
const FLIP_WAIT: Duration = Duration::from_millis(100);

/// The display: an output of a DRM card showing dumb buffers, until it is
/// dropped, which gives the output back to the kernel console.
#[derive(Debug)]
pub struct Display {
    card: Card,
    connector: connector::Handle,
    crtc: crtc::Handle,
    mode: drm::control::Mode,
    /// The CRTC as the kernel console left it, restored when dropped.
    saved: crtc::Info,
    buffers: Option<Buffers>,
    /// Which buffer is shown, and whether two are flipped (C74).
    presenter: Presenter,
    /// Whether a flip waits for its vertical blank.
    is_pending: bool,
    size: (u32, u32),
    /// The time between two frames of the mode shown.
    interval: Duration,
    /// The connector's power property, for blanking (C64).
    dpms: Option<property::Handle>,
}

/// An error with what was being done.
fn context(what: &str) -> impl FnOnce(io::Error) -> io::Error + '_ {
    move |error| io::Error::new(error.kind(), format!("{what}: {error}"))
}

/// Whether `card`'s driver is one of a firmware framebuffer.
fn is_firmware(card: &Card) -> bool {
    drm::Device::get_driver(card)
        .is_ok_and(|driver| driver.name().to_str().is_some_and(swap::is_firmware_driver))
}

impl Display {
    /// [`Display::open_with`] the default options.
    ///
    /// # Errors
    ///
    /// As [`Display::open_with`].
    pub fn open(path: &Path) -> io::Result<Display> {
        Display::open_with(path, Options::default())
    }

    /// Takes the output of the card at `path` that [`choose_mode`] picks,
    /// the one the kernel console shows in the mode it shows unless
    /// `options` asks for another, and shows a black buffer on it.
    ///
    /// # Errors
    ///
    /// When the card cannot be opened, has no output with a display, or
    /// refuses the buffers or the mode (another program is the DRM master:
    /// `PermissionDenied`).
    pub fn open_with(path: &Path, options: Options) -> io::Result<Display> {
        let file = OpenOptions::new().read(true).write(true).open(path);
        let card = Card(file.map_err(context(&path.display().to_string()))?);
        // The first program to open a card is its master already; asking
        // again fails then, and is needed after another master left.
        let _ = drm::Device::acquire_master_lock(&card);
        let resources = card.resource_handles().map_err(context("resources"))?;
        let (connector, mode, crtc) = output(&card, &resources, options.mode)?;
        let (width, height) = mode.size();
        let size = (u32::from(width), u32::from(height));
        let saved = card.get_crtc(crtc).map_err(context("CRTC"))?;
        let is_double = !options.single && !is_firmware(&card);
        let buffers = Buffers::new(&card, size, is_double)?;
        let first = buffers.list.first().map(|buffer| buffer.framebuffer);
        let dpms = power_property(&card, connector);
        let mut display = Display {
            presenter: Presenter::new(buffers.list.len()),
            card,
            connector,
            crtc,
            mode,
            saved,
            buffers: Some(buffers),
            is_pending: false,
            size,
            interval: interval(mode.vrefresh()),
            dpms,
        };
        let shown = display
            .card
            .set_crtc(crtc, first, (0, 0), &[connector], Some(mode));
        if let Err(error) = shown {
            display.presenter.stop_flipping();
            return Err(context("mode")(error));
        }
        Ok(display)
    }

    /// The format of its pixels.
    pub fn format(&self) -> Format {
        self.buffers
            .as_ref()
            .map_or(Format::Xrgb8888, |buffers| buffers.format)
    }

    /// Whether it flips two buffers on vertical blank.
    pub fn is_flipping(&self) -> bool {
        self.presenter.is_flipping()
    }
}

/// Waits, a while at most, for the flip asked last on `card` to happen, so
/// that the buffer it hid is no longer scanned out.
fn wait_flip(card: &Card, is_pending: &mut bool) {
    if std::mem::take(is_pending) && zbm_sys::wait_readable(card, Some(FLIP_WAIT)).unwrap_or(false)
    {
        let _ = card.receive_events().map(Iterator::count);
    }
}

/// The DRM side of presenting: the card's buffers, and its CRTC to flip or
/// set.
struct Outputs<'a> {
    card: &'a Card,
    crtc: crtc::Handle,
    connector: connector::Handle,
    mode: drm::control::Mode,
    buffers: &'a mut Buffers,
    is_pending: &'a mut bool,
}

impl Scanout for Outputs<'_> {
    fn copy(&mut self, index: usize, frame: &Frame, area: Area) {
        let format = self.buffers.format;
        if let Some(buffer) = self.buffers.list.get_mut(index) {
            buffer.copy(frame, area, format);
        }
    }

    fn dirty(&mut self, index: usize, areas: &[Area]) {
        let Some(buffer) = self.buffers.list.get(index) else {
            return;
        };
        let edge = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
        let mut clips = [ClipRect::new(0, 0, 0, 0); DAMAGE_AREAS];
        for (clip, area) in clips.iter_mut().zip(areas) {
            *clip = ClipRect::new(
                edge(area.x),
                edge(area.y),
                edge(area.x + area.width),
                edge(area.y + area.height),
            );
        }
        let count = areas.len().min(DAMAGE_AREAS);
        let _ = self
            .card
            .dirty_framebuffer(buffer.framebuffer, clips.get(..count).unwrap_or_default());
    }

    fn flip(&mut self, index: usize) -> io::Result<()> {
        let buffer = self
            .buffers
            .list
            .get(index)
            .ok_or(io::ErrorKind::NotFound)?;
        self.card
            .page_flip(self.crtc, buffer.framebuffer, PageFlipFlags::EVENT, None)?;
        *self.is_pending = true;
        Ok(())
    }

    fn show(&mut self, index: usize) {
        if let Some(buffer) = self.buffers.list.get(index) {
            let _ = self.card.set_crtc(
                self.crtc,
                Some(buffer.framebuffer),
                (0, 0),
                &[self.connector],
                Some(self.mode),
            );
        }
    }

    fn wait_flip(&mut self) {
        wait_flip(self.card, self.is_pending);
    }
}

/// The connector's `DPMS` property, which turns its display off and on.
fn power_property(card: &Card, connector: connector::Handle) -> Option<property::Handle> {
    let properties = card.get_properties(connector).ok()?;
    properties.iter().find_map(|(&handle, _)| {
        let info = card.get_property(handle).ok()?;
        (info.name().to_bytes() == b"DPMS").then_some(handle)
    })
}
/// The connector, its mode and the CRTC to use, from what the card says,
/// in a mode of the `wanted` size when there is one.
fn output(
    card: &Card,
    resources: &ResourceHandles,
    wanted: Option<(u16, u16)>,
) -> io::Result<(connector::Handle, drm::control::Mode, crtc::Handle)> {
    let crtcs = resources.crtcs();
    let mut infos = Vec::new();
    let mut descs = Vec::new();
    for &handle in resources.connectors() {
        let info = card
            .get_connector(handle, false)
            .map_err(context("connector"))?;
        descs.push(connector_desc(card, resources, &info));
        infos.push(info);
    }
    let choice = choose_mode(&descs, wanted)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no output with a display"))?;
    let missing = || io::Error::new(io::ErrorKind::NotFound, "no such output");
    let info = infos.get(choice.connector).ok_or_else(missing)?;
    let mode = *info.modes().get(choice.mode).ok_or_else(missing)?;
    let crtc = *crtcs.get(choice.crtc).ok_or_else(missing)?;
    Ok((info.handle(), mode, crtc))
}

/// What choosing an output needs to know of the connector `info`.
fn connector_desc(
    card: &Card,
    resources: &ResourceHandles,
    info: &connector::Info,
) -> ConnectorDesc {
    let crtcs = resources.crtcs();
    let crtc_index = |crtc: crtc::Handle| crtcs.iter().position(|&c| c == crtc);
    let current = info
        .current_encoder()
        .and_then(|encoder| card.get_encoder(encoder).ok()?.crtc());
    // The mode shown now, kept when the connector offers it: the same size
    // and refresh, whatever flags the kernel console's copy has.
    let current_mode = current
        .and_then(|crtc| card.get_crtc(crtc).ok()?.mode())
        .and_then(|shown| {
            info.modes()
                .iter()
                .position(|mode| mode.size() == shown.size() && mode.vrefresh() == shown.vrefresh())
        });
    let possible_crtcs = info
        .encoders()
        .iter()
        .filter_map(|&encoder| card.get_encoder(encoder).ok())
        .flat_map(|encoder| resources.filter_crtcs(encoder.possible_crtcs()))
        .filter_map(crtc_index)
        .collect();
    ConnectorDesc {
        link: match info.state() {
            connector::State::Connected => Link::Connected,
            connector::State::Unknown => Link::Unknown,
            connector::State::Disconnected => Link::Disconnected,
        },
        modes: info.modes().iter().map(|mode| mode_desc(*mode)).collect(),
        current_crtc: current.and_then(crtc_index),
        current_mode,
        possible_crtcs,
    }
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
        let Some(buffers) = self.buffers.as_mut() else {
            frame.clear_damage();
            return Ok(());
        };
        let mut outputs = Outputs {
            card: &self.card,
            crtc: self.crtc,
            connector: self.connector,
            mode: self.mode,
            buffers,
            is_pending: &mut self.is_pending,
        };
        self.presenter.present(&mut outputs, frame);
        Ok(())
    }

    fn interval(&self) -> Duration {
        self.interval
    }

    fn set_power(&mut self, on: bool) {
        // DPMS: 0 on, 3 off.
        if let Some(property) = self.dpms {
            let _ = self
                .card
                .set_property(self.connector, property, if on { 0 } else { 3 });
        }
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        wait_flip(&self.card, &mut self.is_pending);
        let saved = &self.saved;
        let _ = self.card.set_crtc(
            saved.handle(),
            saved.framebuffer(),
            saved.position(),
            &[self.connector],
            saved.mode(),
        );
        if let Some(buffers) = self.buffers.take() {
            buffers.destroy(&self.card);
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
