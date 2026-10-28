use pretty_assertions::assert_eq;
use proptest::prelude::*;

use super::*;
use crate::DatasetName;

#[test]
fn controls_are_escaped_and_the_rest_kept() {
    let cases = [
        ("zroot/ROOT/void", "zroot/ROOT/void"),
        ("tab\there", "tab\\x09here"),
        ("line\nbreak", "line\\x0abreak"),
        ("\u{1b}[2J", "\\x1b[2J"),
        ("del\u{7f}", "del\\x7f"),
        ("c1 \u{9b}31m", "c1 \\x9b31m"),
        ("é ü 漢字 🦀", "é ü 漢字 🦀"),
        ("evil\u{202e}gnp.exe", "evil\\u{202e}gnp.exe"),
        ("\u{2066}isolate\u{2069}", "\\u{2066}isolate\\u{2069}"),
        ("", ""),
    ];
    for (text, shown) in cases {
        assert_eq!(Printable(text).to_string(), shown, "{text:?}");
        assert_eq!(Printable(text).is_plain(), text == shown, "{text:?}");
    }
}

#[test]
fn untrusted_text_is_shown_escaped_and_parsed_to_be_used() {
    let name = Untrusted::new("zroot/ROOT/\u{1b}[31mvoid".to_owned());
    assert_eq!(name.printable().to_string(), "zroot/ROOT/\\x1b[31mvoid");
    assert_eq!(
        format!("{name:?}"),
        "Untrusted(\"zroot/ROOT/\\x1b[31mvoid\")"
    );
    assert!(name.parse::<DatasetName>().is_err());

    let good = Untrusted::new("zroot/ROOT/void");
    assert_eq!(
        good.parse::<DatasetName>().map(|name| name.to_string()),
        Ok("zroot/ROOT/void".to_owned())
    );
    assert_eq!(*good.get(), "zroot/ROOT/void");
    assert_eq!(good.into_inner(), "zroot/ROOT/void");
}

proptest! {
    /// Whatever the text, what is shown holds no character that acts on a
    /// terminal, and the text comes back from it when nothing was escaped.
    #[test]
    fn nothing_shown_acts_on_a_terminal(text in any::<String>()) {
        let shown = Printable(&text).to_string();
        prop_assert!(!shown.contains(must_escape), "{shown:?}");
        if Printable(&text).is_plain() {
            prop_assert_eq!(shown, text);
        }
    }
}
