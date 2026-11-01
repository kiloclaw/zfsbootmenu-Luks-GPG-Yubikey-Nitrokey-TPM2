//! The command line of `generate-zbm`, read by the parser the host tools
//! share ([`crate::options`]).

use crate::options::{self, Order, Problem, Spec};

/// The help text: the manual page embedded in the Perl program, as Pod::Usage
/// renders it.
pub const HELP: &str = include_str!("help.txt");

/// The options, with the defaults of the Perl program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    /// `--version`: the ZFSBootMenu version in image names.
    pub version: Option<String>,
    /// `--kernel`: the kernel file.
    pub kernel: Option<String>,
    /// `--kver`: the kernel version.
    pub kernel_version: Option<String>,
    /// `--prefix`: the prefix of image names.
    pub kernel_prefix: Option<String>,
    /// `--bootdir`: where kernels are looked for.
    pub bootdir: String,
    /// `--confd`: the dracut configuration directory or the mkinitcpio
    /// configuration file.
    pub confd: Option<String>,
    /// `--cmdline`: the kernel command line.
    pub cmdline: Option<String>,
    /// `--config`: the configuration file.
    pub config: String,
    /// `--enable`.
    pub enable: bool,
    /// `--disable`.
    pub disable: bool,
    /// `--initcpio` or `--no-initcpio`.
    pub initcpio: Option<bool>,
    /// `--hookd`, in order.
    pub hookd: Option<Vec<String>>,
    /// `--debug`.
    pub debug: bool,
    /// `--showver`.
    pub showver: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            version: None,
            kernel: None,
            kernel_version: None,
            kernel_prefix: None,
            bootdir: "/boot".to_owned(),
            confd: None,
            cmdline: None,
            config: "/etc/zfsbootmenu/config.yaml".to_owned(),
            enable: false,
            disable: false,
            initcpio: None,
            hookd: None,
            debug: false,
            showver: false,
        }
    }
}

/// What the command line asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Run with these options.
    Run(Box<Options>),
    /// `--help` was given: print the help and exit with status 1, as
    /// `pod2usage(-verbose => 2)` does.
    Help,
    /// An option was wrong: exit with status 1.
    Invalid,
}

/// The parsed command line and its warnings, in order, each ending with a
/// newline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parsed {
    /// What to do.
    pub outcome: Outcome,
    /// Messages for standard error.
    pub warnings: Vec<String>,
}

/// The options of `generate-zbm` (`generate-zbm(8)`), in the order of
/// [`FIELDS`].
const SPECS: &[Spec] = &[
    Spec::value(Some("version"), Some('v')),
    Spec::value(Some("kernel"), Some('k')),
    Spec::value(Some("kver"), Some('K')),
    Spec::value(Some("prefix"), Some('p')),
    Spec::value(Some("bootdir"), Some('b')),
    Spec::value(Some("confd"), Some('C')),
    Spec::value(Some("cmdline"), Some('l')),
    Spec::value(Some("config"), Some('c')),
    Spec::flag(Some("enable"), None),
    Spec::flag(Some("disable"), None),
    Spec::negatable("initcpio", Some('i')),
    Spec::value(Some("hookd"), Some('H')),
    Spec::flag(Some("debug"), Some('d')),
    Spec::flag(Some("showver"), Some('V')),
    Spec::flag(Some("help"), Some('h')),
];

/// Which field each option of [`SPECS`] sets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Field {
    Version,
    Kernel,
    KernelVersion,
    Prefix,
    Bootdir,
    Confd,
    Cmdline,
    Config,
    Enable,
    Disable,
    Initcpio,
    Hookd,
    Debug,
    Showver,
    Help,
}

/// The fields of [`SPECS`], in its order.
const FIELDS: [Field; 15] = [
    Field::Version,
    Field::Kernel,
    Field::KernelVersion,
    Field::Prefix,
    Field::Bootdir,
    Field::Confd,
    Field::Cmdline,
    Field::Config,
    Field::Enable,
    Field::Disable,
    Field::Initcpio,
    Field::Hookd,
    Field::Debug,
    Field::Showver,
    Field::Help,
];

/// The message for `problem`, as the program prints it, the option named
/// without its dashes.
fn message(problem: &Problem) -> String {
    match problem {
        Problem::Unknown(name) => format!("Unknown option: {name}\n"),
        Problem::Ambiguous(name, names) => {
            format!("Option {name} is ambiguous ({})\n", names.join(", "))
        }
        Problem::MissingValue(name) => {
            format!(
                "Option {} requires an argument\n",
                name.trim_start_matches('-')
            )
        }
        Problem::UnwantedValue(name) => {
            format!(
                "Option {} does not take an argument\n",
                name.trim_start_matches('-')
            )
        }
    }
}

/// Sets the field of `option` in `options`.
fn set(options: &mut Options, field: Field, value: Option<String>, negated: bool) {
    let text = || value.clone().unwrap_or_default();
    match field {
        Field::Version => options.version = Some(text()),
        Field::Kernel => options.kernel = Some(text()),
        Field::KernelVersion => options.kernel_version = Some(text()),
        Field::Prefix => options.kernel_prefix = Some(text()),
        Field::Bootdir => options.bootdir = text(),
        Field::Confd => options.confd = Some(text()),
        Field::Cmdline => options.cmdline = Some(text()),
        Field::Config => options.config = text(),
        Field::Hookd => options.hookd.get_or_insert_with(Vec::new).push(text()),
        Field::Enable => options.enable = true,
        Field::Disable => options.disable = true,
        Field::Initcpio => options.initcpio = Some(!negated),
        Field::Debug => options.debug = true,
        Field::Showver => options.showver = true,
        Field::Help => {}
    }
}

/// Parses the arguments ([`crate::options`]); operands are ignored.
pub fn parse(args: &[String]) -> Parsed {
    let read = options::parse("generate-zbm", SPECS, Order::Permute, args);
    let mut warnings: Vec<String> = read
        .warnings
        .iter()
        .map(|warning| format!("{warning}\n"))
        .collect();
    warnings.extend(read.problems.iter().map(message));
    if read
        .given
        .iter()
        .any(|given| FIELDS[given.index] == Field::Help)
    {
        return Parsed {
            outcome: Outcome::Help,
            warnings,
        };
    }
    if !read.problems.is_empty() {
        return Parsed {
            outcome: Outcome::Invalid,
            warnings,
        };
    }
    let mut options = Options::default();
    for given in read.given {
        set(
            &mut options,
            FIELDS[given.index],
            given.value,
            given.negated,
        );
    }
    Parsed {
        outcome: Outcome::Run(Box::new(options)),
        warnings,
    }
}

#[cfg(test)]
mod tests;
