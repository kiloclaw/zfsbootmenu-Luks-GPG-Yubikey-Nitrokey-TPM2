use pretty_assertions::assert_eq;

use super::*;

const DEP: &str = "\
kernel/fs/zfs/zfs.ko.zst: kernel/fs/zfs/spl.ko.zst kernel/lib/zlib.ko.zst
kernel/fs/zfs/spl.ko.zst: kernel/lib/zlib.ko.zst
kernel/lib/zlib.ko.zst:
kernel/drivers/gpu/drm/i915/i915.ko.xz: kernel/drivers/gpu/drm/drm-kms-helper.ko.xz
kernel/drivers/gpu/drm/drm-kms-helper.ko.xz:
kernel/drivers/char/tpm/tpm_crb.ko:
kernel/crypto/crc32c.ko:
/opt/extra/odd.ko:
";

const ALIAS: &str = "\
# Aliases extracted from modules themselves.
alias pci:v00008086d*sv*sd*bc03sc*i* i915
alias acpi*:MSFT0101:* tpm_crb
";

const SOFTDEP: &str = "softdep zfs pre: crc32c post: tpm_crb\n";

const BUILTIN: &str = "kernel/drivers/tty/vt.ko\nkernel/fs/ext4/ext4.ko\n";

fn index(config: &[&str], cmdline: &str) -> Index {
    let kernel = KernelFiles {
        dep: DEP,
        alias: ALIAS,
        softdep: SOFTDEP,
        builtin: BUILTIN,
    };
    Index::parse(Path::new("/lib/modules/6.12"), &kernel, config, cmdline)
}

/// The names, parameters and optionality of a plan's steps.
fn steps(plan: &Plan) -> Vec<(String, String, bool)> {
    let Plan::Insert(steps) = plan else {
        panic!("not a plan of steps: {plan:?}");
    };
    steps
        .iter()
        .map(|step| (step.name.clone(), step.parameters.clone(), step.optional))
        .collect()
}

fn step(name: &str, parameters: &str, optional: bool) -> (String, String, bool) {
    (name.to_owned(), parameters.to_owned(), optional)
}

#[test]
fn names_come_from_files_and_dashes_are_underscores() {
    assert_eq!(name_of("kernel/fs/zfs/zfs.ko.zst"), "zfs");
    assert_eq!(name_of("drm-kms-helper.ko.xz"), "drm_kms_helper");
    assert_eq!(name_of("tpm_crb.ko"), "tpm_crb");
    assert_eq!(normalized("drm-kms-helper"), "drm_kms_helper");
    assert_eq!(normalized_pattern("pci-x[a-c]-y"), "pci_x[a-c]_y");
}

#[test]
fn a_module_comes_after_what_it_needs_and_between_its_softdeps() {
    let config = "options zfs zfs_arc_max=1073741824\noptions spl spl_hostid=0x1\n";
    let index = index(
        &[config],
        "BOOT_IMAGE=/vmlinuz zfs.zfs_vdev_max=4 quiet root=zfs",
    );

    let plan = index.plan("zfs", &["zfs_debug=1"]);

    assert_eq!(
        steps(&plan),
        [
            step("crc32c", "", true),
            step("zlib", "", false),
            step("spl", "spl_hostid=0x1", false),
            step(
                "zfs",
                "zfs_arc_max=1073741824 zfs_vdev_max=4 zfs_debug=1",
                false
            ),
            step("tpm_crb", "", true),
        ]
    );
    let Plan::Insert(steps) = plan else {
        panic!("not a plan of steps");
    };
    assert_eq!(
        steps[2].path,
        Path::new("/lib/modules/6.12/kernel/fs/zfs/spl.ko.zst")
    );
    assert_eq!(index.file("odd"), Some(PathBuf::from("/opt/extra/odd.ko")));
    assert_eq!(index.file("none"), None);
    assert!(!index.is_empty());
    assert!(Index::default().is_empty());
}

#[test]
fn aliases_builtins_blacklists_and_installs_decide_what_loads() {
    let config = "\
alias my-gpu i915
blacklist tpm_crb
install crc32c /bin/true
softdep i915 pre: drm-kms-helper
";
    let index = index(&[config], "modprobe.blacklist=nouveau,");

    assert_eq!(
        steps(&index.plan("my-gpu", &[])),
        [step("drm_kms_helper", "", true), step("i915", "", false)]
    );
    assert_eq!(
        steps(&index.plan("pci:v00008086d00001234sv0sd0bc03sc00i00", &[])),
        [step("drm_kms_helper", "", true), step("i915", "", false)]
    );
    // A blacklisted module is not loaded by an alias, but by its name.
    assert_eq!(index.plan("acpi:MSFT0101:", &[]), Plan::Unknown);
    assert_eq!(
        steps(&index.plan("tpm_crb", &[])),
        [step("tpm_crb", "", false)]
    );
    assert_eq!(index.plan("vt", &[]), Plan::Insert(Vec::new()));
    assert_eq!(index.plan("crc32c", &[]), Plan::Command);
    assert_eq!(index.plan("zfs", &[]), Plan::Command);
    assert_eq!(index.plan("nonexistent", &[]), Plan::Unknown);
}

#[test]
fn configuration_lines_go_on_and_comments_are_dropped() {
    let config = "options zfs \\\n  zfs_arc_max=1 # the cache\n# options spl x=1\noptions\n";
    let index = index(&[config], "");
    let plan = index.plan("zfs", &[]);
    let found = steps(&plan);
    assert_eq!(found[3], step("zfs", "zfs_arc_max=1", false));
    assert_eq!(found[2], step("spl", "", false));
    assert_eq!(logical_lines("a \\\nb"), ["a  b"]);
    assert_eq!(logical_lines("a \\"), ["a  "]);
}

#[test]
fn patterns_match_as_the_shell_s() {
    for (pattern, text, matches) in [
        ("pci:v*d*", "pci:v8086d1234", true),
        ("a?c", "abc", true),
        ("a?c", "ac", false),
        ("x[0-9]y", "x5y", true),
        ("x[!0-9]y", "x5y", false),
        ("x[^a]y", "xby", true),
        ("x[]]y", "x]y", true),
        ("x[ab", "xa", false),
        ("*", "", true),
        ("a*", "b", false),
        ("*b*c", "abxbc", true),
        ("*[0-9]", "ab7", true),
        ("*[0-9]", "ab", false),
    ] {
        assert_eq!(
            matches_glob(pattern.as_bytes(), text.as_bytes()),
            matches,
            "{pattern} {text}"
        );
    }
}
