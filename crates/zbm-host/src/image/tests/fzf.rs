//! `fzf` and the flags of `/etc/zfsbootmenu.conf` that say what it can
//! do: in legacy plans only (ADR 0018, point 21).

use std::path::Path;

use super::*;

/// The settings of the legacy plans, with their builder.
const LEGACY: [(Builder, &str); 3] = [
    (Builder::Dracut, ""),
    (Builder::Mkinitcpio, ""),
    (Builder::Mkinitcpio, "zfsbootmenu_miser\tyes\n"),
];

/// The flags of the legacy menu, in the order of the file.
const FLAGS: [&str; 6] = [
    "HAS_REFRESH",
    "HAS_DISABLED",
    "HAS_BORDER",
    "HAS_COLUMN",
    "HAS_COLORS",
    "HAS_RAW",
];

/// `/etc/zfsbootmenu.conf` of `plan`.
fn conf(plan: &Plan) -> String {
    let text = plan.written(Path::new("/etc/zfsbootmenu.conf")).unwrap();
    String::from_utf8_lossy(text).into_owned()
}

#[test]
fn no_rust_plan_names_fzf_or_runs_it() {
    for (builder, settings) in RUST {
        let (files, runner) = (host_files(), host_runner());
        let plan = plan_of(builder, settings, &files, &runner).unwrap();
        let golden = plan.golden();
        assert!(!golden.contains("fzf"), "{builder:?}:\n{golden}");
        let ran = runner.lines();
        assert!(!ran.iter().any(|line| line.contains("fzf")), "{ran:?}");
    }
}

#[test]
fn a_rust_conf_has_no_has_flags() {
    let (files, runner) = (host_files(), host_runner());
    for (builder, settings) in RUST {
        let conf = conf(&plan_of(builder, settings, &files, &runner).unwrap());
        assert!(!conf.contains("HAS_"), "{builder:?}:\n{conf}");
        assert!(conf.contains("\nexport ZBM_RUNTIME=\"rust\"\n"), "{conf}");
    }
}

#[test]
fn legacy_plans_keep_fzf_and_the_flags() {
    for (builder, settings) in LEGACY {
        let (files, runner) = (host_files(), host_runner());
        let plan = plan_of(builder, settings, &files, &runner).unwrap();
        let golden = plan.golden();
        for name in LEGACY_BINARIES {
            let binary = format!("\nbinary\t/usr/bin/{name}\t/usr/bin/{name}\n");
            assert!(golden.contains(&binary), "{builder:?} {settings:?}: {name}");
        }
        let conf = conf(&plan);
        let names: Vec<&str> = conf
            .lines()
            .filter_map(|line| line.strip_prefix("export ")?.split_once('='))
            .map(|(name, _)| name)
            .filter(|name| name.starts_with("HAS_"))
            .collect();
        assert_eq!(names, FLAGS, "{builder:?}:\n{conf}");
        // What `fzf` can do is asked of the build host's `fzf`.
        assert!(
            runner
                .lines()
                .contains(&"fzf -f abc --raw --exit-0".to_owned())
        );
    }
}

#[test]
fn a_rust_image_builds_on_a_host_without_fzf() {
    let without = |files: &FakeFiles, _: &FakeRunner| {
        files.remove_file(Path::new("/usr/bin/fzf")).unwrap();
    };
    for (builder, settings) in RUST {
        let plan = plan_with(builder, settings, without);
        assert!(plan.is_ok(), "{builder:?}: {plan:?}");
    }
    for (builder, settings) in LEGACY {
        let fatal = plan_with(builder, settings, without).unwrap_err();
        assert_eq!(fatal, "failed to install essential executable 'fzf'");
    }
}
