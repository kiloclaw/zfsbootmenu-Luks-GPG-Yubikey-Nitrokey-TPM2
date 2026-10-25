use pretty_assertions::assert_eq;

use super::*;
use crate::record::Test;
use crate::settings::{PW, ZFSKEY};
use crate::testing::{Step, checks, cleanup, scripted, settings};

const STATUS: &str = "ZBMTEST-STATUS-BEGIN\r\n\
    ZS|   pool: tank\r\nZS|  state: DEGRADED\r\nZS|   pool: mixed\r\nZS|  state: ONLINE\r\n\
    ZL| luks-1 open tank\r\nZBMTEST-STATUS-END\r\n\
    ZBMTEST-TEARDOWN dm=[] pools=[ mixed ] state=[]\r\n";

const BE: &str = "BE-INIT-START\r\nBE-KERNEL: 7.3.1-zbm\r\n\
    BE-ZFS-VERSION: zfs-2.4.4-1 zfs-kmod-2.4.4-1\r\nBE-ROOT: tank/ROOT/be / zfs rw\r\n\
    BE|   pool: tank\r\nBE|  state: ONLINE\r\nBE-WRITE: ok\r\n\
    BE-SCRUB: tank pool 'tank' is healthy\r\nBE-F| tank feature@draid active\r\n\
    BE-CMDLINE: root=zfs:tank/ROOT/be spl_hostid=0xdeadbeef\r\nBE-INIT-DONE\r\n";

#[test]
fn a_boot_is_followed_from_the_prompts_to_the_boot_environment() {
    let settings = settings("boot-follow");
    let test = Test::new(&settings, "complex-healthy").unwrap();
    let script = vec![
        Step::Say("Unable to import tank, retrying in 4 seconds\r\n"),
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear("other"),
        Step::Say("Passphrase (leave empty to skip): "),
        Step::Hear(PW),
        Step::Say("Enter passphrase for 'tank/ROOT/be': "),
        Step::Hear(ZFSKEY),
        Step::Say(STATUS),
        Step::Say("Encrypted ZFS password for tank/ROOT/be: "),
        Step::Hear(ZFSKEY),
        Step::Say(BE),
    ];
    let (mut vm, guest) = scripted(&test, script);
    let mut retried = false;

    let follow = Follow::default()
        .answer(Kind::Luks, &["other"])
        .on(Kind::Retry, |_| {
            retried = true;
            Ok(())
        });
    let boot = follow_boot(&mut vm, follow).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert!(retried);
    assert_eq!(
        boot.prompts_text(),
        "['luks', 'luks', 'zfskey', 'be-zfskey']"
    );
    assert_eq!(
        boot.pools().text(),
        "{'tank': 'DEGRADED', 'mixed': 'ONLINE'}"
    );
    assert_eq!(boot.zluks, "luks-1 open tank");
    assert_eq!(
        boot.teardown,
        Some((String::new(), "mixed".into(), String::new()))
    );
    assert_eq!(boot.be_line("BE-KERNEL"), "7.3.1-zbm");
    assert_eq!(
        boot.be_features()
            .get(&("tank".into(), "draid".into()))
            .map(String::as_str),
        Some("active")
    );
    cleanup(&settings);
}

/// The start-up view: the stages, and the dialog `lines` over them.
fn view(lines: &str) -> String {
    format!("\x1b[2J\x1b[H STARTING ZFSBOOTMENU\r\n{lines}")
}

#[test]
fn the_dialogs_of_the_start_up_view_are_answered_once_each() {
    let settings = settings("boot-dialogs");
    let test = Test::new(&settings, "keys-lockout").unwrap();
    let pin = view("│ TPM2 PIN:            │\r\n");
    let wrong = view("│ Wrong PIN.           │\r\n│ TPM2 PIN:            │\r\n");
    let luks = view(
        "│ The TPM is locked    │\r\n│ against PIN guessing.│\r\n\
         │ Passphrase:          │\r\n",
    );
    let import = view("│ Its devices may not be there yet │\r\n");
    let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
    let script = vec![
        Step::Say(leak(pin)),
        Step::Hear("0000"),
        // Typed into, the dialog no longer asks; it closes.
        Step::Say("\x1b[2;12H••••"),
        Step::Say(leak(view("Unlocking /dev/vda2 ...\r\n"))),
        Step::Say(leak(wrong)),
        Step::Hear("9999"),
        Step::Say(leak(luks)),
        Step::Hear(PW),
        Step::Say(leak(import)),
        Step::Keys("!"),
        Step::Say(leak(view(""))),
        Step::Say(STATUS),
    ];
    let (mut vm, guest) = scripted(&test, script);
    let follow = Follow {
        no_be: true,
        ..Follow::default()
    }
    .answer(Kind::Tpm2Pin, &["0000", "9999"])
    .on(Kind::Retry, |vm| vm.send("!"));
    let boot = follow_boot(&mut vm, follow).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(boot.prompts, ["tpm2pin", "tpm2pin", "luks"]);
    assert!(boot.has_shown("locked against PIN guessing"));
    assert!(boot.has_shown("Wrong PIN."));
    assert!(!boot.has_shown("Its devices"));
    cleanup(&settings);
}

