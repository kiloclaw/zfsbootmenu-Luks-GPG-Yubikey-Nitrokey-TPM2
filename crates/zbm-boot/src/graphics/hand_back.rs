//! The GPUs handed back before `kexec` (ROADMAP.md #17): a GPU that one
//! kernel set up rarely starts again under the next, which finds it as the
//! last driver left it. Each GPU the image's graphics drivers took is
//! unbound from its driver, which runs the driver's removal, and its PCI
//! function reset where the device has a reset, so that the booted kernel
//! finds it as at power-on. If `kexec` fails, the drivers take the GPUs
//! again, for the console to show the error.

use std::path::Path;

use zbm_platform::{Files, System};
use zbm_tools::{Shown, udev};

use super::{Gpu, PCI, display_controllers, drivers_of, image_drivers, wait_for_cards};

/// The file that has the PCI bus look for a driver for a device.
const PROBE: &str = "sys/bus/pci/drivers_probe";

/// Hands back each GPU below `root` that the image's graphics drivers
/// drive: unbinds it from its driver and resets it where it can be reset.
/// Returns the GPUs unbound. Does nothing in an image without graphics
/// drivers; what goes wrong is logged, and the boot goes on.
pub fn hand_back(system: &System<'_>, root: &Path) -> Vec<Gpu> {
    let files = system.files;
    let drivers = image_drivers(files, root);
    if drivers.is_empty() {
        return Vec::new();
    }
    display_controllers(files, root)
        .into_iter()
        .filter(|gpu| !drivers_of(&drivers, gpu.vendor).is_empty())
        .filter_map(|gpu| {
            let driver = unbind(files, root, &gpu)?;
            reset(files, root, &gpu, &driver);
            Some(gpu)
        })
        .collect()
}

/// Unbinds `gpu` from its driver; the driver's name, or `None` when it had
/// none or could not be unbound.
fn unbind(files: &dyn Files, root: &Path, gpu: &Gpu) -> Option<String> {
    let device = root.join(PCI).join(&gpu.address);
    // `zbm.gpu` left it alone, or its driver did not take it.
    let driver = files.canonicalize(&device.join("driver")).ok()?;
    let name = driver.file_name()?.to_string_lossy().into_owned();
    match files.write(&driver.join("unbind"), gpu.address.as_bytes()) {
        Ok(()) => Some(name),
        Err(error) => {
            zbm_trace::warn!(
                "graphics: unable to unbind {gpu} from {name}: {error}";
                gpu = %gpu.describe(), %name, %error
            );
            None
        }
    }
}

/// Resets `gpu`, unbound from `driver`, with the methods the kernel offers
/// for it (`reset_method`), and says what became of it.
fn reset(files: &dyn Files, root: &Path, gpu: &Gpu, driver: &str) {
    let device = root.join(PCI).join(&gpu.address);
    let methods = files
        .read_to_string(&device.join("reset_method"))
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();
    let reset = device.join("reset");
    let how = if files.exists(&reset) {
        match files.write(&reset, b"1") {
            Ok(()) if methods.is_empty() => "reset".to_owned(),
            Ok(()) => format!("reset ({methods})"),
            Err(error) => format!("unable to reset it: {error}"),
        }
    } else {
        "it has no reset".to_owned()
    };
    zbm_trace::info!(
        "graphics: handed {gpu} back: unbound from {driver}; {how}";
        gpu = %gpu.describe(), %driver, %how
    );
}

/// After a failed `kexec`: has the PCI bus find the drivers of `gpus`
/// again, and waits for their displays, for the console to take one.
pub fn take_back(system: &System<'_>, root: &Path, gpus: &[Gpu]) {
    if gpus.is_empty() {
        return;
    }
    for gpu in gpus {
        if let Err(error) = system
            .files
            .write(&root.join(PROBE), gpu.address.as_bytes())
        {
            zbm_trace::warn!(
                "graphics: unable to have a driver take {gpu} again: {error}";
                gpu = %gpu.describe(), %error
            );
        }
    }
    wait_for_cards(system, root, gpus);
    udev::settle(system.runner, Shown::Hidden);
}
