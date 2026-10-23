use std::fs;

use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

#[test]
fn reports_give_the_font_and_the_size() {
    let plain = "AUTOSIZE rc=0 size=120 384 /usr/share/zfsbootmenu/fonts/ter-v20b.psf, \
                 /dev/tty1 is 384x120";
    let doubled = "x\nAUTOSIZE rc=0 size=50 160 /usr/share/zfsbootmenu/fonts/ter-v24b.psf, \
                   doubled, /dev/tty1 is 160x50\n";

    assert_eq!(
        parse_report(plain),
        Some(expected("ter-v20b.psf", false, 384, 120))
    );
    assert_eq!(
        parse_report(doubled),
        Some(expected("ter-v24b.psf", true, 160, 50))
    );
    assert_eq!(parse_report("AUTOSIZE rc=1 size= "), None);
}

#[test]
fn the_density_may_be_off_by_the_rounding_of_the_edid() {
    // 3840 pixels on 975 mm, and on 97 cm.
    assert!(density_matches("display density 100\n", Some(&EDID_DPI)));
    assert!(density_matches("display density 101", Some(&EDID_DPI)));
    assert!(!density_matches("display density 120", Some(&EDID_DPI)));
    assert!(!density_matches("display density unknown", Some(&EDID_DPI)));
    assert!(density_matches("display density unknown", None));
    assert!(!density_matches("display density 100", None));
    assert!(!density_matches("", None));
}

#[test]
fn a_run_is_checked_against_the_font_it_should_keep() {
    let settings = settings("runtime-autosize-run");
    let test = Test::new(&settings, "runtime-rust-autosize").unwrap();
    // The script holds static text: the two commands live as long as the test.
    let run: &'static str = Box::leak(format!("{RUN}; echo RC=$?").into_boxed_str());
    let density: &'static str = Box::leak(format!("{DENSITY}; echo RC=$?").into_boxed_str());
    let script = vec![
        Step::Hear(run),
        Step::Say("RC=0\r\n"),
        Step::Hear(density),
        Step::Say("display density 100\r\nRC=0\r\n"),
        Step::Hear(run),
        Step::Say(
            "AUTOSIZE rc=0 size=120 384 /usr/share/zfsbootmenu/fonts/ter-v20b.psf, \
             /dev/tty1 is 384x120\r\nRC=0\r\n",
        ),
        Step::Hear(run),
        Step::Say(
            "AUTOSIZE rc=0 size=75 240 /usr/share/zfsbootmenu/fonts/ter-v32b.psf, \
             /dev/tty1 is 240x75\r\nRC=0\r\n",
        ),
    ];
    let (mut vm, guest) = scripted(&test, script);
    let edid = expected("ter-v20b.psf", false, 384, 120);

    check_density(
        &test,
        &mut vm,
        Some(&EDID_DPI),
        "the EDID gives about 100 dpi",
    )
    .unwrap();
    check_run(&test, &mut vm, &edid, "with the EDID").unwrap();
    check_run(&test, &mut vm, &edid, "again").unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let checks = checks(&test);
    assert_eq!(
        checks[0],
        "ok   - the EDID gives about 100 dpi (display density 100)"
    );
    assert!(checks[1].starts_with("ok   - with the EDID: ter-v20b.psf at 384x120"));
    assert!(checks[2].starts_with("FAIL - again: ter-v20b.psf at 384x120"));
    assert!(fs::read_to_string(test.dir().join("steps.log")).is_ok());
    cleanup(&settings);
}
