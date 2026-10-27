//! Layouts per keyboard (`zbm.keyboard_layouts`): a keyboard known by its
//! USB vendor and product types with a layout of its own, and keeps its
//! own key state and lights; the others share the console's layout.

use std::path::Path;

use super::{Decoder, Keyboards, Source};

/// A keyboard's USB vendor and product, as `lsusb` shows them.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DeviceId {
    /// The vendor.
    pub vendor: u16,
    /// The product.
    pub product: u16,
}

impl DeviceId {
    /// The id `text` writes as `vendor:product` in hexadecimal: `046d:c31c`.
    pub fn parse(text: &str) -> Option<DeviceId> {
        let (vendor, product) = text.split_once(':')?;
        let hex = |part: &str| {
            (part.len() == 4)
                .then(|| u16::from_str_radix(part, 16).ok())
                .flatten()
        };
        Some(DeviceId {
            vendor: hex(vendor)?,
            product: hex(product)?,
        })
    }

    /// The id of the input device at `device` (`/dev/input/event3`), as
    /// `sysfs` tells it (`class/input/event3/device/id/vendor`).
    pub(super) fn of(sysfs: &Path, device: &Path) -> Option<DeviceId> {
        let name = device.file_name()?;
        let id = sysfs.join("class/input").join(name).join("device/id");
        let read = |field: &str| {
            let text = std::fs::read_to_string(id.join(field)).ok()?;
            u16::from_str_radix(text.trim(), 16).ok()
        };
        Some(DeviceId {
            vendor: read("vendor")?,
            product: read("product")?,
        })
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04x}:{:04x}", self.vendor, self.product)
    }
}

impl Keyboards {
    /// Gives the keyboards of `rules` their layouts, those open now and
    /// those that come later.
    pub fn set_device_layouts(&mut self, rules: Vec<(DeviceId, Source)>) {
        self.rules = rules;
        for index in 0..self.devices.len() {
            self.adopt(index);
        }
        self.show_locks();
    }

    /// Gives the keyboard at `index` its own decoder, with its layout, when
    /// a rule names it and the layout loads.
    pub(super) fn adopt(&mut self, index: usize) {
        let Some(device) = self.devices.get_mut(index) else {
            return;
        };
        let source = self
            .rules
            .iter()
            .find(|(id, _)| Some(*id) == device.id)
            .map(|(_, source)| source);
        let Some(layout) = source.and_then(|source| source.layout().ok()) else {
            return;
        };
        let mut own = Decoder::new();
        own.set_compose_key(self.compose_key);
        own.set_num_lock(self.decoder.is_num_lock());
        own.set_layout(Some(layout));
        device.own = Some(own);
        device.lights = None;
    }

    /// The decoder of the keyboard at `index`: its own, or the shared one.
    pub(super) fn decoder_of(&mut self, index: usize) -> &mut Decoder {
        match self
            .devices
            .get_mut(index)
            .and_then(|device| device.own.as_mut())
        {
            Some(own) => own,
            None => &mut self.decoder,
        }
    }

    /// The keyboards that have a layout of their own, and its name.
    pub fn device_layouts(&self) -> Vec<(DeviceId, String)> {
        self.devices
            .iter()
            .filter_map(|device| Some((device.id?, device.own.as_ref()?.layout().to_owned())))
            .collect()
    }
}
