use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{checks, cleanup, settings};

const STATUS: &str = "SigBlk:\t0000000000000000\nSigIgn:\t0000000000384004\n\
                      SigCgt:\t000000004b813efb\n";

fn complete(blk: &str) -> Masks {
    Masks(vec![
        ("SigBlk".into(), blk.into()),
        ("SigIgn".into(), "0000000000384004".into()),
        ("SigCgt".into(), "000000004b813efb".into()),
    ])
}

#[test]
fn masks_are_read_from_proc_status_lines() {
    let masks = masks(STATUS).unwrap();

    assert_eq!(masks, complete("0000000000000000"));
    assert!(masks.is_complete());
    assert_eq!(
        masks.text(),
        "{'SigBlk': '0000000000000000', 'SigIgn': '0000000000384004', \
         'SigCgt': '000000004b813efb'}"
    );
}

#[test]
fn a_repeated_mask_keeps_its_place_and_takes_the_last_value() {
    let masks = masks("SigIgn: 1 SigBlk: 2 SigIgn: 3 SigQ: 4").unwrap();

    assert_eq!(masks.text(), "{'SigIgn': '3', 'SigBlk': '2'}");
    assert!(!masks.is_complete());
    assert_eq!(Masks::default().text(), "{}");
}

#[test]
fn the_shell_output_splits_into_fzf_and_the_shell() {
    let out = format!("fzf=[SigBlk: 1 SigIgn: 2 SigCgt: 3 ]\n{STATUS}");

    let (fzf, shell) = split_output(&out).unwrap();

    assert_eq!(fzf.text(), "{'SigBlk': '1', 'SigIgn': '2', 'SigCgt': '3'}");
    assert_eq!(shell, complete("0000000000000000"));
}

#[test]
fn without_fzf_the_whole_output_is_the_shell() {
    let (fzf, shell) = split_output(STATUS).unwrap();
    let (_, after) = split_output(&format!("SigBlk: 9\nfzf=[\n{STATUS}")).unwrap();

    assert_eq!(fzf, Masks::default());
    assert_eq!(shell, complete("0000000000000000"));
    assert_eq!(after, complete("0000000000000000"));
}

#[test]
fn signals_json_is_written_as_python_writes_it_and_read_back() {
    let result = Dispositions {
        hook: Masks(vec![
            ("SigBlk".into(), "0".into()),
            ("SigIgn".into(), "1".into()),
        ]),
        fzf: Masks::default(),
        shell: Masks(vec![("SigCgt".into(), "2".into())]),
    };

    let json = result.to_json();

    assert_eq!(
        json,
        "{\n  \"hook\": {\n    \"SigBlk\": \"0\",\n    \"SigIgn\": \"1\"\n  },\n  \
         \"fzf\": {},\n  \"shell\": {\n    \"SigCgt\": \"2\"\n  }\n}\n"
    );
    assert_eq!(Dispositions::from_json(&json).unwrap(), result);
    assert!(Dispositions::from_json("{").is_err());
}

#[test]
fn the_legacy_run_checks_fzf_too() {
    let settings = settings("runtime-signals-legacy");
    let test = Test::new(&settings, "runtime-legacy-signals").unwrap();
    let result = Dispositions {
        hook: complete("0"),
        fzf: Masks::default(),
        shell: complete("0"),
    };

    verify(&test, ZbmRuntime::Legacy, &result);

    assert_eq!(
        checks(&test),
        [
            "ok   - the hook reported its dispositions",
            "ok   - the recovery shell reported its dispositions",
            "ok   - the recovery shell does not ignore Ctrl-C (SigIgn 0000000000384004)",
            "FAIL - fzf's dispositions were recorded",
        ]
    );
    cleanup(&settings);
}

#[test]
fn a_recovery_shell_that_ignores_ctrl_c_fails() {
    let settings = settings("runtime-signals-rust");
    let test = Test::new(&settings, "runtime-rust-signals").unwrap();
    let mut shell = complete("0");
    shell.set("SigIgn", "0000000000384006");
    let mut partial = complete("0");
    partial.0.pop();
    let result = Dispositions {
        hook: partial,
        fzf: Masks::default(),
        shell,
    };

    verify(&test, ZbmRuntime::Rust, &result);

    assert_eq!(
        checks(&test),
        [
            "FAIL - the hook reported its dispositions",
            "ok   - the recovery shell reported its dispositions",
            "FAIL - the recovery shell does not ignore Ctrl-C (SigIgn 0000000000384006)",
        ]
    );
    cleanup(&settings);
}

#[test]
fn the_hook_is_the_script_of_the_python_harness() {
    assert!(HOOK.starts_with("#!/bin/bash\nsig() { grep -E '^Sig(Blk|Ign|Cgt):'"));
    assert!(HOOK.contains(r"| tr -s ' \t' ' ' | tr '\n' ' '; }"));
    assert!(HOOK.ends_with(") </dev/null >/dev/null 2>&1 &\n"));
    assert_eq!(
        SHELL_COMMAND,
        "read -r l < /zfsbootmenu/zbmtest-fzf-sig 2>/dev/null; echo \"${l}\"; \
         grep -E '^Sig(Blk|Ign|Cgt)' /proc/$$/status"
    );
}
