use std::path::Path;

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

    let gpus = hand_back(&fake.context(), Path::new(ROOT));

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

    let gpus = hand_back(&fake.context(), Path::new(ROOT));

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

    assert!(hand_back(&fake.context(), Path::new(ROOT)).is_empty());
    take_back(&fake.context(), Path::new(ROOT), &[]);

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

    take_back(&fake.context(), Path::new(ROOT), &[gpu]);

    assert_eq!(
        fake.files.text("/sys/bus/pci/drivers_probe").as_deref(),
        Some("0000:03:00.0")
    );
    assert_eq!(fake.runner.lines(), ["udevadm settle"]);
    assert!(fake.kmsg().contains("0000:03:00.0 (1002:7590) is card0"));
}
