use std::path::PathBuf;

use pretty_assertions::assert_eq;

use super::*;

/// The sections the procedures run, by page.
const USED: &[(&str, &[&str])] = &[
    (
        "vdev-configurations.rst",
        &[
            "Conventions",
            "Step 2: Define a helper that creates and opens LUKS volumes",
            "Step 3: Create the LUKS volumes",
            "Example 1: Single disk",
            "Example 2: Two-way mirror",
            "Example 3: Three-way mirror",
            "Example 4: Striped mirrors",
            "Example 5: RAIDZ",
            "Example 6: dRAID",
            "Example 7: Special allocation class",
            "Example 8: Log (SLOG)",
            "Example 9: Cache (L2ARC)",
            "Example 10: Hot spare",
            "Example 11: A complete layout",
            "Example 12: A mirror of two NVMe SSDs, unlocked without a crypttab",
            "Step 1: Create a boot environment",
            "Step 2: Let the installed system unlock the volumes",
        ],
    ),
    ("zfs-versions.rst", &["Upgrading a Pool"]),
    (
        "administration.rst",
        &[
            "Administration",
            "Removing an auxiliary vdev",
            "Replacing a failed disk",
            "Expanding a RAIDZ vdev",
            "Turning a single disk into a mirror",
            "Adding a vdev",
        ],
    ),
];

fn titles(text: &str) -> Vec<String> {
    sections(text).into_iter().map(|(title, _)| title).collect()
}

fn lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|&line| line.to_owned()).collect()
}

#[test]
fn a_section_runs_from_its_underlined_title_to_the_next_title() {
    let text = "Preamble\n\nFirst\n=====\none\n\ntwo\nSecond\n------\nthree";

    let sections = sections(text);

    assert_eq!(
        sections,
        [
            ("First".to_owned(), "one\n\ntwo".to_owned()),
            ("Second".to_owned(), "three".to_owned())
        ]
    );
}

#[test]
fn every_punctuation_character_underlines() {
    for c in UNDERLINE_CHARS.chars() {
        let text = format!("Title\n{}\nbody", c.to_string().repeat(5));

        let sections = sections(&text);

        assert_eq!(sections, [("Title".to_owned(), "body".to_owned())], "{c}");
    }
}

#[test]
fn an_underline_is_one_character_four_times_and_as_long_as_the_title() {
    let text = "Ab\n===\nCd\n=-==\nEfghij\n=====\nKl\n.....\nMn\n====  \nOp\n  ====\n";

    let titles = titles(text);

    assert_eq!(titles, ["Mn"]);
}

#[test]
fn a_title_is_stripped_and_a_blank_line_is_never_one() {
    let text = "  Title  \n=========\n   \n====\nbody";

    let sections = sections(text);

    assert_eq!(
        sections,
        [("Title".to_owned(), "   \n====\nbody".to_owned())]
    );
}

#[test]
fn a_later_section_of_the_same_title_replaces_the_earlier_one_in_its_place() {
    let text = "A\n====\none\nB\n====\ntwo\nA\n====\nthree";

    let sections = sections(text);

    assert_eq!(
        sections,
        [
            ("A".to_owned(), "three".to_owned()),
            ("B".to_owned(), "two".to_owned())
        ]
    );
}

#[test]
fn lines_are_split_as_python_splits_them() {
    let text = "a\r\nb\rc\x0bd\x0ce\u{2028}f\n\ng\n";

    let lines = split_lines(text);

    assert_eq!(lines, ["a", "b", "c", "d", "e", "f", "", "g"]);
    assert_eq!(split_lines(""), Vec::<&str>::new());
}

#[test]
fn a_shell_block_ends_at_a_line_no_deeper_than_its_directive() {
    let text = "  .. code-block:: bash\n\n    # one\n\n      out\n\n  after\n    # not code\n";

    let blocks = blocks(text);

    assert_eq!(blocks, [lines(&["# one", "", "  out"])]);
}

