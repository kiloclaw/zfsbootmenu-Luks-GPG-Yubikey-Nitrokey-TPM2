//! The build option of the locked menu (ROADMAP.md SE9).

use super::super::{Doc, OptionSpec, Reader, Source, Value};

/// `zfsbootmenu_lock_hash`: the hash of the PIN of the locked menu
/// (ROADMAP.md SE9).
pub static LOCK_HASH: OptionSpec = OptionSpec {
    name: "zfsbootmenu_lock_hash",
    aliases: &[],
    source: Source::BuildOption,
    value: Value::Text,
    default: None,
    summary: "The Argon2id hash of the PIN that locks the menu",
    reader: Reader::Shell("the dracut module and the mkinitcpio hook"),
    doc: Doc::Entry {
        heading: "**zfsbootmenu_lock_hash='$argon2id$v=19$m=...'**",
        body: LOCK_HASH_DOC,
    },
    deprecation: None,
    examples: &["zfsbootmenu_lock_hash='$argon2id$v=19$m=65536,t=3,p=1$...'"],
};

const LOCK_HASH_DOC: &str = r"
In images with the *rust* runtime, locks the menu with a PIN: everything but the default boot then
needs the PIN, or a passphrase that opened a LUKS volume during this boot. Locked: the recovery
shell, however it opens, editing the command line, booting another boot environment, kernel or
snapshot, and every change (snapshots taken or destroyed, rollbacks, clones, the default,
read/write and forced imports, the checkpoint). The countdown and the default boot, looking around,
unlocking a volume with its own passphrase, rebooting and powering off need nothing. The PIN typed
lasts until the menu has been idle for five minutes, or the recovery shell ends; each wrong one
doubles the wait before the next answer, from one second to thirty. A locked image ignores
**zbm.hookroot**. **zbm lock hash** asks for the PIN twice, of 8 characters at least, and prints
the line to add: the hash lies on the EFI system partition, where anyone can read it and guess PINs
offline, so a passphrase of a few words is better. The lock only holds when the image cannot be
changed, nor its command line: boot it as a signed unified kernel image under Secure Boot. Without
lockdown or Secure Boot, the image warns at start-up that a changed command line gets past the
lock. An image with the *legacy* runtime fails to build with this option set.
";
