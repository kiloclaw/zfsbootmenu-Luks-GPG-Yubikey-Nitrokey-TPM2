//! The display and the keyboard without a terminal (ROADMAP.md #19, step 1).
//!
//! The menu normally runs on a terminal: the kernel console, which draws
//! text with a PSF font and translates keys with the image's keymap, or a
//! serial console. This crate lets a front end do both itself:
//!
//! - [`frame`]: pixels in memory, and the area drawn since they were shown;
//! - [`grid`]: text on them, with the image's console fonts
//!   ([`psf`]), at the size `zbm console-autosize` chooses;
//! - [`display`]: a DRM dumb buffer on the output the kernel console
//!   shows, in its mode ([`output`]), which works with any DRM driver,
//!   `simpledrm` on the firmware's framebuffer included, and gives the
//!   output back when dropped;
//! - [`palette`]: the console's colours, and the 256 of xterm;
//! - [`input`] and [`keys`]: key presses from the keyboards' input devices,
//!   with a [`layout`] compiled from XKB data, or on the US layout;
//! - [`vt`]: the virtual terminal taken over (graphics mode, keyboard off)
//!   and given back, so that the kernel console neither draws over the menu
//!   nor passes its keys to the shell.
//!
//! The front end (`zbm_tui::menu::display`) draws Ratatui's cells on a
//! [`grid::Grid`], presents the [`frame::Frame`] on a [`display::Screen`]
//! and reads keys from an [`input::KeySource`]; tests use their own screen
//! and keys.
//!
//! ```
//! use zbm_display::frame::{Area, Frame, Rgb};
//!
//! let mut frame = Frame::new(4, 2);
//! frame.take_damage();
//! frame.fill(Area { x: 1, y: 0, width: 2, height: 1 }, Rgb(255, 0, 0));
//!
//! assert_eq!(frame.pixel(2, 0), Some(Rgb(255, 0, 0)));
//! assert_eq!(frame.take_damage().first().map(|area| area.width), Some(2));
//! ```

pub mod boxes;
pub mod decor;
pub mod display;
pub mod faces;
pub mod frame;
pub mod grid;
pub mod input;
pub mod keys;
pub mod layout;
pub mod outline;
pub mod output;
pub mod palette;
pub mod png;
pub mod psf;
pub mod render;
pub mod speaker;
pub mod vt;
