//! Keyboard layouts: XKB keymaps, compiled when the image is built
//! ([`compile`]) and loaded at boot, with the key state they keep
//! (modifiers, Caps Lock, the third level of `AltGr`) and their dead keys,
//! which a compose table turns into characters (ROADMAP.md #19, step 4).
//!
//! A keymap file is self-contained: loading one reads no other file, so the
//! image needs no XKB data, only the layouts it offers.

use kbvm::lookup::LookupTable;
use kbvm::state_machine::{self, Direction, Event, StateMachine};
use kbvm::xkb::Context;
use kbvm::xkb::compose::{ComposeTable, FeedResult, State as ComposeState};
use kbvm::xkb::diagnostic::Diagnostic;
use kbvm::{Components, Keycode, Keysym, ModifierMask, syms};

pub mod compile;

/// Why a layout could not be compiled or loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// The XKB data has no layout or variant of that name.
    Unknown(String),
    /// A keymap or compose file that does not parse.
    Invalid(String),
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutError::Unknown(name) => write!(f, "{name}: no such keyboard layout"),
            LayoutError::Invalid(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for LayoutError {}

/// What a key typed: its character, when it has one, and the modifiers
/// held.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Typed {
    /// The character, or `None` for a key that types none, a dead key or a
    /// modifier.
    pub char: Option<char>,
    /// The character typed before `char`: the accent of a dead key that
    /// the key does not combine with, which the kernel's keymap types
    /// too.
    pub before: Option<char>,
    /// Whether Shift was held.
    pub shift: bool,
    /// Whether Ctrl was held.
    pub ctrl: bool,
    /// Whether Alt was held.
    pub alt: bool,
}

/// A context that reads no file and no environment variable: keymaps and
/// compose tables come as bytes.
pub(crate) fn sealed_context() -> Context {
    let mut builder = Context::builder();
    builder.enable_default_includes(false);
    builder.enable_environment(false);
    builder.build()
}

/// A keyboard layout and the state of its keys.
pub struct Layout {
    name: String,
    machine: StateMachine,
    lookup: LookupTable,
    state: state_machine::State,
    components: Components,
    dead_keys: Option<(ComposeTable, ComposeState)>,
    /// The dead key that started the sequence being composed.
    dead: Option<Keysym>,
    /// The state machine's events, kept to be wiped: they tell the keys.
    events: Vec<Event>,
}

/// How many events of the state machine one key makes, at most, as far as
/// the vector is allocated ahead.
const EVENTS: usize = 16;

