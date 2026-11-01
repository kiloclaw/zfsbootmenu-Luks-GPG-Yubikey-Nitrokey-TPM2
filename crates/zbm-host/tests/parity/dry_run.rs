//! `generate-zbm --dry-run` (ADR 0013): what a run would do, and nothing
//! done. The plans of a set of configurations are golden files
//! (`snapshots/`), so that a change to what generate-zbm does is a diff to
//! review; `cargo insta review` takes a changed plan.

use crate::fixtures::{BASIC, COMPONENTS, setup_backups};
use crate::sandbox::{Case, Sandbox, efi_tools, run_rust, tools};
use zbm_testkit::can_run;

/// The plan of `case`: what the run printed, with its exit status; and the
/// files it left, which must be those it found.
fn plan(case: &Case) -> String {
    let before = run_rust(&Case {
        args: &["-V"],
        ..clone_case(case)
    });
    let run = run_rust(case);
    assert_eq!(
        without_calls(&run.state),
        without_calls(&before.state),
        "a dry run changes nothing"
    );
    assert!(!run.log.contains("dracut ["), "{}", run.log);
    format!("exit {}\n{}", run.code, run.stdout)
}

/// The files of `state` but the log of the fake programs' calls, which
/// read-only checks such as `mountpoint` write.
fn without_calls(state: &str) -> String {
    let Some(start) = state.find("calls.log, mode 644:\n") else {
        return state.to_owned();
    };
    let end = state[start..]
        .find("config.yaml")
        .map_or(state.len(), |at| start + at);
    format!("{}{}", &state[..start], &state[end..])
}

/// `case` again, for a second sandbox.
fn clone_case(case: &Case) -> Case {
    Case {
        config: case.config.clone(),
        args: case.args,
        env: case.env,
        setup: case.setup,
        differs: case.differs,
    }
}

fn golden(name: &str, case: &Case) {
    let plan = plan(case);
    insta::with_settings!({ prepend_module_to_snapshot => false, omit_expression => true }, {
        insta::assert_snapshot!(name, plan);
    });
}

#[test]
fn components_with_revisions() {
    if !can_run(tools(), "binutils") {
        return;
    }
    golden(
        "dry-run-components",
        &Case::new(
            COMPONENTS,
            &["-n", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
        ),
    );
}

#[test]
fn a_bundle_and_backups_with_a_boot_partition_and_hooks() {
    if !can_run(tools() && efi_tools(), "binutils or the systemd EFI stub") {
        return;
    }
    let config = BASIC.replace("  Versions: 3\n", "  Versions: false\n");
    golden(
        "dry-run-bundle-backups",
        &Case {
            setup: setup_backups,
            ..Case::new(
                &config,
                &["--dry-run", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
}

#[test]
fn enable_says_what_it_would_change() {
    let run = run_rust(&Case::new(
        "Global:\n  ManageImages: false\n",
        &["-n", "-c", "{root}/config.yaml", "--enable"],
    ));
    assert_eq!(run.code, 0);
    assert_eq!(
        run.stdout,
        "Would set ManageImages to 'true' in <root>/config.yaml\n"
    );
    assert!(run.state.contains("ManageImages: false"), "{}", run.state);
}

/// Old initramfs images of a prefix other than vmlinuz: `initramfs-*`
/// beside the prefix's kernels.
fn setup_old_initramfs(sandbox: &Sandbox) {
    std::fs::create_dir_all(sandbox.path().join("esp/EFI/zbm")).unwrap();
    for name in [
        "zbm-6.2.5-test_1",
        "initramfs-6.2.5-test_1.img",
        "initramfs-9_1.img",
    ] {
        sandbox.dir.write(format!("esp/EFI/zbm/{name}"), name);
    }
}

#[test]
fn old_initramfs_names_of_another_prefix_are_warned_about() {
    if !can_run(tools(), "binutils") {
        return;
    }
    let run = run_rust(&Case {
        setup: setup_old_initramfs,
        ..Case::new(
            COMPONENTS,
            &[
                "-c",
                "{root}/config.yaml",
                "-k",
                "{root}/boot/linux-lts",
                "-p",
                "zbm",
                "-v",
                "9.9",
            ],
        )
    });

    assert_eq!(run.code, 0, "{}", run.stdout);
    assert!(
        run.stdout.contains(
            "WARNING: the initramfs of this kernel prefix is now named initramfs-zbm-*.img; \
             1 image(s) of the old name"
        ),
        "{}",
        run.stdout
    );
    let plain = run_rust(&Case::new(
        COMPONENTS,
        &["-c", "{root}/config.yaml", "-b", "{root}/boot"],
    ));
    assert!(
        !plain.stdout.contains("initramfs of this kernel prefix"),
        "{}",
        plain.stdout
    );
}
