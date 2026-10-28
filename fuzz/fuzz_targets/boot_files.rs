//! The files of a boot environment's `/boot` and its `os-release` (input I5
//! of docs/dev/threat-model.md), which whoever made the pool chooses (A3).
//! The input is split at its first NUL: what comes before is `os-release`;
//! what follows is a list of names, one per line, made files of `/boot`.
//! Finding the kernels, their initramfs images and command line files, the
//! choice of kernel, and the `root=` prefix from `os-release` may not panic
//! or hang; every kernel found is a file of `/boot` with an initramfs there,
//! and the prefix is one of the known ones.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use zbm_boot::commandline::{os_release, root_prefix};
use zbm_boot::kernels::{discover, select};
use zbm_testkit::fakes::FakeFiles;
use zbm_types::KernelSelector;

/// Where the boot environment is mounted.
const MNT: &str = "/mnt";

/// The prefixes `root_prefix` gives without `org.zfsbootmenu:rootprefix`.
const KNOWN: [&str; 3] = ["root=zfs:", "zfs=", "root=ZFS="];

fuzz_target!(|data: &[u8]| {
    let (release, names) = match data.iter().position(|&byte| byte == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (data, &[][..]),
    };
    let release = String::from_utf8_lossy(release);
    let _ = os_release(&release);

    let files = FakeFiles::new();
    files.dir(Path::new(MNT).join("boot"));
    files.file(Path::new(MNT).join("etc/os-release"), release.as_bytes());
    let names = String::from_utf8_lossy(names);
    for name in names.lines().take(64) {
        // A file name: no separator, and not the directory itself.
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            continue;
        }
        files.file(Path::new(MNT).join("boot").join(name), b"");
    }

    let found = discover(&files, "zroot/ROOT/void", Path::new(MNT));
    for kernel in &found.kernels {
        assert!(kernel.kernel.starts_with("/boot/"), "{kernel:?}");
        assert!(kernel.initramfs.starts_with("/boot/"), "{kernel:?}");
        assert_eq!(kernel.dataset, "zroot/ROOT/void");
    }
    // The last name also chooses the kernel, as `org.zfsbootmenu:kernel`.
    let selector = names
        .lines()
        .last()
        .and_then(|text| text.parse::<KernelSelector>().ok());
    if let Some(chosen) = select(&found.kernels, selector.as_ref()) {
        assert!(found.kernels.contains(chosen));
    }

    let prefix = root_prefix(&files, None, Path::new(MNT));
    assert!(KNOWN.contains(&prefix.as_str()), "{prefix:?}");
});
