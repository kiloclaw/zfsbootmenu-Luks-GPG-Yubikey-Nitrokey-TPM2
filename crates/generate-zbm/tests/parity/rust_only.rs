//! Configuration keys that only the Rust program reads, so the Perl program
//! has nothing to compare: `Global.ConsoleFont`, and reproducible images
//! with `SOURCE_DATE_EPOCH` (ROADMAP.md #5); and where the Rust program
//! differs from the Perl one on purpose, to fix it (CHANGELOG.md): image
//! directories with spaces, two kernel prefixes in one directory, and a
//! section without its image directory.

use crate::fixtures::COMPONENTS;
use crate::sandbox::{Case, Sandbox, run_rust};

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

#[test]
fn with_source_date_epoch_dracut_builds_a_reproducible_image() {
    let mut case = Case::new(COMPONENTS, ARGS);
    case.env = &[("SOURCE_DATE_EPOCH", "1700000000")];
    let reproducible = run_rust(&case);
    case.env = &[("SOURCE_DATE_EPOCH", "")];
    let empty = run_rust(&case);

    assert_eq!(reproducible.code, 0, "{}", reproducible.stdout);
    assert!(
        reproducible.log.contains(" [--reproducible]"),
        "{}",
        reproducible.log
    );
    assert!(!empty.log.contains("--reproducible"), "{}", empty.log);
}

/// The components of the kernel `linux-lts` under the prefix `prefix`,
/// version 9.9.
const LINUX: &[&str] = &[
    "-c",
    "{root}/config.yaml",
    "-k",
    "{root}/boot/linux-lts",
    "-p",
    "vmlinuz-linux",
    "-v",
    "9.9",
];

/// Writes `files` below the sandbox's `esp/EFI`.
fn write_images(sandbox: &Sandbox, files: &[&str]) {
    for file in files {
        let path = sandbox.path().join("esp/EFI").join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, *file).unwrap();
    }
}

#[test]
fn an_image_directory_with_a_space_gets_a_new_revision() {
    let config = COMPONENTS.replace("esp/EFI/zbm", "esp/EFI/my zbm");
    let run = run_rust(&Case {
        setup: |sandbox| {
            write_images(
                sandbox,
                &[
                    "my zbm/vmlinuz-linux-9.9_1",
                    "my zbm/initramfs-linux-9.9_1.img",
                ],
            );
        },
        ..Case::new(&config, LINUX)
    });

    assert_eq!(run.code, 0, "{}", run.stdout);
    assert!(
        run.stdout
            .contains("Created kernel image <root>/esp/EFI/my zbm/vmlinuz-linux-9.9_2\n"),
        "{}",
        run.stdout
    );
    // The first revision is left as it was.
    assert!(
        run.state
            .contains("esp/EFI/my zbm/vmlinuz-linux-9.9_1: 26 bytes"),
        "{}",
        run.state
    );
}

#[test]
fn the_images_of_another_prefix_are_neither_replaced_nor_pruned() {
    let config = COMPONENTS.replace("Versions: 3", "Versions: 1");
    let run = run_rust(&Case {
        setup: |sandbox| {
            write_images(
                sandbox,
                &[
                    "zbm/vmlinuz-linux-8.0_1",
                    "zbm/initramfs-linux-8.0_1.img",
                    "zbm/vmlinuz-linux-9.0_1",
                    "zbm/initramfs-linux-9.0_1.img",
                    "zbm/vmlinuz-linux-lts-8.0_1",
                    "zbm/initramfs-linux-lts-8.0_1.img",
                ],
            );
        },
        ..Case::new(&config, LINUX)
    });

    assert_eq!(run.code, 0, "{}", run.stdout);
    assert!(
        run.stdout
            .contains("Created initramfs image <root>/esp/EFI/zbm/initramfs-linux-9.9_1.img\n"),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains("Removed file <root>/esp/EFI/zbm/vmlinuz-linux-8.0_1\n"),
        "{}",
        run.stdout
    );
    // With one older version kept, 8.0 goes; the images of the prefix
    // `vmlinuz-linux-lts` stay, though their names start the same.
    for kept in [
        "vmlinuz-linux-9.0_1:",
        "vmlinuz-linux-lts-8.0_1:",
        "initramfs-linux-lts-8.0_1.img:",
    ] {
        assert!(run.state.contains(kept), "{kept}\n{}", run.state);
    }
    assert!(
        !run.state.contains("zbm/vmlinuz-linux-8.0_1:"),
        "{}",
        run.state
    );
}

#[test]
fn an_enabled_section_without_its_image_directory_is_an_error() {
    let config = COMPONENTS.replace("  ImageDir: {root}/esp/EFI/zbm\n", "");
    let run = run_rust(&Case::new(&config, LINUX));

    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(
        run.stdout
            .contains("ERROR: Components.ImageDir is not set\n"),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("Created"), "{}", run.stdout);
}
