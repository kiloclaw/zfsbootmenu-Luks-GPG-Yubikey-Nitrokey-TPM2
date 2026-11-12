//! The user's hooks that name a file of the legacy runtime that images of
//! the Rust runtime do not have (ADR 0018, points 20 and 22): the build
//! warns of them.

use std::fmt::Write as _;
use std::path::Path;

use super::*;

/// The settings of a Rust image, whose hooks are those of the fake host's
/// hook root.
const RUST_HOOKS: &str = "zfsbootmenu_runtime\trust\n";

/// The fake host's hook root.
const HOOK_ROOT: &str = "/etc/zfsbootmenu/hooks";

/// The legacy runtime's hook that closes the LUKS volumes, as the checkout
/// has it.
fn legacy_luks_close() -> Vec<u8> {
    std::fs::read(checkout_root().join("hooks/teardown.d/80-luks-close.sh")).unwrap()
}

/// Puts the hook `name` of `stage` in the fake host's hook root, with
/// `text`.
fn hook(files: &FakeFiles, stage: &str, name: &str, text: impl AsRef<[u8]>) {
    let path = Path::new(HOOK_ROOT).join(stage).join(name);
    files.file_with_mode(path, text, 0o755);
}

/// The warnings of the plan `golden` about hooks.
fn hook_warnings(golden: &str) -> Vec<&str> {
    golden
        .lines()
        .filter(|line| line.starts_with("warn\t[zfsbootmenu] hook "))
        .collect()
}

/// What the build says of a hook at `dest` that names `named` alone.
fn names_warning(dest: &str, named: &str) -> String {
    format!(
        "warn\t[zfsbootmenu] hook {dest} names {named}, which images of the Rust runtime do not \
         have"
    )
}

#[test]
fn a_rust_image_warns_about_a_hook_that_sources_a_missing_legacy_file() {
    let settings = format!("{RUST_HOOKS}zfsbootmenu_setup\t/root/hooks/setup.d/60-log.sh\n");
    for builder in [Builder::Dracut, Builder::Mkinitcpio] {
        let plan = plan_with(builder, &settings, |files, _| {
            let text = "source /lib/zfsbootmenu-core.sh\n\
                        source /lib/zfsbootmenu-luks.sh || exit 1\n\
                        /libexec/zfsbootmenu-preview \"$1\"\n";
            hook(files, "setup.d", "50-preview.sh", text);
            let log = "[ -r /lib/echo-log-lib.sh ] && . /lib/echo-log-lib.sh\n";
            files.file_with_mode("/root/hooks/setup.d/60-log.sh", log, 0o755);
            // A program, which the build does not read as text.
            let mut binary = vec![0x7f, b'E', b'L', b'F', 0xff, 0xfe, b'\n'];
            binary.extend_from_slice(b"/lib/zfsbootmenu-luks.sh\n");
            hook(files, "setup.d", "70-binary", binary);
        })
        .unwrap();
        let warnings = hook_warnings(&plan);
        let preview = "/lib/zfsbootmenu-luks.sh, /libexec/zfsbootmenu-preview";
        assert_eq!(
            warnings,
            [
                names_warning("/libexec/hooks/setup.d/60-log.sh", "/lib/echo-log-lib.sh"),
                names_warning("/libexec/hooks/setup.d/50-preview.sh", preview),
            ],
            "{builder:?}"
        );
    }
}

#[test]
fn a_copied_legacy_luks_close_warns_that_volumes_stay_open() {
    for builder in [Builder::Dracut, Builder::Mkinitcpio] {
        let plan = plan_with(builder, RUST_HOOKS, |files, _| {
            hook(files, "teardown.d", "80-luks-close.sh", legacy_luks_close());
        })
        .unwrap();
        assert_eq!(
            hook_warnings(&plan),
            [
                "warn\t[zfsbootmenu] hook /libexec/hooks/teardown.d/80-luks-close.sh takes the \
                 place of the built-in step that closes the LUKS volumes, but closes none: \
                 images of the Rust runtime do not have /lib/zfsbootmenu-luks.sh, so the \
                 volumes stay open across kexec; delete it, or make it run `zluks close`"
            ],
            "{builder:?}"
        );
    }
}

