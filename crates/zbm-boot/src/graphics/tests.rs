use std::path::Path;
use std::time::Duration;

use zbm_platform::{Installed, KernelLog};
use zbm_testkit::fakes::FakeSystem;

use super::*;

const ROOT: &str = "/";

fn log(fake: &FakeSystem) -> Installed {
    fake.files.dir("/zfsbootmenu");
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
}

/// A PCI device below `/sys/bus/pci/devices`.
fn device(fake: &FakeSystem, address: &str, class: &str, vendor: &str, device: &str) {
    let dir = format!("/sys/bus/pci/devices/{address}");
    fake.files
        .file(format!("{dir}/class"), format!("{class}\n"))
        .file(format!("{dir}/vendor"), format!("{vendor}\n"))
        .file(format!("{dir}/device"), format!("{device}\n"));
}

/// The DRM card a driver made for `address`, with one connector.
fn card(fake: &FakeSystem, address: &str, card: &str, status: &str) {
    fake.files
        .dir(format!("/sys/bus/pci/devices/{address}/drm/{card}"))
        .dir(format!("/sys/bus/pci/devices/{address}/drm/renderD128"))
        .file(
            format!("/sys/class/drm/{card}-DP-1/status"),
            format!("{status}\n"),
        );
}

/// A machine with an Intel GPU, an AMD GPU and a network card, and an image
/// with `drivers`.
fn machine(drivers: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .file(format!("/{DRIVERS}"), format!("{drivers}\n"));
    device(&fake, "0000:00:02.0", "0x030000", "0x8086", "0x4680");
    device(&fake, "0000:03:00.0", "0x030000", "0x1002", "0x7590");
    device(&fake, "0000:04:00.0", "0x020000", "0x8086", "0x15f3");
    fake
}

fn overridden(fake: &FakeSystem) -> Vec<String> {
    fake.files
        .paths()
        .into_iter()
        .filter(|path| path.ends_with("driver_override"))
        .map(|path| {
            let text = fake.files.text(&path).unwrap_or_default();
            format!("{} {}", path.display(), text.trim())
        })
        .collect()
}

#[test]
fn zbm_gpu_names_an_address_a_vendor_or_off() {
    assert_eq!(Choice::parse(None), Ok(Choice::All));
    assert_eq!(Choice::parse(Some(" ")), Ok(Choice::All));
    assert_eq!(Choice::parse(Some("off")), Ok(Choice::Off));
    assert_eq!(Choice::parse(Some("AMD")), Ok(Choice::Vendor(0x1002)));
    assert_eq!(Choice::parse(Some("intel")), Ok(Choice::Vendor(0x8086)));
    assert_eq!(
        Choice::parse(Some("03:00.0")),
        Ok(Choice::Address("0000:03:00.0".to_owned()))
    );
    assert_eq!(
        Choice::parse(Some("0000:0A:00.1")),
        Ok(Choice::Address("0000:0a:00.1".to_owned()))
    );
    for wrong in [
        "nvidia",
        "3:00.0",
        "0000:03:00",
        "0000:03:00.0.1",
        "zz:00.0",
    ] {
        let error = Choice::parse(Some(wrong)).unwrap_err();
        assert!(error.contains("is not a PCI address"), "{wrong}: {error}");
    }
}

#[test]
fn display_controllers_are_listed_by_address() {
    let fake = machine("amdgpu");

    let gpus = display_controllers(&fake.files, Path::new(ROOT));

    assert_eq!(
        gpus,
        [
            Gpu {
                address: "0000:00:02.0".to_owned(),
                vendor: 0x8086,
                device: 0x4680
            },
            Gpu {
                address: "0000:03:00.0".to_owned(),
                vendor: 0x1002,
                device: 0x7590
            },
        ]
    );
}

#[test]
fn an_image_without_graphics_drivers_does_nothing() {
    let fake = FakeSystem::new();
    device(&fake, "0000:03:00.0", "0x030000", "0x1002", "0x7590");

    load(&fake.system(), Path::new(ROOT), Some("amd"));

    assert!(fake.runner.lines().is_empty());
    assert!(overridden(&fake).is_empty());
    assert_eq!(console_card(&fake.files, Path::new(ROOT), None), None);
}

