//! Tests of the device tree of a boot environment.

use super::*;
use zbm_testkit::fakes::FakeFiles;

/// The device tree the boot loads: the candidate of `property`, when it is
/// set and the file is found inside the boot environment mounted at `mnt`,
/// as the boot sequence finds it.
fn devicetree(
    files: &dyn Files,
    property: Option<&str>,
    kernel: &str,
    mnt: &Path,
) -> Option<PathBuf> {
    let property = property?;
    let inside = devicetree_candidate(property, kernel)?;
    crate::kernels::boot_image(files, mnt, &inside)
        .ok()
        .map(PathBuf::from)
}

#[test]
fn device_trees() {
    let files = FakeFiles::new();
    let mnt = Path::new("/mnt");
    files.file("/mnt/boot/dtbs/dtbs-6.1.0/qcom/x13s.dtb", "");
    files.file("/mnt/usr/lib/linux-image-6.1.0/x13s.dtb", "");
    let dtb = |property: &str, kernel: &str| {
        devicetree(&files, Some(property), kernel, mnt).map(|path| path.display().to_string())
    };
    assert_eq!(
        dtb("qcom/x13s.dtb", "/boot/vmlinuz-6.1.0"),
        Some("/mnt/boot/dtbs/dtbs-6.1.0/qcom/x13s.dtb".to_owned())
    );
    assert_eq!(
        dtb(
            "/usr/lib/linux-image-%{kernel}/x13s.dtb",
            "/boot/vmlinuz-6.1.0"
        ),
        Some("/mnt/usr/lib/linux-image-6.1.0/x13s.dtb".to_owned())
    );
    assert_eq!(dtb("qcom/missing.dtb", "/boot/vmlinuz-6.1.0"), None);
    assert_eq!(
        devicetree(&files, Some("-"), "/boot/vmlinuz-6.1.0", mnt),
        None
    );
    assert_eq!(devicetree(&files, None, "/boot/vmlinuz-6.1.0", mnt), None);
    // Without a hyphen the "version" is the whole path, as in the legacy code.
    assert_eq!(dtb("x.dtb", "/boot/vmlinuz"), None);
    // A path out of the boot environment is not tried.
    let candidate = devicetree_candidate;
    assert_eq!(
        candidate("../../../../etc/x.dtb", "/boot/vmlinuz-6.1.0"),
        None
    );
    assert_eq!(candidate("/../etc/x.dtb", "/boot/vmlinuz-6.1.0"), None);
    assert_eq!(
        candidate("/usr/%{kernel}/x.dtb", "/boot/vmlinuz-6.1.0-../.."),
        None
    );
    assert_eq!(
        candidate("/usr/%{kernel}/x..dtb", "/boot/vmlinuz-6.1.0"),
        Some(PathBuf::from("/usr/6.1.0/x..dtb"))
    );
}

#[test]
fn a_device_tree_is_found_inside_the_boot_environment() {
    let files = FakeFiles::new();
    let mnt = Path::new("/mnt");
    files
        .file("/etc/zfs/key", "SECRET")
        .file("/mnt/boot/dtbs/real.dtb", "")
        .symlink(
            "/mnt/boot/dtbs/dtbs-6.1.0/inside.dtb",
            "/boot/dtbs/real.dtb",
        )
        .symlink("/mnt/boot/dtbs/dtbs-6.1.0/key.dtb", "/etc/zfs/key")
        .fifo("/mnt/boot/dtbs/dtbs-6.1.0/fifo.dtb");
    let dtb = |property: &str| devicetree(&files, Some(property), "/boot/vmlinuz-6.1.0", mnt);

    assert_eq!(
        dtb("inside.dtb"),
        Some(PathBuf::from("/mnt/boot/dtbs/real.dtb"))
    );
    // The image's key is not the environment's, which has none.
    assert_eq!(dtb("key.dtb"), None);
    assert_eq!(dtb("/etc/zfs/key"), None);
    assert_eq!(dtb("fifo.dtb"), None);
}
