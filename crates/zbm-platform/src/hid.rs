//! HID devices' feature reports (`hidraw`), as a YubiKey's OTP interface
//! speaks them (ROADMAP.md SB11): eight bytes each way, report 0.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// The length of a report.
pub const REPORT: usize = zbm_sys::hidraw::REPORT;

/// One feature report, wiped when it goes, as it may hold part of a
/// challenge or a response.
#[derive(Default)]
pub struct Report(pub [u8; REPORT]);

impl Drop for Report {
    fn drop(&mut self) {
        zbm_sys::wipe(&mut self.0);
    }
}

/// Opening HID devices for their feature reports.
pub trait HidDevices: Send + Sync {
    /// Opens the `hidraw` device `path`.
    ///
    /// # Errors
    ///
    /// When it cannot be opened for reading and writing.
    fn open(&self, path: &Path) -> io::Result<Box<dyn FeatureReports>>;
}

/// The feature reports of one open device.
pub trait FeatureReports {
    /// Reads the device's report into `report`.
    ///
    /// # Errors
    ///
    /// When the device gives none, as when it went away.
    fn get(&mut self, report: &mut Report) -> io::Result<()>;

    /// Sends `report` to the device.
    ///
    /// # Errors
    ///
    /// When the device refuses it.
    fn set(&mut self, report: &Report) -> io::Result<()>;
}

/// The kernel's `hidraw` devices.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemHid;

impl HidDevices for SystemHid {
    #[allow(
        clippy::disallowed_methods,
        reason = "the device is kept open for its requests, which Files has none of"
    )]
    fn open(&self, path: &Path) -> io::Result<Box<dyn FeatureReports>> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Box::new(HidrawFile(file)))
    }
}

/// An open `hidraw` device.
struct HidrawFile(File);

impl FeatureReports for HidrawFile {
    fn get(&mut self, report: &mut Report) -> io::Result<()> {
        zbm_sys::hidraw::get_feature(&self.0, &mut report.0)
    }

    fn set(&mut self, report: &Report) -> io::Result<()> {
        zbm_sys::hidraw::set_feature(&self.0, &report.0)
    }
}

/// No HID devices, for systems that reach none.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoHid;

impl HidDevices for NoHid {
    fn open(&self, _path: &Path) -> io::Result<Box<dyn FeatureReports>> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_no_hid_device_opens_but_gives_no_report() {
        let mut device = SystemHid.open(Path::new("/dev/null")).unwrap();
        let mut report = Report([0xff; REPORT]);
        assert!(device.get(&mut report).is_err());
        assert!(device.set(&Report::default()).is_err());
        assert!(SystemHid.open(Path::new("/nonexistent/hidraw0")).is_err());
        assert!(NoHid.open(Path::new("/dev/hidraw0")).is_err());
    }
}
