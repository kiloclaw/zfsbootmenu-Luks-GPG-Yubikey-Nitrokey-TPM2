//! The menu of ZFSBootMenu (REFACTOR-ROADMAP.md, R5), on
//! [Ratatui](https://ratatui.rs) and `nucleo-matcher`, in the Elm
//! architecture: [`menu`], with every string it shows from the message
//! catalogue of [`text`].
//!
//! The menu runs on a [`menu::machine::Machine`]: the fake machine of
//! `zbm-tui --demo` and of the tests, or the real one, through `zbm-app`,
//! in the boot image.
//!
//! ```
//! use zbm_tui::menu::caps::Capabilities;
//! use zbm_tui::menu::demo::{DemoOptions, Session};
//! use zbm_tui::menu::theme::Theme;
//!
//! let theme = Theme::new(Capabilities::FULL);
//! let mut session = Session::new(&DemoOptions::default(), theme);
//! session.start();
//! assert!(session.model().started);
//! ```

pub mod menu;
mod quiet;
pub mod text;
