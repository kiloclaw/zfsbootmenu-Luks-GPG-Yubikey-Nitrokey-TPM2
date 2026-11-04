//! The generated library against the legacy files, and the wrappers'
//! shell code on Bash.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests run the generated library on Bash and fail loudly"
)]

use std::collections::BTreeMap;

use pretty_assertions::assert_eq;
use serde::{Deserialize, Serialize};
use zbm_platform::exec::{Command, Input, Runner, Stream, SystemRunner};
use zbm_testkit::legacy::{Bash, path};
use zbm_testkit::{TempDir, parity};

use super::*;

/// What the legacy files are, as recorded: each file's text, the functions
/// Bash defines when it sources the three, and the copied functions as
/// Bash prints them.
#[derive(Debug, Deserialize, Serialize)]
struct Legacy {
    texts: BTreeMap<String, String>,
    defined: Vec<String>,
    copied: String,
}

/// Bash code that sources the library below `dir`, the three files and
/// nothing else of the image, as a hook sources it.
fn sourcing(dir: &str) -> String {
    format!(
        r#"source() {{ builtin source "{dir}/${{1##*/}}"; }}
source {dir}/kmsg-log-lib.sh || exit 99
source {dir}/zfsbootmenu-core.sh || exit 99
unset -f source"#
    )
}

/// Prints the functions Bash knows, then the copied ones as Bash prints
/// them.
fn report() -> String {
    format!(
        "declare -F | cut -d' ' -f3\necho ---\ndeclare -f {}",
        COPIED.join(" ")
    )
}

/// Runs `code` with Bash.
fn bash(code: &str, env: &[(&str, &str)]) -> zbm_platform::exec::Output {
    let mut command = Command::new("bash")
        .arg("-c")
        .arg(code)
        .stdin(Input::Null)
        .stderr(Stream::Capture)
        .env("LC_ALL", "C");
    for (name, value) in env {
        command = command.env(name, value);
    }
    SystemRunner.run(&command).expect("bash starts")
}

/// The legacy files, recorded.
fn legacy() -> Legacy {
    let mut parity = parity!("the_library_has_every_legacy_function");
    parity.legacy("the three files", || {
        let lib = path("zfsbootmenu/lib");
        let texts = FILES
            .iter()
            .map(|file| {
                let text = std::fs::read_to_string(lib.join(file)).unwrap();
                ((*file).to_owned(), text)
            })
            .collect();
        let output = Bash::new()
            .code(&sourcing(&lib.to_string_lossy()))
            .code(&report())
            .call("true", &[], b"");
        let text = output.stdout_text();
        let (defined, copied) = text.split_once("---\n").unwrap();
        Legacy {
            texts,
            defined: defined.lines().map(str::to_owned).collect(),
            copied: copied.to_owned(),
        }
    })
}

/// The generated files of `legacy` in a directory, with the wrappers
/// running `zbm` at `zbm`.
fn generated(legacy: &Legacy, zbm: &str) -> TempDir {
    let dir = TempDir::new("zbm-library");
    let all: Vec<&str> = legacy.texts.values().map(String::as_str).collect();
    for (file, text) in &legacy.texts {
        dir.write(file, generate(text, &all).replace(ZBM, zbm));
    }
    dir
}

/// ADR 0017, point 11: the library defines every function of the legacy
/// files, the copied ones as they are, the others as wrappers.
#[test]
fn the_library_has_every_legacy_function() {
    let legacy = legacy();
    let all: Vec<&str> = legacy.texts.values().map(String::as_str).collect();
    let mut names: Vec<&str> = all.iter().flat_map(|text| functions(text)).collect();
    names.sort_unstable();
    let mut defined: Vec<&str> = legacy.defined.iter().map(String::as_str).collect();
    defined.sort_unstable();
    assert_eq!(names, defined, "the functions found are those Bash defines");
    let mut listed = FUNCTIONS.to_vec();
    listed.sort_unstable();
    assert_eq!(names, listed, "FUNCTIONS lists the legacy functions");
    for name in COPIED {
        assert!(names.contains(&name), "{name} is a legacy function");
    }

    let dir = generated(&legacy, "/bin/false");
    let output = bash(
        &format!("{}\n{}", sourcing(&dir.path().to_string_lossy()), report()),
        &[],
    );
    assert!(output.status.is_success(), "{}", output.stderr_text());
    let text = output.stdout_text();
    let (now, copied) = text.split_once("---\n").unwrap();
    let mut now: Vec<&str> = now.lines().filter(|name| *name != "_zbm_zfunc").collect();
    now.sort_unstable();
    assert_eq!(now, defined);
    assert_eq!(
        copied, legacy.copied,
        "the copied functions are the legacy ones"
    );
    for (file, text) in &legacy.texts {
        let generated = generate(text, &all);
        for name in functions(text) {
            let wrapped = generated.contains(&format!("_zbm_zfunc {name} \"$@\""));
            assert_eq!(wrapped, !COPIED.contains(&name), "{file}: {name}");
        }
    }
}

