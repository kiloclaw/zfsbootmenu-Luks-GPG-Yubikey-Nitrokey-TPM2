//! The new menu (REFACTOR-ROADMAP.md, phases D and R5): the design of
//! `docs/dev/menu-design/` as code.
//!
//! - [`world`]: the snapshot of the machine, the actions and the events,
//!   proposed for `zbm-app`;
//! - [`caps`] and [`theme`]: the visual system;
//! - [`keymap`]: the key map;
//! - [`Model`], [`update`] and [`view`]: the Elm architecture of the
//!   prototype;
//! - [`demo`]: a fake machine that answers the actions, for
//!   `zbm-tui --demo`.

pub mod caps;
pub mod demo;
pub mod filter;
pub mod format;
pub mod keymap;
pub mod model;
pub mod theme;
pub mod update;
pub mod world;

pub use model::Model;
pub use update::{Message, update};
