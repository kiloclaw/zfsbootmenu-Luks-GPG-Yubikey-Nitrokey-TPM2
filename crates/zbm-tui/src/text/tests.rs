use fluent_bundle::FluentResource;
use pretty_assertions::assert_eq;

use super::*;

/// Runs `test` with `language` as this thread's, then restores the old.
fn with<T>(language: Language, test: impl FnOnce() -> T) -> T {
    let old = set_language(language);
    let result = test();
    set_language(old);
    result
}

#[test]
fn the_catalogue_parses_without_errors_or_duplicates() {
    let parsed = FluentResource::try_new(ENGLISH.to_owned());
    assert!(
        parsed.is_ok(),
        "{:?}",
        parsed.err().map(|(_, errors)| errors)
    );
    let mut bundle = FluentBundle::<FluentResource>::new(vec![]);
    let added = bundle.add_resource(FluentResource::try_new(ENGLISH.to_owned()).unwrap());
    assert!(added.is_ok(), "{added:?}");
}

#[test]
fn messages_take_their_arguments_and_attributes() {
    assert_eq!(
        text!("list-count-of", shown = 2_usize, total = 7_usize),
        "2 of 7"
    );
    assert_eq!(text!("key-ctrl", key = "C"), "Ctrl-C");
    assert_eq!(text!("snapshots-none.text"), "Press n to take one.");
    assert_eq!(title(id!("action-read-write")), "Read/Write");
    assert_eq!(get(id!("action-read-write")), "read/write");
}

#[test]
fn a_missing_message_is_its_id() {
    assert_eq!(get(Id::new("no-such-message")), "no-such-message");
    assert_eq!(title(Id::new("no-such-message")), "no-such-message.title");
    assert_eq!(
        get(Id::new("list-no-matches.missing")),
        "list-no-matches.missing"
    );
}

#[test]
fn numbers_keep_their_decimals() {
    assert_eq!(
        text!("list-count-of", shown = decimal("1.5"), total = 3_u32),
        "1.5 of 3"
    );
    assert_eq!(
        text!("list-count-of", shown = decimal("x"), total = 3_u32),
        "x of 3"
    );
}

#[test]
fn the_pseudo_language_accents_and_lengthens_text_but_not_values() {
    with(Language::Pseudo, || {
        assert_eq!(language(), Language::Pseudo);
        assert_eq!(text!("key-enter"), "ÉÉñtéér");
        let pools = text!("pool-host", hostid = "deadbeef");
        assert_eq!(pools, "hhóstt deadbeef");
    });
    assert_eq!(language(), Language::English);
    assert_eq!(text!("key-enter"), "Enter");
}

#[test]
fn pseudo_text_is_a_third_longer() {
    let english = "Pools and LUKS Volumes";
    let letters = english.chars().filter(|c| c.is_alphabetic()).count();
    let pseudo = pseudo(english);
    assert_eq!(
        pseudo.chars().count(),
        english.chars().count() + letters.div_ceil(3)
    );
    assert!(pseudo.contains('ó'));
    assert_eq!(accented('z'), 'z');
}

#[test]
fn languages_are_named() {
    assert_eq!(Language::from_name("en"), Some(Language::English));
    assert_eq!(Language::from_name("pseudo"), Some(Language::Pseudo));
    assert_eq!(Language::from_name("xx"), None);
    assert_eq!(Language::default(), Language::English);
    assert_eq!(id!("key-esc").as_str(), "key-esc");
}
