//! The stages of a boot: the start-up stages, and the stages that run hooks.

use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};

/// Defines an enum of names with its list, `as_str`, `FromStr` and `Display`.
macro_rules! named {
    (
        $(#[$meta:meta])*
        $name:ident, $what:literal, $all:ident {
            $($(#[$variant_meta:meta])* $variant:ident = $text:literal,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub enum $name {
            $($(#[$variant_meta])* $variant,)+
        }

        impl $name {
            /// Every value, in the order a boot meets them.
            pub const $all: &[$name] = &[$($name::$variant,)+];

            /// The name.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text,)+
                }
            }
        }

        impl FromStr for $name {
            type Err = ParseError;

            fn from_str(text: &str) -> Result<$name, ParseError> {
                const NAMES: &[&str] = &[$($text,)+];
                $name::$all
                    .iter()
                    .find(|value| value.as_str() == text)
                    .copied()
                    .ok_or_else(|| ParseError::new($what, text, Reason::NotOneOf(NAMES)))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

named! {
    /// A stage of the start-up program, named after the legacy script
    /// (`init.d/<name>`) it replaces. The state directory records the stages
    /// that finished by these names.
    ///
    /// ```
    /// use zbm_types::Stage;
    ///
    /// assert_eq!(Stage::ALL.first(), Some(&Stage::Modules));
    /// assert_eq!("50-import-pools".parse::<Stage>()?, Stage::ImportPools);
    /// # Ok::<(), zbm_types::ParseError>(())
    /// ```
    Stage, "start-up stage", ALL {
        /// Loading kernel modules.
        Modules = "10-kmods",
        /// Setting the host ID.
        HostId = "20-hostid",
        /// Waiting for the devices of `zbm.wait_for`.
        DeviceWait = "30-device-wait",
        /// Running the early setup hooks.
        EarlyHooks = "40-early-hooks",
        /// Unlocking LUKS volumes.
        LuksUnlock = "45-luks-unlock",
        /// Importing pools.
        ImportPools = "50-import-pools",
    }
}

named! {
    /// A stage that runs hooks: the directory of `/libexec/hooks` the hooks
    /// are in.
    ///
    /// ```
    /// use zbm_types::HookStage;
    ///
    /// assert_eq!(HookStage::Teardown.as_str(), "teardown.d");
    /// assert_eq!(HookStage::ALL.len(), 5);
    /// ```
    HookStage, "hook stage", ALL {
        /// Before the start-up stages that import pools.
        EarlySetup = "early-setup.d",
        /// After the start-up stages, before the menu.
        Setup = "setup.d",
        /// When a boot environment's encryption key is loaded.
        LoadKey = "load-key.d",
        /// After a boot environment is chosen, before its kernel is loaded.
        BootSel = "boot-sel.d",
        /// Just before `kexec` starts the kernel.
        Teardown = "teardown.d",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_round_trips_in_order() {
        let names: Vec<&str> = Stage::ALL.iter().map(|stage| stage.as_str()).collect();
        assert_eq!(
            names,
            [
                "10-kmods",
                "20-hostid",
                "30-device-wait",
                "40-early-hooks",
                "45-luks-unlock",
                "50-import-pools"
            ]
        );
        for stage in Stage::ALL {
            assert_eq!(stage.to_string().parse::<Stage>(), Ok(*stage));
        }
        for stage in HookStage::ALL {
            assert_eq!(stage.to_string().parse::<HookStage>(), Ok(*stage));
        }
    }

    #[test]
    fn unknown_stages_are_errors() {
        assert_eq!(
            "teardown".parse::<HookStage>().unwrap_err().reason(),
            &Reason::NotOneOf(&[
                "early-setup.d",
                "setup.d",
                "load-key.d",
                "boot-sel.d",
                "teardown.d"
            ])
        );
        assert!("60-menu".parse::<Stage>().is_err());
    }
}
