//! The plain console's prompts, `zbm_tools::console`'s, for the front ends
//! of `zbm`, which use no adapter: [`ConsoleInteraction`], the
//! [`Interaction`](zbm_platform::interaction::Interaction) of a terminal
//! without the menu, and its colours and retry delay.

pub use zbm_tools::console::{ConsoleInteraction, colorize, retry_delay};
