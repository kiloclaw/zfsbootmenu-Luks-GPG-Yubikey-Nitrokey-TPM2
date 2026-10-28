//! What unlocking needs beyond the volumes: why a TPM2 key was kept, and the
//! keyboard layouts a secret can be typed with
//! (`docs/dev/menu-design/unlocking.md`).

/// A TPM2 key of a volume, sealed to some PCRs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tpm2Seal {
    /// The PCRs the key is sealed to.
    pub pcrs: Vec<u8>,
    /// The PCRs that changed since: while any did, the TPM keeps the key.
    pub changed: Vec<PcrChange>,
}

impl Tpm2Seal {
    /// Whether the TPM releases the key: no PCR it is sealed to changed.
    pub fn is_current(&self) -> bool {
        self.changed.is_empty()
    }

    /// The PCRs the key is sealed to, for people: `0 and 7`.
    pub fn pcr_list(&self) -> String {
        let names: Vec<String> = self.pcrs.iter().map(u8::to_string).collect();
        match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            Some((last, _)) => last.clone(),
            None => "none".to_owned(),
        }
    }
}

/// A PCR that changed since a key was sealed to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcrChange {
    /// The PCR.
    pub index: u8,
    /// What the event log says changed, when the values at sealing were
    /// recorded: `the firmware was updated to 1.21`.
    pub detail: Option<String>,
}

impl PcrChange {
    /// The change, for people: `PCR 0 (firmware code): the firmware was
    /// updated to 1.21`.
    pub fn describe(&self) -> String {
        let pcr = format!("PCR {} ({})", self.index, pcr_name(self.index));
        match &self.detail {
            Some(detail) => format!("{pcr}: {detail}"),
            None => pcr,
        }
    }
}

/// What a PCR measures on a UEFI machine, as the TCG PC Client profile and
/// systemd use them.
pub fn pcr_name(index: u8) -> &'static str {
    match index {
        0 => "firmware code",
        1 => "firmware settings",
        2 => "option ROMs",
        3 => "option ROM settings",
        4 => "boot loader",
        5 => "partition table",
        7 => "Secure Boot state",
        9 => "initramfs",
        11 => "unified kernel image",
        12 => "kernel command line",
        14 => "shim certificates",
        15 => "system identity",
        _ => "other measurements",
    }
}

/// Why the unlock dialog asks what it asks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    /// The last secret typed was wrong.
    Wrong(String),
    /// Another method failed, and this one is asked instead.
    Instead(String),
}

impl Notice {
    /// What the dialog says.
    pub fn text(&self) -> &str {
        match self {
            Notice::Wrong(text) | Notice::Instead(text) => text,
        }
    }

    /// Whether the last secret typed was wrong.
    pub fn is_wrong_secret(&self) -> bool {
        matches!(self, Notice::Wrong(_))
    }
}

/// The keyboard layouts a secret can be typed with.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Keyboard {
    /// The layouts, the console's own first: `us`, then those the menu can
    /// translate into.
    pub layouts: Vec<String>,
    /// Whether keys come from the machine's own keyboard. Over a serial
    /// console the terminal sends characters in its own layout, and the menu
    /// does not translate them.
    pub local: bool,
    /// The characters of each layout by key position, from the image's
    /// keymaps, which the menu translates what is typed with; a layout
    /// whose keymap does not load has none, and is not translated.
    pub positions: std::collections::BTreeMap<String, zbm_display::layout::Positions>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seal_says_whether_the_tpm_releases_the_key_and_why_not() {
        let changed = PcrChange {
            index: 0,
            detail: Some("the firmware was updated to 1.21".into()),
        };
        let mut seal = Tpm2Seal {
            pcrs: vec![0, 2, 7],
            changed: vec![changed.clone()],
        };

        assert!(!seal.is_current());
        assert_eq!(seal.pcr_list(), "0, 2 and 7");
        assert_eq!(
            changed.describe(),
            "PCR 0 (firmware code): the firmware was updated to 1.21"
        );
        let unknown = PcrChange {
            index: 23,
            detail: None,
        };
        assert_eq!(unknown.describe(), "PCR 23 (other measurements)");
        seal.changed.clear();
        seal.pcrs = vec![7];
        assert!(seal.is_current());
        assert_eq!(seal.pcr_list(), "7");
        seal.pcrs.clear();
        assert_eq!(seal.pcr_list(), "none");
        let wrong = Notice::Wrong("wrong passphrase".into());
        assert!(wrong.is_wrong_secret() && wrong.text() == "wrong passphrase");
        assert!(!Notice::Instead("the TPM kept the key".into()).is_wrong_secret());
    }
}
