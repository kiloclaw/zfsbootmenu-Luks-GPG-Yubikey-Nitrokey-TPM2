//! The menu of ZFSBootMenu, on [Ratatui](https://ratatui.rs) and
//! `nucleo-matcher`, in the Elm architecture.
//!
//! The menu is three pure parts and one impure one:
//!
//! - [`Model`]: everything on the screen: the entries, the filter text, the
//!   selection.
//! - [`update`]: `(Model, Message) -> (Model, Vec<Request>)`, what a key
//!   press does; tested with tables of messages.
//! - [`view`]: draws a [`Model`] into a Ratatui frame; tested with snapshots
//!   at three sizes.
//! - [`terminal::run`]: reads keys, calls [`update`] and draws until a
//!   [`Request`] ends the menu; the impure part, tested on the serial console
//!   of a VM.
//!
//! Key bindings are one table, [`keymap::KEYMAP`], which both turns key
//! presses into [`Message`]s and writes the help line, so the help cannot
//! drift from the behaviour.
//!
//! This is the pilot of REFACTOR-ROADMAP.md (R0): one screen, the list of
//! boot environments. R5 adds the others.
//!
//! ```
//! use zbm_app::pilot::Snapshot;
//! use zbm_tui::{Message, Model, Request, update};
//! use zbm_tui::keymap::Intent;
//!
//! let model = Model::new(&Snapshot::example(), false);
//! let (model, _) = update(model, Message::Type('u'));
//! let (_, requests) = update(model, Message::Intent(Intent::Boot));
//! assert_eq!(requests, [Request::Boot("zroot/ROOT/ubuntu".parse()?)]);
//! # Ok::<(), zbm_types::ParseError>(())
//! ```

mod filter;
pub mod keymap;
pub mod menu;
mod model;
mod quiet;
pub mod terminal;
pub mod text;
mod update;
mod view;

pub use model::{Entry, Match, Model};
pub use update::{Message, Request, update};
pub use view::view;
