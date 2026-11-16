//! The files of a boot environment's `/boot` and its `os-release` (input I5
//! of docs/dev/threat-model.md), which whoever made the pool chooses (A3).
//! The input is split at its NUL bytes: what comes before the first is
//! `os-release`; each part after it is a name made a file of `/boot`, so
//! that names may hold newlines and tabs, or, written `a>b`, a symbolic
//! link `a` to `b`, which may lead anywhere. The image has its own key at
//! `/etc/zfs/key`, outside the environment.
//! Finding the kernels, their initramfs images and command line files, the
//! choice of kernel, and the `root=` prefix from `os-release` may not panic
//! or hang; every kernel found, and its initramfs, is a name of `/boot`
//! with no control character, reads back from its line of the kernel list,
//! and resolves inside the environment to a regular file; and the prefix
//! is one of the known ones.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use zbm_bootenv::commandline::{os_release, root_prefix};
use zbm_bootenv::kernels::{Kernel, boot_image, discover, select};
use zbm_platform::Files;
use zbm_testkit::fakes::FakeFiles;
use zbm_types::KernelSelector;

/// Where the boot environment is mounted.
const MNT: &str = "/mnt";

/// The prefixes `root_prefix` gives without `org.zfsbootmenu:rootprefix`.
const KNOWN: [&str; 3] = ["root=zfs:", "zfs=", "root=ZFS="];

/// Whether `name` can be a name in a directory.
fn is_file_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && name != "." && name != ".."
}

fuzz_target!(|data: &[u8]| {
    let mut parts = data.split(|&byte| byte == 0);
    let release = String::from_utf8_lossy(parts.next().unwrap_or_default());
    let _ = os_release(&release);

    let files = FakeFiles::new();
    files.file("/etc/zfs/key", "SECRET");
    files.dir(Path::new(MNT).join("boot"));
    files.file(Path::new(MNT).join("etc/os-release"), release.as_bytes());
    let names: Vec<String> = parts
        .take(64)
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    for name in &names {
        let boot = Path::new(MNT).join("boot");
        match name.split_once('>') {
            Some((link, target)) if is_file_name(link) && !target.is_empty() => {
                files.symlink(boot.join(link), target);
            }
            Some(_) => {}
            None if is_file_name(name) => {
                files.file(boot.join(name), b"");
            }
            None => {}
        }
    }

    let found = discover(&files, "zroot/ROOT/void", Path::new(MNT));
    for kernel in &found.kernels {
        assert_eq!(kernel.dataset, "zroot/ROOT/void");
        assert_eq!(&Kernel::parse(&kernel.line()), kernel);
        for name in [&kernel.kernel, &kernel.initramfs] {
            assert!(name.starts_with("/boot/"), "{kernel:?}");
            assert!(!name.contains(char::is_control), "{kernel:?}");
            let opened = boot_image(&files, Path::new(MNT), Path::new(name));
            let opened = opened.unwrap_or_else(|why| panic!("{kernel:?}: {why}"));
            assert!(opened.starts_with("/mnt/"), "{opened}");
            assert!(files.is_file(Path::new(&opened)), "{opened}");
        }
    }
    for (_, kcl) in &found.kcl_files {
        assert!(kcl.starts_with(MNT), "{}", kcl.display());
    }
    // The last name also chooses the kernel, as `org.zfsbootmenu:kernel`.
    let selector = names
        .last()
        .and_then(|text| text.parse::<KernelSelector>().ok());
    if let Some(chosen) = select(&found.kernels, selector.as_ref()) {
        assert!(found.kernels.contains(chosen));
    }

    let prefix = root_prefix(&files, None, Path::new(MNT));
    assert!(KNOWN.contains(&prefix.as_str()), "{prefix:?}");
});
