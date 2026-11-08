//! The keyboard layout the menu assumes: what is typed is translated by key
//! position with the image's XKB keymaps (`zbm_app::model::Keyboard`,
//! ROADMAP.md UI1), and the unlock dialog names the layout typing uses.

/// The console's own layout when the image names none: the kernel's and
/// the console's default.
pub const CONSOLE: &str = "us";

/// The US layout, by whose keys a character the key map binds nowhere
/// means something (`docs/dev/menu-design/keymap.md`, rule 3).
pub(crate) const US: &str = "us";
