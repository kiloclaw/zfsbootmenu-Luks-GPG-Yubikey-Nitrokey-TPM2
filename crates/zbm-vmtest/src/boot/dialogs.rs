//! The dialogs of the start-up view (ROADMAP.md, R5 step 4): with
//! the menu in the image, the start-up program of the Rust runtime asks in
//! dialogs what the legacy runtime asks on the console. The patterns match
//! the screen, as the view redraws only the cells that change.
//!
//! A dialog stays on the screen until it is answered or goes on by itself,
//! so a test that acts on one notes the screen it matched, then waits for
//! it to change ([`Vm::wait_for_change`](crate::vm::Vm::wait_for_change))
//! before it looks for the next prompt. What is typed into an unlock
//! dialog does not show: the first change is the dialog closing, which the
//! serial console brings in pieces. A screen pattern matches only once the
//! screen stays the same for a moment, so the last lines of a dialog that
//! closes are not taken for one that asks again. Asked again, a dialog
//! comes back with what went wrong or the tries left, so that its screen
//! differs from the one answered even when the harness missed the screens
//! between.

use crate::error::Result;
use crate::vm::Vm;

/// The unlock dialog asks for a LUKS passphrase.
pub const LUKS: &str = r"screen:│ Passphrase: +│";

/// The unlock dialog asks for a TPM2 PIN.
pub const TPM2PIN: &str = r"screen:│ TPM2 PIN: +│";

/// The unlock dialog asks for the passphrase of a GnuPG key file.
pub const GPG: &str = r"screen:│ Key file passphrase: +│";

/// The unlock dialog waits for the device that holds a key file.
pub const KEY_DEVICE: &str = r"screen:Waiting for the key device";

/// A question: the device of a LUKS volume is missing. Its sentence starts
/// the line; a long device name may wrap the rest.
pub const MISSING: &str = r"screen:│ The LUKS device ";

/// A question: a device that `zbm.wait_for` names is missing.
pub const DEVICES: &str = r"screen:Devices Are Missing";

/// A question: no pool, or not the pool asked for, was imported.
pub const RETRY: &str = r"screen:│ Its devices may not be there yet";

/// How long the screen may take to change once a dialog was acted on, in
/// seconds.
pub const CLOSE: u64 = 600;

/// Types `reply` and Enter into the dialog the screen shows, then waits
/// for the screen to change.
///
/// # Errors
///
/// When the console closed, or the screen did not change in time.
pub fn reply(vm: &mut Vm<'_>, reply: &str) -> Result<()> {
    let shown = vm.console().screen();
    vm.sendline(reply)?;
    vm.wait_for_change(&shown, CLOSE)
}

/// Whether `c` draws a box: the lines of the box-drawing block.
fn is_box(c: char) -> bool {
    ('\u{2500}'..='\u{257f}').contains(&c)
}

/// The words of `screen`, its lines and box lines turned into single
/// spaces: what a dialog wraps over several lines reads as one sentence.
pub fn words(screen: &str) -> String {
    let words: Vec<&str> = screen
        .split(|c: char| c.is_whitespace() || is_box(c))
        .filter(|word| !word.is_empty())
        .collect();
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::expect::{Matcher, on_screen};

    /// The unlock dialog, its field holding `typed`.
    fn dialog(typed: &str) -> String {
        format!(
            "│    ┌──────────────────────────┐   │\n\
             │    │   Unlock luks-1234       │   │\n\
             │    │ The TPM is locked against│   │\n\
             │    │ PIN guessing.            │   │\n\
             │    │ Passphrase: {typed:<12} │   │\n\
             │    └──────────────────────────┘   │\n"
        )
    }

    fn matches(pattern: &str, screen: &str) -> bool {
        on_screen(&[Matcher::new(pattern).unwrap()], screen).is_some()
    }

    #[test]
    fn an_unlock_dialog_matches_only_while_its_field_is_empty() {
        assert!(matches(LUKS, &dialog("")));
        // Revealed, what was typed shows.
        assert!(!matches(LUKS, &dialog("pw")));
        assert!(!matches(TPM2PIN, &dialog("")));
    }

    #[test]
    fn the_words_of_a_dialog_read_across_its_lines() {
        let words = words(&dialog(""));
        assert!(words.contains("The TPM is locked against PIN guessing."));
        assert_eq!(super::words(" ┌─┐\n│ a  b │\n"), "a b");
    }
}
