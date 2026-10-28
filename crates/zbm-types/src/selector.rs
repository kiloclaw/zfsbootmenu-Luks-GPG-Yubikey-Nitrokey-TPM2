//! The kernel a boot environment boots: `org.zfsbootmenu:kernel`.

use std::fmt;
use std::str::FromStr;

use regex::bytes::{Regex, RegexBuilder};

use crate::ere;
use crate::error::{ParseError, Reason};

/// The value of `org.zfsbootmenu:kernel`: a POSIX extended regular
/// expression that the paths of a boot environment's kernels are matched
/// against (`/boot/vmlinuz-6.12.9_1`), and the last kernel it matches is
/// the one to boot.
///
/// It means what it meant to the legacy code, which matched it with Bash's
/// `[[ path =~ value ]]` in the C locale (see `ere.rs` for the translation),
/// but it is compiled once, in the process, instead of running a shell per
/// kernel. A value that is not a valid pattern does not parse; one with a
/// back-reference neither, as the `regex` crate has none.
///
/// ```
/// use zbm_types::KernelSelector;
///
/// let selector: KernelSelector = "6\\.6".parse()?;
/// assert!(selector.matches("/boot/vmlinuz-6.6.30"));
/// assert!(!selector.matches("/boot/vmlinuz-6.12.9"));
/// assert!("(".parse::<KernelSelector>().is_err());
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug)]
pub struct KernelSelector {
    /// The value, as it was set.
    pattern: String,
    /// It, compiled.
    regex: Regex,
}

impl KernelSelector {
    /// Whether the kernel at `path` is one the selector names.
    pub fn matches(&self, path: &str) -> bool {
        self.regex.is_match(path.as_bytes())
    }

    /// The value, as it was set.
    pub fn as_str(&self) -> &str {
        &self.pattern
    }
}

impl FromStr for KernelSelector {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<KernelSelector, ParseError> {
        let invalid = |why| ParseError::new("kernel selector", text, Reason::Pattern(why));
        let translated = ere::translate(text.as_bytes()).map_err(invalid)?;
        let regex = RegexBuilder::new(&translated)
            .unicode(false)
            .nest_limit(u32::try_from(ere::NEST_MAX).unwrap_or(u32::MAX))
            .build()
            .map_err(|_| invalid("it is too large to compile"))?;
        Ok(KernelSelector {
            pattern: text.to_owned(),
            regex,
        })
    }
}

impl fmt::Display for KernelSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.pattern)
    }
}

impl PartialEq for KernelSelector {
    fn eq(&self, other: &KernelSelector) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for KernelSelector {}
