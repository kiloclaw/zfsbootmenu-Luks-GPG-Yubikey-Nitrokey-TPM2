//! Configuration keys that only the Rust program reads, so the Perl program
//! has nothing to compare: `Global.ConsoleFont`.

use crate::fixtures::COMPONENTS;
use crate::sandbox::{Case, run_rust};

const ARGS: &[&str] = &["-c", "{root}/config.yaml", "-b", "{root}/boot"];

/// The components configuration with `Global.ConsoleFont: font`.
fn with_font(font: &str) -> String {
    COMPONENTS.replace(
        "  ManageImages: true\n",
        &format!("  ManageImages: true\n  ConsoleFont: {font}\n"),
    )
}

#[test]
fn the_console_font_is_passed_to_dracut_in_lower_case() {
    let run = run_rust(&Case::new(&with_font("Spleen"), ARGS));

    assert_eq!(run.code, 0, "{}", run.stdout);
    let font = "zfsbootmenu_console_font=spleen";
    assert!(run.log.contains(&format!("{font}\n")), "{}", run.log);
}

#[test]
fn the_console_font_is_passed_to_mkinitcpio() {
    let config = with_font("terminus").replace(
        "  ManageImages: true\n",
        "  ManageImages: true\n  InitCPIO: true\n  InitCPIOConfig: {root}/mkinitcpio.conf\n",
    );

    let run = run_rust(&Case::new(&config, ARGS));

    assert_eq!(run.code, 0, "{}", run.stdout);
    assert!(run.log.contains("mkinitcpio ["), "{}", run.log);
    assert!(
        run.log.contains("zfsbootmenu_console_font=terminus\n"),
        "{}",
        run.log
    );
}

#[test]
fn without_the_key_the_build_option_is_not_set() {
    let run = run_rust(&Case::new(COMPONENTS, ARGS));

    assert_eq!(run.code, 0, "{}", run.stdout);
    assert!(run.log.contains("dracut ["), "{}", run.log);
    assert!(!run.log.contains("zfsbootmenu_console_font"), "{}", run.log);
}

#[test]
fn another_font_stops_before_anything_is_built() {
    let run = run_rust(&Case::new(&with_font("comic"), ARGS));

    assert_eq!(run.code, 1);
    assert!(
        run.stdout
            .contains("Global.ConsoleFont must be terminus or spleen, not 'comic'"),
        "{}",
        run.stdout
    );
    assert!(!run.log.contains("dracut"), "{}", run.log);
}
