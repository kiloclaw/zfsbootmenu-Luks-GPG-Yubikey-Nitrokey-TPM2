//! Helpers for the tests of this crate.

use crate::import::{ImportSettings, Imported};
use zbm_app::model::Event;
use zbm_boot::Recovery;
use zbm_platform::System;
use zbm_state::State;
use zbm_tools::console::Answer;

use crate::front::Front;
use crate::step::NoRecovery;

/// A front end that records the stages' events and what they asked of it;
/// it answers as [`NoRecovery`] does, and hands the console over at once.
#[derive(Default)]
pub struct Recording {
    /// The events, in order.
    pub events: Vec<Event>,
    /// What the stages asked, in order: `recovery`, `unlock`, `devices 5`,
    /// `console`, `import`.
    pub asked: Vec<String>,
    recovery: NoRecovery,
}

impl Front for Recording {
    fn send(&mut self, event: Event) {
        self.events.push(event);
    }

    fn recovery(&mut self) -> &dyn Recovery {
        self.asked.push("recovery".to_owned());
        &mut self.recovery
    }

    fn unlock_luks(&mut self, system: &System<'_>) {
        self.asked.push("unlock".to_owned());
        self.recovery.unlock_luks(system);
    }

    fn devices_missing(&mut self, system: &System<'_>, delay: i64) -> Answer {
        self.asked.push(format!("devices {delay}"));
        self.recovery.devices_missing(system, delay)
    }

    fn with_console(&mut self, system: &System<'_>, run: &mut dyn FnMut(&System<'_>)) {
        self.asked.push("console".to_owned());
        run(system);
    }

    fn import(
        &mut self,
        system: &System<'_>,
        state: &State<'_>,
        settings: &ImportSettings,
    ) -> Imported {
        self.asked.push("import".to_owned());
        self.recovery.import(system, state, settings)
    }
}

/// The lease that mkinitcpio's `net` hook left in `/tmp/net-eth0.conf` on
/// QEMU's user network, as lane 1 of SB17 found it on a real image.
pub const QEMU_LEASE: &str = "\
DEVICE='eth0'
IPV4ADDR='10.0.2.15'
IPV4BROADCAST='10.0.2.255'
IPV4NETMASK='255.255.255.0'
IPV4GATEWAY='10.0.2.2'
IPV4DNS0='10.0.2.3'
IPV4DNS1='0.0.0.0'
HOSTNAME=''
DNSDOMAIN=''
NISDOMAIN=''
ROOTSERVER='10.0.2.2'
ROOTPATH=''
filename=''
";
