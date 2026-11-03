//! Tests of the device tree of a boot environment.

use super::*;
use zbm_testkit::fakes::FakeFiles;

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
    let candidate = |property, kernel| devicetree_candidate(property, kernel, mnt);
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
        Some(PathBuf::from("/mnt/usr/6.1.0/x..dtb"))
    );
}
