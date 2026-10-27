//! The table of options: every kernel command line option, build option,
//! ZFS property, crypttab option and `generate-zbm` configuration key that
//! ZFSBootMenu reads, defined once.
//!
//! Each [`OptionSpec`] holds the names an option is read under, the values
//! it takes, its default, a summary, examples, and its entry in the manual
//! pages. From the table:
//!
//! - the parsers look options up by their specification ([`crate::args`],
//!   [`crate::crypttab`], `generate-zbm`), so an option without one cannot be
//!   read;
//! - `cargo xtask gen-docs` writes the option sections of `zfsbootmenu(7)`,
//!   `zfsbootmenu-crypttab(5)` and `generate-zbm(5)` ([`crate::docs`]);
//! - [`crate::Config`] reports each value with the option it belongs to and
//!   where it came from.
//!
//! ```
//! use zbm_config::spec::{self, Source};
//!
//! let timeout = spec::find(Source::KernelCommandLine, "timeout").unwrap();
//! assert_eq!(timeout.name, "zbm.timeout");
//! assert!(timeout.alias("timeout").unwrap().deprecation.is_some());
//! ```

pub mod build;
pub mod cmdline;
pub mod crypttab;
pub mod generate;
pub mod properties;

/// Where an option is set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// The kernel command line of ZFSBootMenu.
    KernelCommandLine,
    /// The dracut or mkinitcpio configuration that builds the image.
    BuildOption,
    /// A property of a pool.
    PoolProperty,
    /// A property of a dataset, inherited along the boot environment tree.
    DatasetProperty,
    /// The options field of a crypttab entry.
    Crypttab,
    /// A key of a section of `/etc/zfsbootmenu/config.yaml`.
    GenerateZbm(GenerateSection),
}

/// The sections of `/etc/zfsbootmenu/config.yaml`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerateSection {
    /// `Global`.
    Global,
    /// `Kernel`.
    Kernel,
    /// `Components`.
    Components,
    /// `EFI`.
    Efi,
}

impl GenerateSection {
    /// The name of the section in the file.
    pub fn as_str(self) -> &'static str {
        match self {
            GenerateSection::Global => "Global",
            GenerateSection::Kernel => "Kernel",
            GenerateSection::Components => "Components",
            GenerateSection::Efi => "EFI",
        }
    }
}

/// The values an option takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Value {
    /// None: the name alone sets the option.
    Flag,
    /// On or off. On the command line, an empty value, `no`, `off` and `0`
    /// mean off, and anything else on.
    Bool,
    /// A whole number.
    Integer,
    /// One of a fixed list of words.
    Choice(&'static [&'static str]),
    /// Either no value, or one of a fixed list of words.
    FlagOrChoice(&'static [&'static str]),
    /// Text: a path, a device, a list, a command line.
    Text,
}

impl Value {
    /// Whether `value` is one this option documents. A flag takes no value
    /// (`None`); every other option takes one.
    pub fn allows(self, value: Option<&str>) -> bool {
        match (self, value) {
            (Value::Flag | Value::FlagOrChoice(_), None) | (Value::Bool | Value::Text, Some(_)) => {
                true
            }
            (Value::Integer, Some(value)) => {
                let digits = value.strip_prefix(['-', '+']).unwrap_or(value);
                !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            }
            (Value::Choice(words) | Value::FlagOrChoice(words), Some(value)) => {
                words.contains(&value)
            }
            _ => false,
        }
    }
}

/// What reads an option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reader {
    /// The Rust code, through this table.
    Rust,
    /// A shell script of the image or of its build, named here, until R6 of
    /// `ROADMAP.md` replaces it.
    Shell(&'static str),
    /// Something other than ZFSBootMenu, such as ZFS itself.
    Other(&'static str),
}

/// How a deprecated option or name is documented.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Deprecation {
    /// The heading of its entry, in reStructuredText.
    pub heading: &'static str,
    /// What to do instead, completing "Deprecated; ...".
    pub instead: &'static str,
}

/// Another name an option is read under.
///
/// On the kernel command line, reading an option by an alias logs a warning
/// naming the option, as the legacy runtime does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Alias {
    /// The name.
    pub name: &'static str,
    /// Its entry among the deprecated options, if it has one.
    pub deprecation: Option<Deprecation>,
}

/// Where an option is documented.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Doc {
    /// An entry of its own in a manual page.
    Entry {
        /// The heading, in reStructuredText.
        heading: &'static str,
        /// The text under the heading, in reStructuredText, without its
        /// indentation.
        body: &'static str,
    },
    /// In the entry of the option named here.
    Within(&'static str),
    /// Not in ZFSBootMenu's manual pages, for the reason given.
    NotInManual(&'static str),
}

/// The specification of one option.
#[derive(Debug)]
pub struct OptionSpec {
    /// The name the option is read under first.
    pub name: &'static str,
    /// Other names it is read under.
    pub aliases: &'static [Alias],
    /// Where it is set.
    pub source: Source,
    /// The values it takes.
    pub value: Value,
    /// Its value when it is not set, if it has one, as text.
    pub default: Option<&'static str>,
    /// One line about it.
    pub summary: &'static str,
    /// What reads it.
    pub reader: Reader,
    /// Where it is documented.
    pub doc: Doc,
    /// When the option is deprecated, its entry among the deprecated options.
    pub deprecation: Option<Deprecation>,
    /// Ways of setting it, as written where it is set (`zbm.timeout=5`,
    /// `tries=5`, `ManageImages: true`).
    pub examples: &'static [&'static str],
}

impl OptionSpec {
    /// Every name the option is read under: its name, then its aliases.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        std::iter::once(self.name).chain(self.aliases.iter().map(|alias| alias.name))
    }

    /// The alias `name`, if the option has one.
    pub fn alias(&self, name: &str) -> Option<&Alias> {
        self.aliases.iter().find(|alias| alias.name == name)
    }

    /// Whether the option is read under `name`.
    pub fn is_named(&self, name: &str) -> bool {
        self.names().any(|known| known == name)
    }
}

/// Two specifications are the same option when they have the same source and
/// name, which [`find`] makes unique.
impl PartialEq for OptionSpec {
    fn eq(&self, other: &OptionSpec) -> bool {
        self.source == other.source && self.name == other.name
    }
}

impl Eq for OptionSpec {}

/// Every option, by source, in the order of the manual pages.
pub fn all() -> impl Iterator<Item = &'static OptionSpec> {
    [
        cmdline::ALL,
        cmdline::luks::ALL,
        properties::ALL,
        build::ALL,
        crypttab::ALL,
        generate::ALL,
    ]
    .into_iter()
    .flatten()
    .copied()
}

/// The option of `source` read under `name`, by its name or an alias.
pub fn find(source: Source, name: &str) -> Option<&'static OptionSpec> {
    all().find(|spec| spec.source == source && spec.is_named(name))
}

#[cfg(test)]
mod tests;
