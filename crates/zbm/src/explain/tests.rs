use pretty_assertions::assert_eq;

use super::*;

/// Runs `zbm explain` with `args`: its status, standard output and error.
fn explain(args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let status = run(
        &args,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap_or_default();
    (status, text(stdout), text(stderr))
}

#[test]
fn every_code_is_unique_and_in_its_crates_range() {
    let all = explanations();
    let mut numbers: Vec<u16> = all.iter().map(|entry| entry.code.number()).collect();
    numbers.dedup();
    assert_eq!(numbers.len(), all.len(), "a code is used twice");
    // The tables, by crate, and the hundreds each may use.
    let ranges: [(&[Explanation], u16); 7] = [
        (zbm_app::codes::ZFS_CODES, 2),
        (zbm_pool::POOL_CODES, 3),
        (zbm_luks::LUKS_CODES, 4),
        (zbm_app::codes::CRYPT_CODES, 5),
        (zbm_boot::BOOT_CODES, 6),
        (zbm_boot::saved_logs::SAVE_CODES, 6),
        (zbm_startup::STARTUP_CODES, 7),
    ];
    for (table, hundreds) in ranges {
        for entry in table {
            assert_eq!(entry.code.number() / 100, hundreds, "{}", entry.code);
        }
    }
    for entry in &all {
        assert!(
            !entry.summary.is_empty() && !entry.text.is_empty(),
            "{}",
            entry.code
        );
        assert!(
            !entry.text.contains("  "),
            "{}: text with a double space",
            entry.code
        );
    }
}

#[test]
fn a_code_is_explained_however_it_is_written() {
    let (status, stdout, stderr) = explain(&["zbm-e0412"]);
    assert_eq!((status, stderr.as_str()), (0, ""));
    assert!(
        stdout.starts_with("ZBM-E0412: no key method opened the volume\n\n"),
        "{stdout}"
    );
    assert!(stdout.lines().all(|line| line.len() <= 78), "{stdout}");
    assert_eq!(explain(&["412"]).1, stdout);
}

#[test]
fn without_a_code_every_code_is_listed() {
    let (status, stdout, _) = explain(&[]);
    assert_eq!(status, 0);
    assert_eq!(stdout.lines().count(), explanations().len());
    assert!(
        stdout.contains("ZBM-E0604  no kernel to boot was found\n"),
        "{stdout}"
    );
}

#[test]
fn an_unknown_code_or_a_wrong_use_fails() {
    assert_eq!(
        explain(&["E0999"]),
        (
            1,
            String::new(),
            "zbm explain: no error has the code E0999\n".to_owned()
        )
    );
    assert_eq!(explain(&["word"]).0, 1);
    assert_eq!(
        explain(&["--all"]),
        (USAGE, String::new(), USAGE_TEXT.to_owned())
    );
    assert_eq!(explain(&["1", "2"]).0, USAGE);
}

#[test]
fn text_is_cut_between_words() {
    assert_eq!(wrapped("one two three four", 9), "one two\nthree\nfour\n");
    assert_eq!(wrapped("", 9), "\n");
}