#[test]
fn a_dialog_that_closes_in_pieces_is_not_answered_again() {
    let settings = settings("boot-dialog-pieces");
    let test = Test::new(&settings, "layouts-parity").unwrap();
    let luks = view("│ Unlock luks-1        │\r\n│ Passphrase:          │\r\n");
    let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
    let script = vec![
        Step::Say(leak(luks)),
        Step::Hear(PW),
        // What was typed does not show. The dialog closes over the serial
        // console a line at a time: its field shows a moment longer.
        Step::Say("\x1b[2;1H\x1b[2K"),
        Step::Pause(100),
        Step::Say("\x1b[3;1H\x1b[2K"),
        Step::Pause(100),
        Step::Say(STATUS),
    ];
    let (mut vm, guest) = scripted(&test, script);
    let follow = Follow {
        no_be: true,
        ..Follow::default()
    };
    let boot = follow_boot(&mut vm, follow).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(boot.prompts, ["luks"]);
    cleanup(&settings);
}

#[test]
fn the_common_checks_read_the_reports() {
    let settings = settings("boot-checks");
    let test = Test::new(&settings, "layouts-main").unwrap();
    let (mut vm, guest) = scripted(&test, vec![Step::Say(STATUS), Step::Say(BE)]);
    let boot = follow_boot(&mut vm, Follow::default()).unwrap();
    guest.finish();

    check_teardown(&test, &boot, &[]);
    check_teardown(&test, &boot, &["mixed"]);
    check_be(
        &test,
        &boot,
        "tank",
        &["tank", "mixed"],
        &[("mixed", "DEGRADED")],
    );

    assert_eq!(
        checks(&test),
        [
            "FAIL - teardown exported pools and closed volumes (dm=[] pools=[mixed] state=[])",
            "ok   - teardown exported pools and closed volumes (dm=[] pools=[mixed] state=[])",
            "ok   - kexec into the ZFS 2.4.4 boot environment",
            "ok   - boot environment kernel 7.3.1-zbm",
            "ok   - boot environment runs ZFS 2.4.4 (zfs-2.4.4-1 zfs-kmod-2.4.4-1)",
            "ok   - root file system tank/ROOT/be / zfs rw",
            "ok   - boot environment imported tank: ONLINE (expected ONLINE)",
            "FAIL - boot environment imported mixed: None (expected DEGRADED)",
            "ok   - root dataset is writable in the boot environment",
            "ok   - scrub of tank in the boot environment: pool 'tank' is healthy",
        ]
    );
    cleanup(&settings);
}

#[test]
fn a_boot_that_ends_in_the_emergency_shell_stops_there() {
    let settings = settings("boot-emergency");
    let test = Test::new(&settings, "complex-skip").unwrap();
    let script = vec![Step::Say("Dropping to an emergency shell\r\n")];
    let (mut vm, guest) = scripted(&test, script);

    let boot = follow_boot(&mut vm, Follow::default()).unwrap();

    guest.finish();
    assert_eq!(boot.prompts, ["emergency"]);
    check_teardown(&test, &boot, &[]);
    check_be(&test, &boot, "tank", &[], &[]);
    assert_eq!(
        checks(&test),
        [
            "FAIL - teardown report missing",
            "FAIL - kexec into the ZFS 2.4.4 boot environment"
        ]
    );
    cleanup(&settings);
}

#[test]
fn a_boot_environment_that_asks_for_a_luks_passphrase_is_recorded() {
    let settings = settings("boot-be-ask");
    let test = Test::new(&settings, "firmware-boot").unwrap();
    let script = vec![
        Step::Say(STATUS),
        Step::Say("Please enter passphrase for disk vda2: "),
    ];
    let (mut vm, guest) = scripted(&test, script);

    let boot = follow_boot(&mut vm, Follow::default()).unwrap();

    guest.finish();
    assert_eq!(boot.prompts, ["be-luks-prompt"]);
    cleanup(&settings);
}

#[test]
fn without_the_boot_environment_the_boot_ends_after_the_reports() {
    let settings = settings("boot-no-be");
    let test = Test::new(&settings, "procedures").unwrap();
    let script = vec![
        Step::Say("TPM2 PIN (leave empty to use another method): "),
        Step::Hear(""),
        Step::Say(STATUS),
    ];
    let (mut vm, guest) = scripted(&test, script);

    let follow = Follow {
        no_be: true,
        ..Follow::default()
    };
    let boot = follow_boot(&mut vm, follow).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(boot.prompts, ["tpm2pin"]);
    assert!(boot.be.is_empty() && boot.teardown.is_some());
    cleanup(&settings);
}