#[test]
fn a_luks_close_that_runs_zluks_close_does_not_warn() {
    let plan = plan_with(Builder::Dracut, RUST_HOOKS, |files, _| {
        let text = "#!/bin/sh\n\
                    source /lib/kmsg-log-lib.sh\n\
                    source /lib/zfsbootmenu-core.sh\n\
                    /bin/zluks close\n";
        hook(files, "teardown.d", "80-luks-close.sh", text);
        // The legacy hook under another name takes no step's place.
        hook(files, "teardown.d", "70-close.sh", legacy_luks_close());
    })
    .unwrap();
    assert_eq!(
        hook_warnings(&plan),
        [names_warning(
            "/libexec/hooks/teardown.d/70-close.sh",
            "/lib/zfsbootmenu-luks.sh"
        )]
    );
}

#[test]
fn a_luks_close_that_names_another_missing_file_gets_the_plain_warning() {
    let plan = plan_with(Builder::Dracut, RUST_HOOKS, |files, _| {
        let text = "source /lib/echo-log-lib.sh\n/bin/zluks close\n";
        hook(files, "teardown.d", "80-luks-close.sh", text);
    })
    .unwrap();
    assert_eq!(
        hook_warnings(&plan),
        [names_warning(
            "/libexec/hooks/teardown.d/80-luks-close.sh",
            "/lib/echo-log-lib.sh"
        )]
    );
}

#[test]
fn a_hook_of_the_module_root_named_by_an_option_is_checked() {
    let settings = format!(
        "{RUST_HOOKS}zfsbootmenu_teardown\t{MODULE_ROOT}/hooks/teardown.d/80-luks-close.sh\n"
    );
    let plan = plan_with(Builder::Dracut, &settings, |_, _| {}).unwrap();
    let warnings = hook_warnings(&plan);
    assert_eq!(warnings.len(), 1, "{plan}");
    assert!(
        warnings[0].starts_with(
            "warn\t[zfsbootmenu] hook /libexec/hooks/teardown.d/80-luks-close.sh takes the \
             place of"
        ),
        "{plan}"
    );
}

#[test]
fn a_script_that_is_not_utf8_is_still_checked() {
    let plan = plan_with(Builder::Dracut, RUST_HOOKS, |files, _| {
        // A comment in ISO-8859-1: "Ü" is the byte 0xdc.
        let mut text = b"#!/bin/sh\n# \xdcbersetzung\n".to_vec();
        text.extend_from_slice(b"source /lib/zfsbootmenu-luks.sh\n");
        hook(files, "setup.d", "40-latin1.sh", text);
    })
    .unwrap();
    assert_eq!(
        hook_warnings(&plan),
        [names_warning(
            "/libexec/hooks/setup.d/40-latin1.sh",
            "/lib/zfsbootmenu-luks.sh"
        )]
    );
}

#[test]
fn a_legacy_image_does_not_warn() {
    let hooks = |files: &FakeFiles, _: &FakeRunner| {
        hook(files, "teardown.d", "80-luks-close.sh", legacy_luks_close());
        let text = "/libexec/zfsbootmenu-preview \"$1\"\n";
        hook(files, "setup.d", "50-preview.sh", text);
    };
    let rust = plan_with(Builder::Dracut, RUST_HOOKS, hooks).unwrap();
    assert_eq!(hook_warnings(&rust).len(), 2, "{rust}");
    for builder in [Builder::Dracut, Builder::Mkinitcpio] {
        let legacy = plan_with(builder, "", hooks).unwrap();
        assert!(legacy.contains("\t/libexec/hooks/teardown.d/80-luks-close.sh\t"));
        assert_eq!(hook_warnings(&legacy), [] as [&str; 0], "{builder:?}");
    }
}

#[test]
fn a_hook_naming_a_linked_tool_does_not_warn() {
    let mut text = String::new();
    for linked in zbm_types::LINKED {
        let _ = writeln!(text, "{}/{} --help", linked.dir, linked.name);
    }
    // What the image takes, generates or writes of the module root.
    for path in [
        "/lib/zfsbootmenu-core.sh",
        "/lib/kmsg-log-lib.sh",
        "/lib/zfsbootmenu-kcl.sh",
        "/libexec/zfsbootmenu-input",
        "/libexec/zfsbootmenu-run-hooks",
        "/bin/zbm",
    ] {
        text.push_str(path);
        text.push('\n');
    }
    text.push_str("/libexec/zfsbootmenu-help -L recovery-shell\n");
    let plan = plan_with(Builder::Dracut, RUST_HOOKS, |files, _| {
        hook(files, "boot-sel.d", "20-tools.sh", text.as_str());
    })
    .unwrap();
    assert_eq!(
        hook_warnings(&plan),
        [names_warning(
            "/libexec/hooks/boot-sel.d/20-tools.sh",
            "/libexec/zfsbootmenu-help"
        )]
    );
}
