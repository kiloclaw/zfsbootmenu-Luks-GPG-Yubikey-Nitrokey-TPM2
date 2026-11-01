//! Tests of the configuration loader.

use pretty_assertions::assert_eq;

use super::*;

fn text(text: &str) -> Value {
    Value::Text(text.to_owned())
}

#[test]
fn scalars_keep_what_the_file_says() {
    assert_eq!(resolve_plain(""), Value::Null);
    assert_eq!(resolve_plain("~"), Value::Null);
    assert_eq!(resolve_plain("True"), Value::Bool(true));
    assert_eq!(resolve_plain("FALSE"), Value::Bool(false));
    // A version stays as written; Perl read 3.10 as the number 3.1.
    assert_eq!(resolve_plain("3.10"), text("3.10"));
    assert_eq!(resolve_plain("007"), text("007"));
    assert_eq!(resolve_plain("no"), text("no"));
}

#[test]
fn values_are_read_as_the_keys_want_them() {
    assert_eq!(Value::Bool(false).text().as_deref(), Some("false"));
    assert_eq!(text("3.10").text().as_deref(), Some("3.10"));
    assert_eq!(Value::Null.text(), None);
    assert_eq!(Value::seq(Vec::new()).text(), None);
    assert!(Value::Bool(false).has_value() && !text("").has_value() && !Value::Null.has_value());

    for no in ["", "0", "false", "No", "OFF", " no "] {
        assert!(!text(no).truthy(), "{no:?}");
    }
    for yes in ["1", "yes", "on", "anything", "0.0"] {
        assert!(text(yes).truthy(), "{yes:?}");
    }
    assert!(!Value::Null.truthy() && Value::Bool(true).truthy() && !Value::Bool(false).truthy());
    assert!(Value::map(Vec::new()).truthy());

    assert_eq!(text("3").integer(), Some(3));
    assert_eq!(text(" -2 ").integer(), Some(-2));
    assert_eq!(text("0o17").integer(), Some(15));
    assert_eq!(text("0x10").integer(), Some(16));
    assert_eq!(text("3.9").integer(), Some(3));
    assert_eq!(text("1e3").integer(), Some(1000));
    assert_eq!(text("three").integer(), None);
    assert_eq!(text("inf").integer(), None);
    assert_eq!(Value::Bool(true).integer(), None);
}

#[test]
fn documents() {
    assert!(load("").is_err());
    assert!(load("- a\n").is_err());
    assert!(load("just text\n").is_err());
    assert!(load("a: 1\na: 2\n").is_err());
    assert!(load("a: [\n").is_err());
    let config = load("a: &x 1\nb: *x\n---\nc: 2\n").unwrap();
    assert_eq!(config.get("b"), Some(text("1")));
    assert_eq!(config.get("c"), None);
    let config = load(
        "a: !!str 3\nb: !!int \"4\"\nc: !foo bar\nd: 'q'\ne: >-\n  folded\n  text\nf: !!bool true\n\
         g: !!null ~\n? [k]\n: v\n",
    );
    assert!(config.is_err(), "a key that is not a scalar");
    let config = load(
        "a: !!str 3\nb: !!int \"4\"\nc: !foo bar\nd: 'q'\ne: >-\n  folded\n  text\nf: !!bool true\n\
         g: !!null ~\n~: empty key\n",
    )
    .unwrap();
    assert_eq!(config.get("a"), Some(text("3")));
    assert_eq!(config.get("b"), Some(text("4")));
    assert_eq!(config.get("c"), Some(text("bar")));
    assert_eq!(config.get("d"), Some(text("q")));
    assert_eq!(config.get("e"), Some(text("folded text")));
    assert_eq!(config.get("f"), Some(Value::Bool(true)));
    assert_eq!(config.get("g"), Some(Value::Null));
    assert_eq!(config.get(""), Some(text("empty key")));
    assert_eq!(text("x").get("a"), None);
}

#[test]
fn sections() {
    let config = load("Global:\n  ManageImages: true\nEFI: text\nKernel:\n").unwrap();
    assert_eq!(
        config.lookup("Global", "ManageImages").unwrap(),
        Some(Value::Bool(true))
    );
    assert_eq!(config.lookup("Kernel", "Path").unwrap(), None);
    assert_eq!(config.lookup("None", "Path").unwrap(), None);
    let error = config.lookup("EFI", "Enabled").unwrap_err();
    assert_eq!(
        error.0,
        "The section EFI of the configuration is not a mapping of keys"
    );
}

#[test]
fn aliases_share_collections() {
    let config =
        load("Other: &g\n  BootMountPoint: /boot\nGlobal: *g\nList: &l [1]\nCopy: *l\n").unwrap();
    assert_eq!(
        config.lookup("Global", "BootMountPoint").unwrap(),
        Some(text("/boot"))
    );
    let (Some(Value::Seq(list)), Some(Value::Seq(copy))) = (config.get("List"), config.get("Copy"))
    else {
        panic!("sequences");
    };
    assert!(Rc::ptr_eq(&list, &copy));
    assert!(load("a: &x [1, *x]\n").is_err());
}
