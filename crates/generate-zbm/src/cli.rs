//! The command line, parsed as Perl's Getopt::Long 2.54 parses it with the
//! option table of `generate-zbm`.
//!
//! The Perl program configures Getopt::Long with `no_ignore_case` and
//! `auto_version` and otherwise keeps its defaults: options may start with
//! `--`, `-` or `+`, long names may be shortened to any unique prefix,
//! `--opt=value` and `-opt=value` both work, a string option takes the next
//! argument whatever it looks like, and arguments that are not options may
//! appear anywhere. When `POSIXLY_CORRECT` is set, Getopt::Long instead accepts
//! only `--` and `-`, does not shorten names, splits `=value` only after `--`,
//! and stops at the first argument that is not an option.

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

/// The parsed command line and Getopt::Long's warnings, in order, each ending
/// with a newline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parsed {
    /// What to do.
    pub outcome: Outcome,
    /// Messages for standard error.
    pub warnings: Vec<String>,
}

/// Which field an option sets.
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

/// How an option takes its value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    /// A switch: `name`.
    Flag,
    /// A switch with negations: `name!`.
    Negatable,
    /// A string: `name=s`.
    Text,
    /// A repeatable string: `name=s@`.
    TextList,
}

/// The option table of `generate-zbm`: names (the first is the canonical one),
/// kind and field.
const TABLE: &[(&[&str], Kind, Field)] = &[
    (&["version", "v"], Kind::Text, Field::Version),
    (&["kernel", "k"], Kind::Text, Field::Kernel),
    (&["kver", "K"], Kind::Text, Field::KernelVersion),
    (&["prefix", "p"], Kind::Text, Field::Prefix),
    (&["bootdir", "b"], Kind::Text, Field::Bootdir),
    (&["confd", "C"], Kind::Text, Field::Confd),
    (&["cmdline", "l"], Kind::Text, Field::Cmdline),
    (&["config", "c"], Kind::Text, Field::Config),
    (&["enable"], Kind::Flag, Field::Enable),
    (&["disable"], Kind::Flag, Field::Disable),
    (&["initcpio", "i"], Kind::Negatable, Field::Initcpio),
    (&["hookd", "H"], Kind::TextList, Field::Hookd),
    (&["debug", "d"], Kind::Flag, Field::Debug),
    (&["showver", "V"], Kind::Flag, Field::Showver),
    (&["help", "h"], Kind::Flag, Field::Help),
];

/// One name Getopt::Long recognises.
#[derive(Clone, Debug)]
struct Name {
    name: String,
    canonical: &'static str,
    kind: Kind,
    field: Field,
    /// Whether this is a `no` or `no-` form of a negatable option.
    negated: bool,
}

/// Every name, sorted as Perl's `sort keys` sorts them.
fn names() -> Vec<Name> {
    let mut names = Vec::new();
    for (aliases, kind, field) in TABLE {
        for alias in *aliases {
            names.push(Name {
                name: (*alias).to_owned(),
                canonical: aliases[0],
                kind: *kind,
                field: *field,
                negated: false,
            });
            if *kind == Kind::Negatable {
                for prefix in ["no", "no-"] {
                    names.push(Name {
                        name: format!("{prefix}{alias}"),
                        canonical: aliases[0],
                        kind: *kind,
                        field: *field,
                        negated: true,
                    });
                }
            }
        }
    }
    names.sort_by(|a, b| a.name.cmp(&b.name));
    names
}

/// The result of looking at one argument (`FindOption`).
enum Found {
    /// Not an option.
    Operand,
    /// An option in error; the warning has been recorded.
    Error,
    /// A switch with its value.
    Switch(Field, bool),
    /// A string option with its value.
    Text(Field, String),
}

struct Parser {
    names: Vec<Name>,
    posix: bool,
    warnings: Vec<String>,
}

impl Parser {
    /// Looks at one argument: whether it is an option and, if it is, its value,
    /// taken from `rest` when the option needs one and was given no `=value`.
    fn find(&mut self, arg: &str, rest: &mut std::collections::VecDeque<String>) -> Found {
        let Some(starter) = self.starter(arg) else {
            return Found::Operand;
        };
        let (opt, optarg) = self.split_value(starter, &arg[starter.len()..]);
        let Some(tryopt) = self.complete(opt) else {
            return Found::Error;
        };
        let Some(name) = self.names.iter().find(|name| name.name == tryopt).cloned() else {
            if opt.is_empty() {
                self.warnings
                    .push(format!("Missing option after {starter}\n"));
            } else {
                self.warnings.push(format!("Unknown option: {opt}\n"));
            }
            return Found::Error;
        };
        self.value(&name, optarg, rest)
    }

