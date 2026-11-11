//! The new menu (ROADMAP.md, phases D and R5): the design of
//! `docs/dev/menu-design/` as code.
//!
//! - [`world`]: the snapshot of the machine, the actions and the events,
//!   proposed for `zbm-app`;
//! - [`caps`] and [`theme`]: the visual system;
//! - [`keymap`]: the key map;
//! - [`Model`], [`update`](fn@update) and [`view`](fn@view): the Elm
//!   architecture of the prototype;
//! - [`machine`]: what the menu runs on, and [`session`]: the menu on it;
//! - [`pager`]: a text in the recovery shell, with the menu's keys;
//! - [`demo`]: a fake machine that answers the actions, for
//!   `zbm-tui --demo`;
//! - [`terminal`] and [`display`]: the menu on a terminal, or on the
//!   display without one (ROADMAP.md #19); [`cells`]: on the cells of the
//!   console on the display (UI6).

pub mod announce;
pub mod caps;
pub mod cells;
pub mod demo;
pub mod display;
pub mod filter;
pub mod format;
pub mod frontend;
pub mod keyboard;
pub mod keymap;
pub mod lines;
pub mod machine;
pub mod model;
pub mod pager;
pub mod screen;
pub mod secret;
pub mod session;
pub mod startup;
pub mod term_keys;
pub mod terminal;
pub mod theme;
pub mod update;
pub mod view;
pub mod world;

pub use model::Model;
pub use update::{Message, update};
pub use view::view;