/// A `zbm` that prints its arguments and the variables the wrappers pass,
/// and exits with `$STATUS`.
fn stub(dir: &TempDir) -> String {
    let stub = dir.write_executable(
        "zbm",
        "#!/bin/bash\n\
         [ \"$2\" = get_spl_hostid ] && { printf 0x00bab10c; exit 0; }\n\
         echo \"zbm $* read_write=${read_write-unset}\"\n\
         exit \"${STATUS:-0}\"\n",
    );
    stub.to_string_lossy().into_owned()
}

/// DESIGN.md, "Compatibility library": the wrappers pass the arguments and
/// the variables the functions read, return the status, and leave in the
/// caller's shell what the legacy functions left there.
#[test]
fn the_wrappers_keep_the_calling_conventions() {
    let legacy = legacy();
    let bin = TempDir::new("zbm-stub");
    let dir = generated(&legacy, &stub(&bin));
    let run = |code: &str, status: &str| {
        let output = bash(
            &format!("{}\n{code}", sourcing(&dir.path().to_string_lossy())),
            &[("STATUS", status)],
        );
        (output.status.code(), output.stdout_text())
    };

    // Arguments as they are, a variable set without `export`, the status.
    let (code, out) = run(
        "read_write=yes\nis_degraded 'a b' c; echo \"status $?\"",
        "3",
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        out,
        "zbm zfunc is_degraded a b c read_write=yes\nstatus 3\n"
    );

    let (_, out) = run(
        "BOOTFS=old\nset_default_env tank/ROOT/new; echo \"$BOOTFS\"",
        "0",
    );
    assert!(out.ends_with("tank/ROOT/new\n"), "{out}");
    let (_, out) = run(
        "BOOTFS=old\nset_default_env tank/ROOT/new; echo \"$? $BOOTFS\"",
        "1",
    );
    assert!(out.ends_with("1 old\n"), "{out}");

    let (_, out) = run(
        "import_pool tank; echo \"$? ret=$ret spl_hostid=${spl_hostid-unset}\"",
        "1",
    );
    assert!(out.ends_with("1 ret=1 spl_hostid=0x00bab10c\n"), "{out}");
    let (_, out) = run(
        "import_pool tank; echo \"$? ret=$ret spl_hostid=${spl_hostid-unset}\"",
        "0",
    );
    assert!(out.ends_with("0 ret=0 spl_hostid=unset\n"), "{out}");

    let (_, out) = run("is_mounted /dev/sda1; echo \"$? $mount_dev\"", "1");
    assert!(out.ends_with("1 /dev/sda1\n"), "{out}");
    let (_, out) = run(
        "timed_prompt -m x; echo \"$? ${HEIGHT+height} ${WIDTH+width}\"",
        "2",
    );
    assert!(out.ends_with("2 height width\n"), "{out}");

    // The copied functions run without zbm.
    let (_, out) = run("colorize red x; echo; zinfo 'not to kmsg' 2>/dev/null", "9");
    assert!(!out.contains("zbm "), "{out}");
}

#[test]
fn functions_are_found_at_the_start_of_a_line() {
    let text = "a() {\n  inner() {\n  }\n}\nb_2() {\n}\nnot ( ) {\nc() { :; }\n# d() {\n";
    assert_eq!(functions(text), ["a", "b_2"]);
    let generated = generate(text, &[text]);
    assert!(generated.starts_with("# Generated"), "{generated}");
    assert!(!generated.contains("inner"));
    assert!(generate("no functions\n", &[]).starts_with("no functions"));
}
