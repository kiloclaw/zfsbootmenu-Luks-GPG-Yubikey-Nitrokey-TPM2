//! What the menu knows about the machine: the snapshot it reads, the
//! actions it asks for and the events it receives, from `zbm-app`
//! ([`zbm_app::model`]). The prototype answers them with
//! [`FakeMachine`](super::demo::FakeMachine).

pub use zbm_app::model::*;
