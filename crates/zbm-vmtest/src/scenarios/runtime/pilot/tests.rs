use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{checks, cleanup, settings};

#[test]
fn the_unlock_pilot_commands_are_those_of_the_python_harness() {
    assert_eq!(
        format_command(),
        "if command -v truncate >/dev/null; then truncate -s 32M /tmp/pilot.img; \
         elif command -v fallocate >/dev/null; then fallocate -l 32M /tmp/pilot.img; \
         else head -c 33554432 /dev/zero > /tmp/pilot.img; fi && \
         dev=$(losetup -f --show /tmp/pilot.img) && printf %s zbm-pilot-key-1234 | \
         cryptsetup luksFormat --batch-mode --type luks2 --pbkdf pbkdf2 \
         --pbkdf-force-iterations 1000 --key-file=- \"$dev\" && \
         printf %s zbm-pilot-key-1234 > /tmp/pilot.key && printf wrong > /tmp/wrong.key && \
         echo DEV=$dev"
    );
    assert_eq!(
        open_command("wrong"),
        "zluks --pilot open \"$dev\" zbm-pilot --key-file /tmp/wrong.key \
         --journal /tmp/pilot.jsonl"
    );
}

#[test]
fn the_journal_must_record_both_attempts_without_the_key() {
    let settings = settings("runtime-pilot-journal");
    let test = Test::new(&settings, "runtime-rust-pilot").unwrap();

    check_journal(
        &test,
        r#"{"message":"refused","stdin":"secret"}
{"message":"opened","stdin":"secret"}"#,
    );
    check_journal(&test, r#"{"message":"opened","key":"zbm-pilot-key-1234"}"#);

    assert_eq!(
        checks(&test),
        [
            "ok   - the journal records that the key went to standard input",
            "ok   - the journal does not contain the key",
            "ok   - the journal records the refused and the successful attempt",
            "FAIL - the journal records that the key went to standard input",
            "FAIL - the journal does not contain the key",
            "FAIL - the journal records the refused and the successful attempt",
        ]
    );
    cleanup(&settings);
}
