//! The kernel's table of mounts, read back: each line of
//! [`MOUNTINFO`] as a [`Listed`] mount, with the kernel's octal escapes
//! undone, so that a mount point is compared as the bytes it was mounted
//! on ([`mounts`]). The table's text comes from names that whoever made
//! a pool chooses, so an odd line is skipped, never a panic. Nothing here
//! reads a file or runs a program.

/// The kernel's table of this process's mounts.
pub const MOUNTINFO: &str = "/proc/self/mountinfo";

/// A line of the kernel's table of mounts, with the kernel's octal escapes
/// read back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listed {
    /// The mount ID, its first field, which the kernel gives to another
    /// mount once this one is gone.
    pub id: u64,
    /// The directory of the filesystem that is mounted, its fourth field.
    pub root: Vec<u8>,
    /// The mount point, its fifth field.
    pub point: Vec<u8>,
    /// The filesystem's type, the first field after `-`; empty where the
    /// line has none.
    pub kind: Vec<u8>,
    /// The filesystem's source, the second field after `-`, the dataset of
    /// a ZFS mount; empty where the line has none.
    pub source: Vec<u8>,
}

impl Listed {
    /// Whether this is another mount of the same directory of the same
    /// filesystem as `other`.
    #[must_use]
    pub fn is_mounted_as(&self, other: &Listed) -> bool {
        self.id != other.id
            && (&self.root, &self.kind, &self.source) == (&other.root, &other.kind, &other.source)
    }
}

/// The mounts of `table`, the kernel's mountinfo. Fields are split at
/// spaces alone, as the kernel writes them, so that any other byte of a
/// field stays in it. A line without a mount point is skipped.
pub fn mounts(table: &[u8]) -> impl Iterator<Item = Listed> + '_ {
    table.split(|&byte| byte == b'\n').filter_map(|line| {
        let mut fields = line
            .split(|&byte| byte == b' ')
            .filter(|field| !field.is_empty());
        let id = std::str::from_utf8(fields.next()?).ok()?.parse().ok()?;
        let root = fields.nth(2)?;
        let point = fields.next()?;
        let mut described = fields.skip_while(|field| *field != b"-").skip(1);
        Some(Listed {
            id,
            root: unescaped(root),
            point: unescaped(point),
            kind: described.next().map(unescaped).unwrap_or_default(),
            source: described.next().map(unescaped).unwrap_or_default(),
        })
    })
}

/// `field` with each `\` and three octal digits, as the kernel writes a
/// space (`\040`), a tab, a line feed or a backslash, read back as its
/// byte; any other `\` is kept.
fn unescaped(field: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(field.len());
    let mut rest = field;
    while let Some((&byte, after)) = rest.split_first() {
        if let Some(escaped) = octal(after).filter(|_| byte == b'\\') {
            bytes.push(escaped);
            rest = after.get(3..).unwrap_or_default();
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    bytes
}

/// The byte that the first three of `digits` write in octal, if they are
/// octal digits and write one.
fn octal(digits: &[u8]) -> Option<u8> {
    digits.get(..3)?.iter().try_fold(0_u8, |value, &digit| {
        let digit = u8::try_from(char::from(digit).to_digit(8)?).ok()?;
        value.checked_mul(8)?.checked_add(digit)
    })
}

#[cfg(test)]
mod tests;
