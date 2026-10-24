//! The new menu (REFACTOR-ROADMAP.md, phases D and R5): the design of
//! `docs/dev/menu-design/` as code.
//!
//! - [`world`]: the snapshot of the machine, the actions and the events,
//!   proposed for `zbm-app`;
//! - [`caps`] and [`theme`]: the visual system;
//! - [`keymap`]: the key map;
//! - [`Model`], [`update`](fn@update) and [`view`](fn@view): the Elm
//!   architecture of the prototype;
//! - [`machine`]: what the menu runs on, and [`session`]: the menu on it;
//! - [`demo`]: a fake machine that answers the actions, for
//!   `zbm-tui --demo`;
//! - [`terminal`] and [`display`]: the menu on a terminal, or on the
//!   display without one (ROADMAP.md #19).

pub mod caps;
pub mod demo;
pub mod display;
pub mod filter;
pub mod format;
pub mod keyboard;
pub mod keymap;
pub mod machine;
pub mod model;
pub mod session;
pub mod terminal;
pub mod theme;
pub mod update;
pub mod view;
pub mod world;

pub use model::Model;
pub use update::{Message, update};
pub use view::view;
