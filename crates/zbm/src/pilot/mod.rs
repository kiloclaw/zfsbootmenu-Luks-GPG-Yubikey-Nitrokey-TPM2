//! The front ends of the pilots of REFACTOR-ROADMAP.md (R0), which run the
//! refactored layers beside today's code:
//!
//! - [`zluks`]: `zluks --pilot open`, a LUKS volume opened with a key file;
//! - [`menu`]: `zbm-menu-pilot`, the boot environment list of the new menu.

pub mod menu;
pub mod zluks;
