use std::path::Path;

use pretty_assertions::assert_eq;

use super::*;

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn the_keyholder_reads_its_options() {
    assert_eq!(parse(&words("serve")), Ok(Options::default()));
    assert_eq!(
        parse(&words("serve --quiet --journal /tmp/j --verbose")),
        Ok(Options {
            quiet: true,
            journal: Some("/tmp/j".into()),
            verbose: true,
        })
    );
    assert_eq!(parse(&words("")), Err("the command is serve".to_owned()));
    assert_eq!(
        parse(&words("serve --journal")),
        Err("--journal FILE needs a file".to_owned())
    );
    assert_eq!(
        parse(&words("serve -x")),
        Err("unknown option -x".to_owned())
    );
}

#[test]
fn keyholders_start_with_the_options_of_their_client() {
    let keyholders = ProcessKeyholders::new()
        .of(Path::new("/libexec/zbm"))
        .quiet()
        .traced(Some(Path::new("/tmp/j")), true);

    assert_eq!(keyholders.program, Path::new("/libexec/zbm"));
    assert_eq!(
        keyholders.args,
        words("unlock serve --quiet --journal /tmp/j --verbose")
    );
    assert_eq!(
        parse(&keyholders.args[1..]).map(|options| options.quiet),
        Ok(true)
    );
    assert_eq!(
        ProcessKeyholders::default().args,
        ProcessKeyholders::new().traced(None, false).args
    );
}