#[test]
fn every_gpu_of_the_drivers_is_taken_without_zbm_gpu() {
    let fake = machine("amdgpu i915");
    let _log = log(&fake);
    card(&fake, "0000:00:02.0", "card0", "disconnected");
    card(&fake, "0000:03:00.0", "card1", "connected");

    load(&fake.system(), Path::new(ROOT), None);

    // The drivers load in the order of the GPUs' addresses.
    assert_eq!(
        fake.runner.lines(),
        ["modprobe i915", "modprobe amdgpu", "udevadm settle"]
    );
    assert!(overridden(&fake).is_empty());
    let kmsg = fake.kmsg();
    assert!(
        kmsg.contains("graphics: 0000:03:00.0 (1002:7590) is card1, a display connected"),
        "{kmsg}"
    );
    // The console draws on the GPU with a display.
    assert_eq!(
        console_card(&fake.files, Path::new(ROOT), None),
        Some(PathBuf::from("/dev/dri/card1"))
    );
}

#[test]
fn zbm_gpu_keeps_the_drivers_from_the_other_gpus() {
    let fake = machine("amdgpu i915 xe");
    let _log = log(&fake);
    card(&fake, "0000:00:02.0", "card0", "connected");

    load(&fake.system(), Path::new(ROOT), Some("intel"));

    assert_eq!(
        fake.runner.lines(),
        ["modprobe i915", "modprobe xe", "udevadm settle"]
    );
    assert_eq!(
        overridden(&fake),
        ["/sys/bus/pci/devices/0000:03:00.0/driver_override zfsbootmenu-none"]
    );
    assert!(
        fake.kmsg()
            .contains("leaving 0000:03:00.0 (1002:7590) alone")
    );
    assert_eq!(
        console_card(&fake.files, Path::new(ROOT), Some("intel")),
        Some(PathBuf::from("/dev/dri/card0"))
    );
}

#[test]
fn zbm_gpu_off_loads_no_driver() {
    let fake = machine("amdgpu");
    let _log = log(&fake);

    load(&fake.system(), Path::new(ROOT), Some("off"));

    assert!(fake.runner.lines().is_empty());
    assert!(overridden(&fake).is_empty());
    assert!(fake.kmsg().contains("zbm.gpu=off"));
    assert_eq!(
        console_card(&fake.files, Path::new(ROOT), Some("off")),
        None
    );
}

#[test]
fn a_gpu_zbm_gpu_does_not_find_loads_nothing() {
    let fake = machine("amdgpu");
    let _log = log(&fake);

    load(&fake.system(), Path::new(ROOT), Some("0000:05:00.0"));

    assert!(fake.runner.lines().is_empty());
    let kmsg = fake.kmsg();
    assert!(
        kmsg.contains(
            "no GPU for the graphics drivers amdgpu (zbm.gpu=0000:05:00.0; \
                       found: 0000:03:00.0 (1002:7590))"
        ),
        "{kmsg}"
    );
}

#[test]
fn a_gpu_without_a_card_is_waited_for_then_reported() {
    let fake = machine("amdgpu");
    let _log = log(&fake);
    fake.runner.respond(
        "modprobe amdgpu",
        1,
        "modprobe: ERROR: could not insert 'amdgpu'\n",
    );

    load(&fake.system(), Path::new(ROOT), Some("03:00.0"));

    let kmsg = fake.kmsg();
    assert!(
        kmsg.contains("unable to load amdgpu: modprobe: ERROR"),
        "{kmsg}"
    );
    assert!(
        kmsg.contains("no display device for 0000:03:00.0 (1002:7590) after 10 s"),
        "{kmsg}"
    );
    let waited: Duration = fake.clock.sleeps().iter().sum();
    assert_eq!(waited, Duration::from_secs(10));
    assert_eq!(
        console_card(&fake.files, Path::new(ROOT), Some("03:00.0")),
        None
    );
}

#[test]
fn the_card_of_a_virtio_gpu_is_below_its_virtio_device() {
    let fake = FakeSystem::new();
    fake.files.file(format!("/{DRIVERS}"), "virtio_gpu bochs\n");
    device(&fake, "0000:00:01.0", "0x030000", "0x1234", "0x1111");
    device(&fake, "0000:00:09.0", "0x030000", "0x1af4", "0x1050");
    fake.files
        .dir("/sys/bus/pci/devices/0000:00:09.0/virtio0/drm/card1")
        .file("/sys/class/drm/card1-Virtual-1/status", "connected\n");

    assert_eq!(
        console_card(&fake.files, Path::new(ROOT), Some("00:09.0")),
        Some(PathBuf::from("/dev/dri/card1"))
    );
    // The standard VGA, which bochs drives, has no card yet.
    assert_eq!(
        console_card(&fake.files, Path::new(ROOT), Some("00:01.0")),
        None
    );
}