    /// The prefix that makes `arg` an option: `--`, `-`, or (without
    /// POSIXLY_CORRECT) `+`; none for an operand, which a lone `-` is.
    fn starter(&self, arg: &str) -> Option<&'static str> {
        let starter = if arg.starts_with("--") {
            "--"
        } else if arg.starts_with('-') {
            "-"
        } else if arg.starts_with('+') && !self.posix {
            "+"
        } else {
            return None;
        };
        (arg != "-").then_some(starter)
    }

    /// Splits `name=value`, where the name has at least one character, into the
    /// name and the value. Getopt::Long splits it after any prefix unless
    /// POSIXLY_CORRECT limits that to `--`.
    fn split_value<'o>(&self, starter: &str, opt: &'o str) -> (&'o str, Option<String>) {
        if (starter == "--" || !self.posix)
            && let Some(position) = opt.bytes().skip(1).position(|byte| byte == b'=')
        {
            let position = position + 1;
            return (&opt[..position], Some(opt[position + 1..].to_owned()));
        }
        (opt, None)
    }

    /// The name to look up for `opt`: the full name when `opt` is a unique
    /// prefix of one, otherwise `opt` itself. Without a unique option, records
    /// a warning and returns `None`.
    fn complete(&mut self, opt: &str) -> Option<String> {
        if self.posix || opt.is_empty() {
            return Some(opt.to_owned());
        }
        let hits: Vec<&Name> = self
            .names
            .iter()
            .filter(|name| name.name.starts_with(opt))
            .collect();
        let exact = hits.iter().filter(|name| name.name == opt).count();
        let mut resolved: Vec<String> = hits.iter().map(|name| name.name.clone()).collect();
        if !(hits.len() <= 1 || exact == 1) {
            // Several names match: they may all be forms of one option.
            let options = options_of(&hits);
            if options.len() != 1 {
                self.warnings.push(format!(
                    "Option {opt} is ambiguous ({})\n",
                    resolved.join(", ")
                ));
                return None;
            }
            resolved = options;
        }
        if resolved.len() == 1 && resolved[0] != opt {
            return resolved.pop();
        }
        Some(opt.to_owned())
    }

    /// The value of the option `name`: a switch must have no `=value`; a string
    /// takes `optarg` or, without one, the next argument.
    fn value(
        &mut self,
        name: &Name,
        optarg: Option<String>,
        rest: &mut std::collections::VecDeque<String>,
    ) -> Found {
        match name.kind {
            Kind::Flag | Kind::Negatable => {
                if optarg.is_some() {
                    self.warnings
                        .push(format!("Option {} does not take an argument\n", name.name));
                    return Found::Error;
                }
                Found::Switch(name.field, !name.negated)
            }
            Kind::Text | Kind::TextList => {
                let missing = match &optarg {
                    Some(value) => value.is_empty(),
                    None => rest.is_empty(),
                };
                if missing {
                    self.warnings
                        .push(format!("Option {} requires an argument\n", name.name));
                    return Found::Error;
                }
                // A mandatory string takes the next argument whatever it is, even `--`.
                let value = optarg.unwrap_or_else(|| rest.pop_front().unwrap_or_default());
                Found::Text(name.field, value)
            }
        }
    }
}

/// The options that the matching names `hits` are forms of, sorted and without
/// duplicates; a negated form counts as an option of its own.
fn options_of(hits: &[&Name]) -> Vec<String> {
    let mut options: Vec<String> = hits
        .iter()
        .map(|name| {
            if name.negated {
                format!("no{}", name.canonical)
            } else {
                name.canonical.to_owned()
            }
        })
        .collect();
    options.sort();
    options.dedup();
    if options.len() == 2 {
        // `auto_version` lets a match with `version` give way to the other option.
        options.retain(|option| option != "version");
    }
    options
}

/// Parses the arguments; `posixly_correct` tells whether `POSIXLY_CORRECT` is
/// set in the environment.
pub fn parse(args: &[String], posixly_correct: bool) -> Parsed {
    let mut parser = Parser {
        names: names(),
        posix: posixly_correct,
        warnings: Vec::new(),
    };
    let mut options = Options::default();
    let mut errors = 0;
    let mut rest: std::collections::VecDeque<String> = args.iter().cloned().collect();
    while let Some(arg) = rest.pop_front() {
        if arg == "--" {
            break;
        }
        match parser.find(&arg, &mut rest) {
            Found::Operand if posixly_correct => break,
            // Operands are left for the program, which ignores them.
            Found::Operand => {}
            Found::Error => errors += 1,
            Found::Switch(Field::Help, _) => {
                return Parsed {
                    outcome: Outcome::Help,
                    warnings: parser.warnings,
                };
            }
            Found::Switch(field, value) => match field {
                Field::Enable => options.enable = true,
                Field::Disable => options.disable = true,
                Field::Initcpio => options.initcpio = Some(value),
                Field::Debug => options.debug = true,
                Field::Showver => options.showver = true,
                other => unreachable!("{other:?} is not a switch"),
            },
            Found::Text(field, value) => match field {
                Field::Version => options.version = Some(value),
                Field::Kernel => options.kernel = Some(value),
                Field::KernelVersion => options.kernel_version = Some(value),
                Field::Prefix => options.kernel_prefix = Some(value),
                Field::Bootdir => options.bootdir = value,
                Field::Confd => options.confd = Some(value),
                Field::Cmdline => options.cmdline = Some(value),
                Field::Config => options.config = value,
                Field::Hookd => options.hookd.get_or_insert_with(Vec::new).push(value),
                other => unreachable!("{other:?} takes no value"),
            },
        }
    }
    Parsed {
        outcome: if errors == 0 {
            Outcome::Run(Box::new(options))
        } else {
            Outcome::Invalid
        },
        warnings: parser.warnings,
    }
}

#[cfg(test)]
mod tests;
