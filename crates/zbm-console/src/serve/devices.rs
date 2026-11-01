//! How the console opens its devices, and the kernel log it was given
//! (ROADMAP.md SE11, ADR 0012).

use std::fs::File;
use std::sync::Arc;

use zbm_display::devices::Opener;

/// How the console opens its devices (ADR 0012): itself, or through the
/// supervisor's broker; and the kernel log, which its user may not open.
#[derive(Clone)]
pub struct Devices {
    /// What opens the devices.
    pub opener: Arc<dyn Opener>,
    /// The kernel log, open for writing, when the console was given it.
    pub kmsg: Option<Arc<File>>,
}

impl Devices {
    /// The devices of a console that opens them itself.
    pub fn direct() -> Devices {
        Devices {
            opener: zbm_display::devices::direct(),
            kmsg: None,
        }
    }
}

impl std::fmt::Debug for Devices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Devices")
            .field("opener", &self.opener)
            .field("kmsg", &self.kmsg.is_some())
            .finish()
    }
}

impl PartialEq for Devices {
    fn eq(&self, other: &Devices) -> bool {
        let kmsg = match (&self.kmsg, &other.kmsg) {
            (Some(one), Some(other)) => Arc::ptr_eq(one, other),
            (None, None) => true,
            _ => false,
        };
        Arc::ptr_eq(&self.opener, &other.opener) && kmsg
    }
}

impl Eq for Devices {}
