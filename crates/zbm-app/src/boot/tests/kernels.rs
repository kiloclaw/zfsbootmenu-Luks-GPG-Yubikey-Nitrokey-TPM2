//! The menu's kernels of a boot environment, and its choice by version,
//! keep only the lines of its own kernel list that name it (SB20).

use std::path::Path;

use zbm_platform::{Installed, KernelLog};

use super::*;

/// A line of another dataset, as a hand or a hook may write it.
const FOREIGN: &str = "q\t/boot/vmlinuz-x\t/boot/initramfs-x.img\n";

/// The kernel log of `fake`, where warnings go.
fn kernel_log(fake: &FakeSystem) -> Installed {
    fake.files.dir("/zfsbootmenu");
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1)
        .without_journal()
        .install()
}

/// The warnings of the kernel log of `fake`.
fn warnings(fake: &FakeSystem) -> Vec<String> {
    fake.kmsg()
        .lines()
        .filter(|line| line.starts_with("<4>"))
        .map(str::to_owned)
        .collect()
}

/// The kernel list of [`BE`] on `fake`.
fn kernel_list(fake: &FakeSystem) -> PathBuf {
    State::new(&fake.files, "/zfsbootmenu").kernels(BE)
}

#[test]
fn the_menu_lists_no_kernel_of_another_dataset() {
    let fake = machine();
    let _log = kernel_log(&fake);
    // Something adds a line of `q` to the list once the search wrote it,
    // as the search reads `org.zfsbootmenu:kernel`.
    let files = fake.files.clone();
    let list = kernel_list(&fake);
    let property = format!("zfs get -H -o value org.zfsbootmenu:kernel {BE}");
    fake.runner.first(move |command| {
        let text = files.text(&list).unwrap_or_default();
        if command.display() == property && !text.contains(FOREIGN) {
            files.file(&list, format!("{text}{FOREIGN}"));
        }
        None
    });

    for snapshot in 1..=2 {
        let kernels = with_boot(&fake, |boot| boot.kernels(&be()));
        let versions: Vec<String> = kernels
            .iter()
            .map(|kernel| kernel.version.get().clone())
            .collect();
        assert_eq!(versions, ["6.6.2", "6.1.10"]);
        assert!(
            fake.files
                .text(kernel_list(&fake))
                .unwrap()
                .contains(FOREIGN)
        );
        let warnings = warnings(&fake);
        assert_eq!(warnings.len(), snapshot, "{warnings:#?}");
    }
}

#[test]
fn a_version_named_only_by_another_dataset_s_line_is_not_selected() {
    let fake = machine();
    let _log = kernel_log(&fake);
    let own = format!("{BE}\t/boot/vmlinuz-6.1.10\t/boot/initramfs-6.1.10.img\n");
    fake.files
        .file(kernel_list(&fake), format!("{own}{FOREIGN}"));
    let fs = FilesystemName::from(be());

    let chosen = with_boot(&fake, |boot| boot.selected(&fs, Some("x")));
    assert_eq!(chosen, None);
    let chosen = with_boot(&fake, |boot| boot.selected(&fs, Some("6.1.10")));
    assert_eq!(
        chosen.map(|kernel| kernel.kernel),
        Some("/boot/vmlinuz-6.1.10".to_owned())
    );
}

#[test]
fn the_menu_chooses_by_version_a_line_of_its_own_with_leading_blanks() {
    let fake = machine();
    let _log = kernel_log(&fake);
    let own = format!("  {BE}\t/boot/vmlinuz-6.1.10\t/boot/initramfs-6.1.10.img\n");
    fake.files.file(kernel_list(&fake), own);
    let fs = FilesystemName::from(be());

    let chosen = with_boot(&fake, |boot| boot.selected(&fs, Some("6.1.10")));
    let chosen = chosen.expect("the line names the environment");
    assert_eq!(chosen.dataset, BE);
    assert_eq!(chosen.kernel, "/boot/vmlinuz-6.1.10");
    assert_eq!(warnings(&fake), Vec::<String>::new());
}