impl std::fmt::Debug for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layout")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Layout {
    /// The layout `name` from a keymap file that [`compile::keymap`] wrote.
    ///
    /// # Errors
    ///
    /// When the keymap does not parse.
    pub fn from_keymap(name: &str, keymap: &[u8]) -> Result<Layout, LayoutError> {
        let mut ignored: Vec<Diagnostic> = Vec::new();
        let keymap = sealed_context()
            .keymap_from_bytes(&mut ignored, None, keymap)
            .map_err(|error| LayoutError::Invalid(format!("{name}: {error}")))?;
        let builder = keymap.to_builder();
        let machine = builder.build_state_machine();
        Ok(Layout {
            name: name.to_owned(),
            state: machine.create_state(),
            machine,
            lookup: builder.build_lookup_table(),
            components: Components::default(),
            dead_keys: None,
            dead: None,
            events: Vec::with_capacity(EVENTS),
        })
    }

    /// The layout with its dead keys composed by `rules`, a compose file
    /// that [`compile::dead_keys`] wrote; without them, dead keys type
    /// nothing.
    ///
    /// # Errors
    ///
    /// When the rules do not parse.
    pub fn with_dead_keys(mut self, rules: &[u8]) -> Result<Layout, LayoutError> {
        let context = sealed_context();
        let mut builder = context.compose_table_builder();
        builder.buffer(rules);
        let mut ignored: Vec<Diagnostic> = Vec::new();
        let table = builder
            .build(&mut ignored)
            .ok_or_else(|| LayoutError::Invalid("the dead keys do not parse".to_owned()))?;
        let state = table.create_state();
        self.dead_keys = Some((table, state));
        Ok(self)
    }

    /// The layout's name: `de`, or `de(nodeadkeys)` for a variant.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Key `code` pressed or released, through the state machine alone.
    fn feed(&mut self, code: u16, down: bool) {
        let direction = if down { Direction::Down } else { Direction::Up };
        let keycode = Keycode::from_evdev(u32::from(code));
        self.machine
            .handle_key(&mut self.state, &mut self.events, keycode, direction);
        for &event in &self.events {
            self.components.apply_event(event);
        }
        zbm_sys::wipe_values(&mut self.events, Event::KeyUp(Keycode::from_evdev(0)));
    }

    /// Whether Caps Lock is on.
    pub fn is_caps_lock(&self) -> bool {
        self.components.mods_locked.contains(ModifierMask::LOCK)
    }

    /// Whether Num Lock is on.
    pub fn is_num_lock(&self) -> bool {
        self.components.mods_locked.contains(ModifierMask::NUM_LOCK)
    }

    /// Turns Num Lock on or off, as its key does: the state machine keeps
    /// the locks, and would undo one set beside it.
    pub fn set_num_lock(&mut self, on: bool) {
        if self.is_num_lock() != on {
            self.feed(crate::keys::NUM_LOCK, true);
            self.feed(crate::keys::NUM_LOCK, false);
        }
    }

    /// Key `code` held down already when this layout took over from
    /// another, so that a modifier held then still counts.
    pub fn hold(&mut self, code: u16) {
        self.feed(code, true);
    }

    /// The key event of `code`: released (0), pressed (1) or repeated (2).
    /// Releases type nothing and return `None`.
    pub fn key(&mut self, code: u16, value: i32) -> Option<Typed> {
        match value {
            0 => {
                self.feed(code, false);
                return None;
            }
            1 => self.feed(code, true),
            _ => {}
        }
        let mods = self.components.mods;
        let mut typed = Typed {
            char: None,
            before: None,
            shift: mods.contains(ModifierMask::SHIFT),
            ctrl: mods.contains(ModifierMask::CONTROL),
            alt: mods.contains(ModifierMask::ALT),
        };
        let keycode = Keycode::from_evdev(u32::from(code));
        // The character, not the control character of Ctrl-k: the console
        // encodes Ctrl itself.
        let symbol = self
            .lookup
            .lookup(self.components.group, mods, keycode)
            .with_ctrl_transform(false)
            .into_iter()
            .next()?;
        if symbol.keysym().is_modifier() {
            return Some(typed);
        }
        typed.char = match &mut self.dead_keys {
            Some((table, state)) => match table.feed(state, symbol.keysym()) {
                Some(FeedResult::Pending) => {
                    self.dead.get_or_insert(symbol.keysym());
                    None
                }
                Some(FeedResult::Composed { string, .. }) => {
                    self.dead = None;
                    single(string)
                }
                Some(FeedResult::Aborted) => {
                    typed.before = self.dead.take().and_then(|dead| accent(table, dead));
                    symbol.char()
                }
                None => symbol.char(),
            },
            None => symbol.char(),
        };
        Some(typed)
    }
}

/// The character that dead key `dead` types alone, as it does followed by
/// the space bar.
fn accent(table: &ComposeTable, dead: Keysym) -> Option<char> {
    let mut state = table.create_state();
    table.feed(&mut state, dead)?;
    match table.feed(&mut state, syms::space)? {
        FeedResult::Composed { string, .. } => single(string),
        FeedResult::Pending | FeedResult::Aborted => None,
    }
}

/// The character of a composed string of one.
fn single(string: Option<&str>) -> Option<char> {
    let mut chars = string?.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests;