#[test]
fn bash_sh_and_shell_blocks_are_read_and_other_languages_are_not() {
    let text = ".. code-block:: bash\n\n  a\n\
                .. code-block:: sh  \n\n  b\n\
                .. code-block:: shell\n\n  c\n\
                .. code-block:: console\n\n  d\n\
                .. code-block:: shellscript\n\n  e\n\
                ..  code-block:: bash\n\n  f\n\
                .. code-block:: bash\n\n";

    let blocks = blocks(text);

    assert_eq!(blocks, [lines(&["a"]), lines(&["b"]), lines(&["c"])]);
}

#[test]
fn a_block_is_dedented_to_its_least_indented_line() {
    let text = ".. code-block:: bash\n\n      deep\n    shallow\n  \n     mid\n";

    let blocks = blocks(text);

    assert_eq!(blocks, [lines(&["  deep", "shallow", "", " mid"])]);
}

#[test]
fn prompts_start_commands_and_deeper_lines_continue_them() {
    let block = lines(&[
        "# sgdisk -n1 \\",
        "         -n2   /dev/x",
        "# printf '%s\\n' \"${VDEVS[@]}\"",
        "/dev/mapper/luks-5d0c3a1e-...",
        "  /dev/mapper/luks-6e1d4b2f-...",
        "# zpool status",
    ]);

    let commands = block_commands(&block);

    assert_eq!(
        commands,
        [
            "sgdisk -n1 \\\n-n2   /dev/x",
            "printf '%s\\n' \"${VDEVS[@]}\"",
            "zpool status"
        ]
    );
}

#[test]
fn a_blank_line_or_a_tab_ends_a_command() {
    let block = lines(&["# one", "", "  ignored", "# two", "\tignored", "# three"]);

    let commands = block_commands(&block);

    assert_eq!(commands, ["one", "two", "three"]);
}

#[test]
fn a_block_without_prompts_is_one_command() {
    let block = lines(&["f() {", "  echo", "", "}", "#comment"]);

    let commands = block_commands(&block);

    assert_eq!(commands, ["f() {\n  echo\n\n}\n#comment"]);
}

#[test]
fn the_commands_of_a_section_come_from_all_its_blocks() {
    let text = "Other\n=====\n.. code-block:: bash\n\n  # no\n\
                Wanted\n======\n\ntext\n\n.. code-block:: bash\n\n  # a\n  out\n\n\
                - item\n\n  .. code-block:: sh\n\n    X=( 1\n        2 )\n";

    let commands = section_commands(text, "Wanted");

    assert_eq!(commands, Some(lines(&["a", "X=( 1\n    2 )"])));
    assert_eq!(section_commands(text, "Missing"), None);
}

#[test]
fn a_missing_section_or_file_is_an_error() {
    let path = std::env::temp_dir().join(format!("zbm-vmtest-docs-{}.rst", std::process::id()));
    fs::write(&path, "Title\n=====\n.. code-block:: bash\n\n  # true\n").unwrap();

    let found = commands(&path, "Title").unwrap();
    let missing = commands(&path, "Other").unwrap_err().to_string();
    fs::remove_file(&path).unwrap();
    let unreadable = commands(&path, "Title").unwrap_err();

    assert_eq!(found, ["true"]);
    assert_eq!(missing, format!("no section 'Other' in {}", path.display()));
    assert!(matches!(unreadable, Error::Io { .. }));
}

#[test]
fn every_section_the_procedures_run_has_commands() {
    let docs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/luks");

    for (page, titles) in USED {
        for title in *titles {
            let commands = commands(&docs.join(page), title).unwrap();

            assert!(!commands.is_empty(), "{page}: {title}");
        }
    }
}

#[test]
fn the_upgrade_section_enables_dynamic_gang_header_with_one_command() {
    let page = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/luks/zfs-versions.rst");

    let commands = commands(&page, "Upgrading a Pool").unwrap();

    let set: Vec<&String> = commands
        .iter()
        .filter(|command| command.contains("feature@dynamic_gang_header=enabled"))
        .collect();
    assert_eq!(set.len(), 1);
}
