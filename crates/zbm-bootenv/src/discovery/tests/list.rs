//! The readers of a kernel list keep only the lines of its own boot
//! environment (SB20): a line that names another dataset, written by hand
//! or by a hook, is never selected nor listed.

use std::path::Path;

use zbm_platform::{Context, Installed, KernelLog};
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use crate::kernels::Kernel;
use crate::testing::name;
use crate::{listed_kernels, select_kernel};

/// The boot environment.
const FS: &str = "tank/ROOT/be";
/// Its line for `/boot/vmlinuz-1`.
const ONE: &str = "tank/ROOT/be\t/boot/vmlinuz-1\t/boot/initramfs-1.img\n";

/// A machine where the kernel list of [`FS`] is `list`, whose
/// `org.zfsbootmenu:kernel` is `property`, with the kernel log, where
/// warnings go.
fn machine(list: &str, property: &str) -> (FakeSystem, Installed) {
    let fake = FakeSystem::new();
    let state = State::new(&fake.files, "/zfsbootmenu");
    fake.files.dir("/zfsbootmenu").file(state.kernels(FS), list);
    fake.runner.respond(
        &format!("zfs get -H -o value org.zfsbootmenu:kernel {FS}"),
        0,
        &format!("{property}\n"),
    );
    let log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1)
        .without_journal()
        .install();
    (fake, log)
}

/// The kernel the list of [`FS`] on `fake` selects.
fn selected(fake: &FakeSystem) -> Option<Kernel> {
    let state = State::new(&fake.files, "/zfsbootmenu");
    select_kernel(&Context::new(fake.system()), &state, &name(FS))
}

/// The kernels the list of [`FS`] on `fake` lists.
fn listed(fake: &FakeSystem) -> Vec<Kernel> {
    let state = State::new(&fake.files, "/zfsbootmenu");
    listed_kernels(&Context::new(fake.system()), &state, &name(FS))
}

/// The lines of the kernel log at `level`.
fn lines_at(fake: &FakeSystem, level: u8) -> Vec<String> {
    let prefix = format!("<{level}>");
    fake.kmsg()
        .lines()
        .filter(|line| line.starts_with(&prefix))
        .map(str::to_owned)
        .collect()
}

/// The kernel `/boot/vmlinuz-1` of [`FS`].
fn one() -> Kernel {
    Kernel::new(FS, "/boot/vmlinuz-1", "/boot/initramfs-1.img")
}

#[test]
fn a_kernel_list_line_of_another_dataset_is_not_selected() {
    let (fake, _log) = machine(&format!("{ONE}q\tx\ty\n"), "-");
    assert_eq!(selected(&fake), Some(one()));
}

#[test]
fn a_kernel_list_line_of_another_dataset_is_not_matched_by_the_property() {
    let (fake, _log) = machine(&format!("q\tx\ty\n{ONE}"), "x");
    assert_eq!(selected(&fake), Some(one()));
}

#[test]
fn a_kernel_list_of_other_datasets_only_selects_nothing() {
    let (fake, _log) = machine("q\tx\ty\ntank/ROOT/other\t/boot/vmlinuz-1\ti\n", "-");
    assert_eq!(selected(&fake), None);
    let errors = lines_at(&fake, 3);
    assert!(
        errors
            .iter()
            .any(|line| line.contains("failed to identify kexec arguments for tank/ROOT/be")),
        "{errors:#?}"
    );
    assert_eq!(listed(&fake), []);
}

#[test]
fn a_kernel_list_with_a_trailing_empty_line_selects_its_last_kernel() {
    // Rust never writes one; a hand or a hook may.
    let (fake, _log) = machine(&format!("{ONE}\n"), "-");
    assert_eq!(selected(&fake), Some(one()));
    assert_eq!(listed(&fake), [one()]);
    assert_eq!(lines_at(&fake, 4), Vec::<String>::new());
}

#[test]
fn the_selector_still_matches_lines_with_surrounding_blanks() {
    // As the legacy `read -r` reads them: the line is trimmed of spaces and
    // tabs, its initramfs field with it.
    let list = format!(
        " \t{FS}\t/boot/vmlinuz-1\t/boot/initramfs-1.img \t\n\
         {FS}\t/boot/vmlinuz-2\t/boot/initramfs-2.img\n"
    );
    let (fake, _log) = machine(&list, "vmlinuz-1");
    assert_eq!(selected(&fake), Some(one()));
}

#[test]
fn a_line_of_the_environment_with_leading_blanks_is_listed_and_selected_alike() {
    // Blanks before the name are not part of it: every reader takes the
    // line as the environment's, and none warns of it.
    let blanks = format!(" \t{ONE}");
    let (fake, _log) = machine(&blanks, "-");
    assert_eq!(listed(&fake), [one()]);
    assert_eq!(selected(&fake), Some(one()));
    assert_eq!(lines_at(&fake, 4), Vec::<String>::new());

    let (fake, _log) = machine(&blanks, "vmlinuz-1");
    assert_eq!(listed(&fake), [one()]);
    assert_eq!(selected(&fake), Some(one()));
    assert_eq!(lines_at(&fake, 4), Vec::<String>::new());
}

#[test]
fn listed_kernels_warns_for_each_ignored_line_escaped() {
    let list = format!("q\tx\ty\x1b[2J\n{ONE}tank/ROOT/other\t/boot/vmlinuz-2\ti\n");
    let (fake, _log) = machine(&list, "-");

    assert_eq!(listed(&fake), [one()]);
    let warnings = lines_at(&fake, 4);
    assert_eq!(warnings.len(), 2, "{warnings:#?}");
    assert!(warnings[0].contains("the kernel list of tank/ROOT/be"));
    assert!(warnings[1].contains("tank/ROOT/other"), "{warnings:#?}");
    assert!(!fake.kmsg().contains('\x1b'), "{}", fake.kmsg());

    // The selection ignores them too, and says so at debug level only.
    assert_eq!(selected(&fake), Some(one()));
    assert_eq!(lines_at(&fake, 4).len(), 2);
    let ignored = lines_at(&fake, 7)
        .into_iter()
        .filter(|line| line.contains("a line of another dataset is ignored"))
        .count();
    assert_eq!(ignored, 2, "{}", fake.kmsg());
    assert!(!fake.kmsg().contains('\x1b'), "{}", fake.kmsg());
}
