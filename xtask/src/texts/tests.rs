use super::scan::is_text;
use super::*;

/// The scan of `source`, outside test code.
fn scanned(source: &str) -> TextScan {
    scan(&syn::parse_file(source).unwrap(), false)
}

fn values(found: &[Found]) -> Vec<&str> {
    found.iter().map(|found| found.value.as_str()).collect()
}

#[test]
fn text_has_a_letter_outside_placeholders() {
    assert!(is_text("Boot"));
    assert!(is_text("{name} is open"));
    assert!(!is_text("{label:>LABEL$}  "));
    assert!(!is_text("[ {label} ]"));
    assert!(!is_text("{other:?}"));
    assert!(!is_text("→ ·"));
}

#[test]
fn literals_and_ids_are_told_apart() {
    let scan = scanned(
        r#"
        /// A doc comment is not text.
        #[allow(dead_code, reason = "nor is an attribute")]
        fn view() {
            let a = text!("list-count-of", shown = 2, total = format!("{}", "x"));
            let b = vec![id!("key-esc"), id!("key-enter")];
            let c = format!("{} of {total}", "Boot");
            let d = "Cancel";
            let e = format!("{label:>8}");
        }
        #[cfg(test)]
        mod tests {
            fn t() { let _ = ("Not counted", text!("in-tests")); }
        }
        "#,
    );
    assert_eq!(values(&scan.ids), ["list-count-of", "key-esc", "key-enter"]);
    // `of` is outside the placeholders: the format string is text.
    assert_eq!(
        values(&scan.literals),
        ["x", "{} of {total}", "Boot", "Cancel"]
    );
    assert_eq!(values(&scan.test_ids), ["in-tests"]);
    assert_eq!(scan.literals[3].line, 8);
}

#[test]
fn test_files_name_ids_only() {
    let file = syn::parse_file(r#"fn t() { let _ = ("Text", id!("key-esc")); }"#).unwrap();
    let scan = scan(&file, true);
    assert_eq!(scan.literals, []);
    assert_eq!(values(&scan.test_ids), ["key-esc"]);
}

const CATALOGUE: &str = "# A comment.\n\
## A group.\n\
action-boot = boot\n    .title = Boot\n\
size = { $value } { $unit ->\n        [0] B\n       *[1] KiB\n    }\n\
import-failed =\n    .title = { $pool } Did Not Import\n\
unused = never named\n";

#[test]
fn the_catalogue_lists_messages_and_attributes() {
    let ids: Vec<String> = entries(CATALOGUE)
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(
        ids,
        [
            "action-boot",
            "action-boot.title",
            "size",
            "import-failed",
            "import-failed.title",
            "unused"
        ]
    );
    assert_eq!(entries(CATALOGUE)[1].line, 4);
}

#[test]
fn problems_name_literals_unknown_ids_and_unused_messages() {
    let code = TextScan {
        literals: vec![Found {
            line: 3,
            value: "Boot".to_owned(),
        }],
        ids: vec![
            Found {
                line: 4,
                value: "action-boot".to_owned(),
            },
            Found {
                line: 5,
                value: "size".to_owned(),
            },
            Found {
                line: 6,
                value: "import-failed.title".to_owned(),
            },
            Found {
                line: 7,
                value: "missing".to_owned(),
            },
        ],
        test_ids: vec![Found {
            line: 8,
            value: "also-missing".to_owned(),
        }],
    };
    let files = vec![
        ("menu/view.rs".to_owned(), true, code.clone()),
        ("demo.rs".to_owned(), false, code),
    ];
    let problems = problems("menu.ftl", &entries(CATALOGUE), &files);
    assert_eq!(
        problems,
        [
            "menu/view.rs:3: text \"Boot\" is not from the catalogue",
            "menu/view.rs:7: no message \"missing\" in menu.ftl",
            "menu/view.rs:8: no message \"also-missing\" in menu.ftl",
            "demo.rs:7: no message \"missing\" in menu.ftl",
            "demo.rs:8: no message \"also-missing\" in menu.ftl",
            "menu.ftl:11: unused is not used",
        ]
    );
    let twice = format!("{CATALOGUE}unused = again\n");
    assert!(problems_of(&twice).contains(&"menu.ftl:12: unused again".to_owned()));
}

fn problems_of(catalogue: &str) -> Vec<String> {
    problems("menu.ftl", &entries(catalogue), &[])
}

#[test]
fn only_the_menu_s_text_files_hold_text() {
    let root = Path::new("/r");
    assert!(is_text_file(root, &root.join(MENU).join("view/screens.rs")));
    assert!(!is_text_file(root, &root.join(MENU).join("demo/data.rs")));
    assert!(!is_text_file(root, &root.join(MENU).join("caps.rs")));
    assert!(!is_text_file(
        root,
        &root.join("crates/zbm-tui/src/view.rs")
    ));
}
