//! `zbm.shadow=efivar`: the summary of a boot with differences, kept in an
//! EFI variable so that it survives `kexec`, for `zbm shadow report` on the
//! booted system.

use std::io;
use std::path::{Path, PathBuf};

use zbm_state::State;
use zbm_system::Files;

/// Where the kernel shows EFI variables.
pub const EFIVARS: &str = "/sys/firmware/efi/efivars";

/// The variable's name and the vendor GUID of ZFSBootMenu's variables.
pub const VARIABLE: &str = "ZbmShadowSummary-5a2f3c1e-9b4d-4e7a-8c61-2d0f9b3e7a15";

/// `EFI_VARIABLE_NON_VOLATILE | BOOTSERVICE_ACCESS | RUNTIME_ACCESS`, as the
/// four bytes `efivarfs` expects before the data.
const ATTRIBUTES: [u8; 4] = [7, 0, 0, 0];

/// The marker in the state directory that the variable was written this boot.
const MARKER: &str = "shadow-efivar";

/// The variable's file below `efivars`.
pub fn path(efivars: &Path) -> PathBuf {
    efivars.join(VARIABLE)
}

/// Writes `text` to the variable below `efivars`, at most once per boot.
/// Returns whether it wrote it.
///
/// # Errors
///
/// When `efivarfs` cannot be written, for example because it is mounted
/// read-only.
pub fn record(
    files: &dyn Files,
    state: &State<'_>,
    efivars: &Path,
    text: &str,
) -> io::Result<bool> {
    let marker = state.base().join(MARKER);
    if files.exists(&marker) {
        return Ok(false);
    }
    let mut bytes = ATTRIBUTES.to_vec();
    bytes.extend_from_slice(text.as_bytes());
    files.write(&path(efivars), &bytes)?;
    files.write(&marker, b"")?;
    Ok(true)
}

/// The text of the variable below `efivars`, which is then removed; `None`
/// when there is none.
///
/// # Errors
///
/// When the variable cannot be removed (`efivarfs` makes variables
/// immutable: `chattr -i` the file first).
pub fn take(files: &dyn Files, efivars: &Path) -> io::Result<Option<String>> {
    let file = path(efivars);
    let Ok(bytes) = files.read(&file) else {
        return Ok(None);
    };
    let text =
        String::from_utf8_lossy(bytes.get(ATTRIBUTES.len()..).unwrap_or_default()).into_owned();
    files.remove_file(&file)?;
    Ok(Some(text))
}