#[test]
fn a_value_not_understood_takes_every_gpu() {
    let fake = machine("amdgpu");
    let _log = log(&fake);
    card(&fake, "0000:03:00.0", "card1", "connected");

    load(&fake.system(), Path::new(ROOT), Some("nvidia"));

    assert_eq!(fake.runner.lines(), ["modprobe amdgpu", "udevadm settle"]);
    assert!(fake.kmsg().contains("zbm.gpu=nvidia is not a PCI address"));
}

/// The GPU at `address` bound to `driver`, as sysfs links it.
fn bound(fake: &FakeSystem, address: &str, driver: &str) {
    let drivers = format!("/sys/bus/pci/drivers/{driver}");
    fake.files
        .dir(&drivers)
        .symlink(format!("/sys/bus/pci/devices/{address}/driver"), &drivers);
}

#[test]
fn before_kexec_the_gpus_of_the_drivers_are_unbound_and_reset() {
    let fake = machine("amdgpu i915");
    let _log = log(&fake);
    // zbm.gpu left the Intel GPU alone: it has no driver.
    bound(&fake, "0000:03:00.0", "amdgpu");
    fake.files
        .file("/sys/bus/pci/devices/0000:03:00.0/reset", "")
        .file("/sys/bus/pci/devices/0000:03:00.0/reset_method", "bus\n");
    // The network card's driver is not a graphics driver.
    bound(&fake, "0000:04:00.0", "e1000e");

    let gpus = hand_back(&fake.system(), Path::new(ROOT));

    assert_eq!(
        gpus.iter()
            .map(|gpu| gpu.address.as_str())
            .collect::<Vec<_>>(),
        ["0000:03:00.0"]
    );
    assert_eq!(
        fake.files
            .text("/sys/bus/pci/drivers/amdgpu/unbind")
            .as_deref(),
        Some("0000:03:00.0")
    );
    assert_eq!(
        fake.files
            .text("/sys/bus/pci/devices/0000:03:00.0/reset")
            .as_deref(),
        Some("1")
    );
    assert!(
        !fake
            .files
            .exists(Path::new("/sys/bus/pci/drivers/e1000e/unbind"))
    );
    let kmsg = fake.kmsg();
    assert!(
        kmsg.contains(
            "graphics: handed 0000:03:00.0 (1002:7590) back: unbound from amdgpu; reset (bus)"
        ),
        "{kmsg}"
    );
}

#[test]
fn a_gpu_without_a_reset_or_that_stays_bound_is_said_so() {
    let fake = machine("amdgpu i915");
    let _log = log(&fake);
    bound(&fake, "0000:00:02.0", "i915");
    bound(&fake, "0000:03:00.0", "amdgpu");
    fake.files.fail(
        "/sys/bus/pci/drivers/amdgpu/unbind",
        std::io::ErrorKind::PermissionDenied,
    );

    let gpus = hand_back(&fake.system(), Path::new(ROOT));

    assert_eq!(gpus.len(), 1);
    assert_eq!(gpus[0].address, "0000:00:02.0");
    let kmsg = fake.kmsg();
    assert!(
        kmsg.contains("unbound from i915; it has no reset"),
        "{kmsg}"
    );
    assert!(
        kmsg.contains("unable to unbind 0000:03:00.0 (1002:7590) from amdgpu"),
        "{kmsg}"
    );
}

#[test]
fn without_graphics_drivers_nothing_is_handed_back_or_taken() {
    let fake = FakeSystem::new();
    device(&fake, "0000:00:02.0", "0x030000", "0x8086", "0x4680");
    bound(&fake, "0000:00:02.0", "i915");

    assert!(hand_back(&fake.system(), Path::new(ROOT)).is_empty());
    take_back(&fake.system(), Path::new(ROOT), &[]);

    assert!(
        !fake
            .files
            .exists(Path::new("/sys/bus/pci/drivers/i915/unbind"))
    );
    assert!(fake.runner.lines().is_empty());
}

#[test]
fn after_a_failed_kexec_the_drivers_take_the_gpus_again() {
    let fake = machine("amdgpu");
    let _log = log(&fake);
    fake.files.dir("/sys/bus/pci");
    card(&fake, "0000:03:00.0", "card0", "connected");
    let gpu = Gpu {
        address: "0000:03:00.0".to_owned(),
        vendor: 0x1002,
        device: 0x7590,
    };

    take_back(&fake.system(), Path::new(ROOT), &[gpu]);

    assert_eq!(
        fake.files.text("/sys/bus/pci/drivers_probe").as_deref(),
        Some("0000:03:00.0")
    );
    assert_eq!(fake.runner.lines(), ["udevadm settle"]);
    assert!(fake.kmsg().contains("0000:03:00.0 (1002:7590) is card0"));
}
