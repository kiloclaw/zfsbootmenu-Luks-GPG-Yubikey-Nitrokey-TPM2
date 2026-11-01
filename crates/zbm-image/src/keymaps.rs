//! The keyboard layouts of an image (ROADMAP.md #19, step 4), compiled
//! from the build host's XKB data for the console on the display: each
//! layout one self-contained keymap, `<name>.xkb`, beside `dead-keys`, the
//! compose rules of the dead keys, and `layouts`, their names in the order
//! asked for, the first the console's own.

use std::path::Path;

use zbm_display::layout::LayoutError;
use zbm_display::layout::compile;
use zbm_platform::Files;

/// The compose rules of the dead keys, in the layouts' directory.
pub const DEAD_KEYS: &str = "dead-keys";

/// The names of the layouts, one per line, in the layouts' directory.
pub const INDEX: &str = "layouts";

/// The files of the layouts' directory, compiled.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Compiled {
    /// Each file, by name: the keymaps, the index and the dead keys.
    pub files: Vec<(String, Vec<u8>)>,
    /// What was compiled.
    pub done: Vec<String>,
    /// The layouts that could not be compiled, and why.
    pub failed: Vec<String>,
    /// Why the dead keys could not be, when they could not.
    pub no_dead_keys: Option<String>,
}

/// Compiles the layouts `layouts`, each once, from the XKB data `xkb`, and
/// the dead keys from the compose rules `compose` (read through `files`).
pub fn compile_layouts(
    files: &dyn Files,
    xkb: &Path,
    compose: &Path,
    layouts: &[String],
) -> Compiled {
    let mut compiled = Compiled::default();
    let mut index = String::new();
    for (position, name) in layouts.iter().enumerate() {
        if layouts
            .get(..position)
            .is_some_and(|before| before.contains(name))
        {
            continue;
        }
        match compile::keymap(xkb, name) {
            Ok(keymap) => {
                compiled
                    .done
                    .push(format!("{name}: {} bytes", keymap.len()));
                compiled
                    .files
                    .push((format!("{name}.xkb"), keymap.into_bytes()));
                index.push_str(name);
                index.push('\n');
            }
            Err(error) => compiled.failed.push(error.to_string()),
        }
    }
    compiled.files.push((INDEX.to_owned(), index.into_bytes()));
    let rules = files
        .read(compose)
        .map_err(|error| LayoutError::Invalid(format!("{}: {error}", compose.display())))
        .and_then(|compose| compile::dead_keys(&compose));
    match rules {
        Ok(rules) => {
            compiled
                .done
                .push(format!("{DEAD_KEYS}: {} rules", rules.lines().count()));
            compiled
                .files
                .push((DEAD_KEYS.to_owned(), rules.into_bytes()));
        }
        Err(error) => {
            compiled.no_dead_keys = Some(format!("{error}; dead keys will type nothing"));
        }
    }
    compiled
}
