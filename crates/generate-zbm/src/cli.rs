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
    fn find(&mut self, arg: &str, rest: &mut std::collections::VecDeque<String>) -> Found {
        // The prefix: `--`, `-`, or (without POSIXLY_CORRECT) `+`. A lone `-` is an
        // operand.
        let starter = if arg.starts_with("--") {
            "--"
        } else if arg.starts_with('-') {
            "-"
        } else if arg.starts_with('+') && !self.posix {
            "+"
        } else {
            return Found::Operand;
        };
        if arg == "-" {
            return Found::Operand;
        }
        let mut opt = &arg[starter.len()..];

        // `name=value`, where the name has at least one character; Getopt::Long splits
        // it after any prefix unless POSIXLY_CORRECT limits that to `--`.
        let mut optarg = None;
        if (starter == "--" || !self.posix)
            && let Some(position) = opt.bytes().skip(1).position(|byte| byte == b'=')
        {
            let position = position + 1;
            optarg = Some(opt[position + 1..].to_owned());
            opt = &opt[..position];
        }

        // Unique prefixes stand for the full name.
        let mut tryopt = opt.to_owned();
        if !self.posix && !opt.is_empty() {
            let hits: Vec<&Name> = self
                .names
                .iter()
                .filter(|name| name.name.starts_with(opt))
                .collect();
            let exact = hits.iter().filter(|name| name.name == opt).count();
            let mut resolved: Vec<String> = hits.iter().map(|name| name.name.clone()).collect();
            if !(hits.len() <= 1 || exact == 1) {
                // Several names match: they may all be forms of one option.
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
                if options.len() != 1 {
                    self.warnings.push(format!(
                        "Option {opt} is ambiguous ({})\n",
                        resolved.join(", ")
                    ));
                    return Found::Error;
                }
                resolved = options;
            }
            if resolved.len() == 1 && resolved[0] != opt {
                tryopt.clone_from(&resolved[0]);
            }
        }

        let Some(name) = self.names.iter().find(|name| name.name == tryopt).cloned() else {
            if opt.is_empty() {
                self.warnings
                    .push(format!("Missing option after {starter}\n"));
            } else {
                self.warnings.push(format!("Unknown option: {opt}\n"));
            }
            return Found::Error;
        };

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
mod tests {
    use super::*;
    use zbm_core::exec::{Command, Runner, Stream, SystemRunner};
    use zbm_testkit::legacy::path;
    use zbm_testkit::{Rng, have_program, parity};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn run(list: &[&str]) -> Options {
        match parse(&args(list), false).outcome {
            Outcome::Run(options) => *options,
            other => panic!("{list:?} gave {other:?}"),
        }
    }

    #[test]
    fn names_and_values() {
        let options = run(&[
            "-c",
            "-d",
            "--kv",
            "6.1",
            "+V",
            "--no",
            "-H",
            "a",
            "--hookd=b",
            "x",
        ]);
        assert_eq!(options.config, "-d");
        assert_eq!(options.kernel_version.as_deref(), Some("6.1"));
        assert!(options.showver && !options.debug);
        assert_eq!(options.initcpio, Some(false));
        assert_eq!(options.hookd, Some(vec!["a".to_owned(), "b".to_owned()]));
        assert_eq!(run(&["-config=foo"]).config, "foo");
        assert_eq!(run(&["--init"]).initcpio, Some(true));
        assert!(!run(&["--", "-d"]).debug);
    }

    #[test]
    fn errors() {
        let parsed = parse(
            &args(&["--con", "x", "-dV", "--config=", "--en=1", "+"]),
            false,
        );
        assert_eq!(parsed.outcome, Outcome::Invalid);
        assert_eq!(
            parsed.warnings,
            [
                "Option con is ambiguous (confd, config)\n",
                "Unknown option: dV\n",
                "Option config requires an argument\n",
                "Option enable does not take an argument\n",
                "Missing option after +\n",
            ]
        );
        let parsed = parse(&args(&["--x", "--help", "--y"]), false);
        assert_eq!(parsed.outcome, Outcome::Help);
        assert_eq!(parsed.warnings, ["Unknown option: x\n"]);
        let parsed = parse(&args(&["foo", "-d"]), true);
        assert_eq!(parsed.outcome, Outcome::Run(Box::default()));
    }

    /// The `GetOptions` call of the Perl program, run on `list`, reporting the
    /// options, warnings and status.
    fn perl_parse(source: &str, list: &[String], posix: bool) -> String {
        let start = source.find("GetOptions(").unwrap();
        let end = source[start..].find(") or exit 1;").unwrap() + start;
        let call = &source[start..end + 1];
        let program = format!(
            "use strict; use warnings; use Getopt::Long qw(:config no_ignore_case auto_version);\n\
             my %runConf; $runConf{{config}} = '/etc/zfsbootmenu/config.yaml';\n\
             $runConf{{bootdir}} = '/boot';\n\
             sub pod2usage {{ print \"HELP\\n\"; exit 1; }}\n\
             my $ok = {call};\n\
             for my $k (sort keys %runConf) {{ my $v = $runConf{{$k}}; next unless defined $v;\n\
               $v = join('|', @$v) if ref $v; print \"$k=$v\\n\"; }}\n\
             print $ok ? \"ok\\n\" : \"invalid\\n\";\n"
        );
        let mut command = Command::new("perl")
            .arg("-e")
            .arg(program)
            .arg("--")
            .args(list.iter().cloned())
            .merge_stderr()
            .stderr(Stream::Capture)
            .env_remove("POSIXLY_CORRECT");
        if posix {
            command = command.env("POSIXLY_CORRECT", "1");
        }
        SystemRunner.run(&command).unwrap().stdout_text()
    }

    fn rust_parse(list: &[String], posix: bool) -> String {
        let parsed = parse(list, posix);
        let mut text: String = parsed.warnings.concat();
        match parsed.outcome {
            Outcome::Help => text.push_str("HELP\n"),
            Outcome::Invalid | Outcome::Run(_) => {
                let options = match &parsed.outcome {
                    Outcome::Run(options) => (**options).clone(),
                    _ => Options::default(),
                };
                // After an error Perl still holds the values it stored; only valid runs are
                // compared in full.
                if matches!(parsed.outcome, Outcome::Run(_)) {
                    let mut fields: Vec<(&str, String)> = vec![
                        ("bootdir", options.bootdir.clone()),
                        ("config", options.config.clone()),
                    ];
                    let optional = [
                        ("cmdline", &options.cmdline),
                        ("confd", &options.confd),
                        ("kernel", &options.kernel),
                        ("kernel_prefix", &options.kernel_prefix),
                        ("kernel_version", &options.kernel_version),
                        ("version", &options.version),
                    ];
                    for (key, value) in optional {
                        if let Some(value) = value {
                            fields.push((key, value.clone()));
                        }
                    }
                    if let Some(hooks) = &options.hookd {
                        fields.push(("cpio_hookd", hooks.join("|")));
                    }
                    for (key, set) in [
                        ("debug", options.debug),
                        ("disable", options.disable),
                        ("enable", options.enable),
                        ("showver", options.showver),
                    ] {
                        if set {
                            fields.push((key, "1".to_owned()));
                        }
                    }
                    if let Some(value) = options.initcpio {
                        fields.push(("usecpio", u8::from(value).to_string()));
                    }
                    fields.sort();
                    for (key, value) in fields {
                        text.push_str(&format!("{key}={value}\n"));
                    }
                    text.push_str("ok\n");
                } else {
                    text.push_str("invalid\n");
                }
            }
        }
        text
    }

    /// Perl's output without the option values of invalid runs, which the Rust
    /// parser does not report.
    fn comparable(perl: &str) -> String {
        if perl.ends_with("invalid\n") {
            perl.lines()
                .filter(|line| {
                    !line.contains('=')
                        || line.starts_with("Option ")
                        || line.starts_with("Unknown")
                })
                .map(|line| format!("{line}\n"))
                .collect()
        } else {
            perl.to_owned()
        }
    }

    #[test]
    fn matches_getopt_long() {
        let mut parity = parity!("matches_getopt_long");
        if parity.mode().needs_legacy() && !have_program("perl") {
            eprintln!("skipped: perl is missing");
            return;
        }
        let pieces = [
            "-v",
            "--version",
            "--ver",
            "-k",
            "--kernel=x",
            "--k",
            "--ke",
            "--con",
            "--conf",
            "--confi",
            "-c",
            "--config=",
            "-C=c",
            "--noi",
            "--no-initcpio",
            "--noinit",
            "--init",
            "-i",
            "--n",
            "--no",
            "-H",
            "--hookd=a",
            "--ho",
            "--debug",
            "-d",
            "-de",
            "-dV",
            "--debug=1",
            "+d",
            "+",
            "-",
            "--",
            "---x",
            "x",
            "-x",
            "--x",
            "--en",
            "--e",
            "--enable",
            "--disable",
            "--dis",
            "--di",
            "-V",
            "--showver",
            "--sh",
            "-K",
            "--kv",
            "-p",
            "--pre",
            "-b",
            "--boot",
            "-l",
            "--cm",
            "a",
            "",
            "a=b",
            "--=x",
            "--Kver",
            "--h",
            "--help",
            "-noinitcpio",
            "--initcpio=1",
        ];
        let mut rng = Rng::from_env(10);
        for _ in 0..300 {
            let count = rng.below(6);
            let list: Vec<String> = (0..count)
                .map(|_| (*rng.choose(&pieces)).to_owned())
                .collect();
            let posix = rng.below(5) == 0;
            let legacy: String = parity.legacy(format!("args {list:?}, posix {posix}"), || {
                let source = std::fs::read_to_string(path("bin/generate-zbm")).unwrap();
                comparable(&perl_parse(&source, &list, posix))
            });
            assert_eq!(
                rust_parse(&list, posix),
                legacy,
                "args {list:?}, posix {posix}"
            );
        }
    }
}