#[test]
fn pools_keep_their_order_and_last_state() {
    let pools = pools("  pool: b\n state: ONLINE\npool: a\nstate: FAULTED\n state: DEGRADED\n");

    assert_eq!(pools.text(), "{'b': 'ONLINE', 'a': 'DEGRADED'}");
    assert_eq!(pools.sorted_names(), "['a', 'b']");
    assert_eq!(pools.show("c"), "None");
    assert!(pools.contains("a"));
}

#[test]
fn a_pool_block_ends_at_the_next_pool() {
    let status =
        "  pool: t\n state: ONLINE\n  t ONLINE\n  pool: tsingle\n tsingle ONLINE\n  pool: x\n";

    assert_eq!(pool_block(status, "t"), " state: ONLINE\n  t ONLINE");
    assert_eq!(pool_block(status, "tsingle"), " tsingle ONLINE");
    assert_eq!(pool_block(status, "y"), "");
}

#[test]
fn the_new_menu_is_recognized_on_the_screen_where_its_text_lacks_letters() {
    let settings = crate::testing::settings("boot-menu-screen");
    let test = Test::new(&settings, "runtime-rust-keysource").unwrap();
    // The start-up view, then the main view drawn over it: the letters that
    // did not change are not sent again.
    let script = vec![
        Step::Say("\x1b[2J\x1b[1;1H STARTING ZFSBOOTMENU \r\n"),
        Step::Say("\x1b[1;2HBOOT E\x1b[1;9HVIRONMEN\x1b[1;18HS    \r\n"),
        Step::Hear(""),
    ];
    let (mut vm, guest) = scripted(&test, script);

    let index = vm
        .expect(&["ENVIRONMENTS", prompt::NEW_MENU, prompt::MENU], 10)
        .unwrap();
    assert_eq!(index, 1);
    vm.sendline("").unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    crate::testing::cleanup(&settings);
}

#[test]
fn the_shell_is_asked_of_the_new_menu() {
    let settings = crate::testing::settings("boot-menu-shell");
    let test = Test::new(&settings, "runtime-rust-boot").unwrap();
    let script = vec![
        Step::Say("\x1b[2J\x1b[1;1H BOOT\x1b[1;7HENVIRONMENTS \r\n"),
        Step::Keys("!\r"),
    ];
    let (mut vm, guest) = scripted(&test, script);

    shell_from_menu(&mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    crate::testing::cleanup(&settings);
}

#[test]
fn after_a_key_the_menu_could_not_load_its_error_is_closed_for_the_shell() {
    let settings = crate::testing::settings("boot-menu-keyload");
    let test = Test::new(&settings, "complex-badkey-rust").unwrap();
    let script = vec![
        Step::Say(
            "\x1b[2J\x1b[1;1H BOOT ENVIRONMENTS \x1b[5;5H✗ unable to load key for tank/ROOT \r\n",
        ),
        Step::Hear(""),
        Step::Keys("\r"),
        Step::Keys("!\r"),
        Step::Say("zfsbootmenu ~ > "),
        Step::Hear("stty -echo cols 250; export PS1='zbm# ' PS2=''"),
        Step::Hear("true; echo RC=$?"),
        Step::Say("RC=0\r\n"),
    ];
    let (mut vm, guest) = scripted(&test, script);

    let index = vm
        .expect(&[prompt::KEYLOAD, prompt::MENU_KEYLOAD], 10)
        .unwrap();
    assert!(index <= 1);
    vm.sendline("").unwrap();
    shell_after_keyload(&mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    crate::testing::cleanup(&settings);
}

#[test]
fn the_recovery_shell_opens_from_the_import_retry_on_the_console_or_in_the_view() {
    let settings = crate::testing::settings("boot-recovery-shell");
    let shell = [
        Step::Keys("\x1b"),
        Step::Say("zfsbootmenu ~ > "),
        Step::Hear("stty -echo cols 250; export PS1='zbm# ' PS2=''"),
        Step::Hear("true; echo RC=$?"),
        Step::Say("RC=0\r\n"),
    ];
    let question =
        view("│ Its devices may not be there yet │\r\n│ Trying again in 3 seconds. │\r\n");
    let retries: [&'static str; 2] = [
        "Unable to import tank, retrying in 5 seconds\r\n",
        Box::leak(question.into_boxed_str()),
    ];
    for (n, retry) in retries.into_iter().enumerate() {
        let test = Test::new(&settings, &format!("complex-setup-{n}")).unwrap();
        let script = [&[Step::Say(retry)][..], &shell].concat();
        let (mut vm, guest) = scripted(&test, script);
        vm.recovery_shell().unwrap();
        assert_eq!(guest.finish(), Vec::<String>::new());
    }
    crate::testing::cleanup(&settings);
}
