//! The plain console's prompts, which are [`zbm_tools::console`]'s, under
//! the path the start-up program and the front ends use; ROADMAP.md's DM7
//! moves the prompts to the front ends.

pub use zbm_tools::console::{
    Answer, ConsoleInteraction, TimedPrompt, colorize, decolorize, retry_delay, tput,
};
