//! Command-line options as Bash's `getopts` reads them.
//!
//! The legacy shell tools parse options with `getopts`: options may be grouped
//! (`-ed`), an option's value may follow it directly (`-afoo`) or as the next
//! argument (`-a foo`, even when that starts with `-`), `--` ends the options,
//! and the first argument that is not an option ends them too. [`parse`] reads
//! arguments the same way, so the Rust tools accept exactly the same command
//! lines.

/// An option that was read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Opt {
    /// An option without a value.
    Flag(char),
    /// An option with its value.
    Value(char, String),
}

/// Why `getopts` stopped with an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// An option letter that the option string does not list.
    Illegal(char),
    /// An option that needs a value, at the end of the arguments.
    MissingArgument(char),
}

impl Error {
    /// The message Bash prints for the error, with `program` being the script's
    /// `$0`.
    pub fn message(self, program: &str) -> String {
        match self {
            Error::Illegal(option) => format!("{program}: illegal option -- {option}"),
            Error::MissingArgument(option) => {
                format!("{program}: option requires an argument -- {option}")
            }
        }
    }
}

/// The result of reading options.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Parsed {
    /// The options read before the end of the options or an error, in order.
    pub options: Vec<Opt>,
    /// The error that stopped reading, if any. Options after it are not read.
    pub error: Option<Error>,
    /// The arguments after the options (`"$@"` after `shift $((OPTIND-1))`).
    pub operands: Vec<String>,
}

/// Reads options from `args` as `while getopts "<optstring>" opt` does; a
/// letter followed by `:` in `optstring` takes a value.
pub fn parse(optstring: &str, args: &[String]) -> Parsed {
    let takes_value = |option: char| -> Option<bool> {
        let mut chars = optstring.chars().peekable();
        while let Some(letter) = chars.next() {
            let value = chars.peek() == Some(&':');
            if value {
                chars.next();
            }
            if letter == option && letter != ':' {
                return Some(value);
            }
        }
        None
    };

    let mut parsed = Parsed::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            index += 1;
            break;
        }
        let Some(group) = arg.strip_prefix('-').filter(|group| !group.is_empty()) else {
            break;
        };
        index += 1;
        for (position, option) in group.char_indices() {
            match takes_value(option) {
                None => {
                    parsed.error = Some(Error::Illegal(option));
                    parsed.operands = args[index..].to_vec();
                    return parsed;
                }
                Some(false) => parsed.options.push(Opt::Flag(option)),
                Some(true) => {
                    let rest = &group[position + option.len_utf8()..];
                    if !rest.is_empty() {
                        parsed.options.push(Opt::Value(option, rest.to_owned()));
                    } else if let Some(value) = args.get(index) {
                        parsed.options.push(Opt::Value(option, value.clone()));
                        index += 1;
                    } else {
                        parsed.error = Some(Error::MissingArgument(option));
                        return parsed;
                    }
                    break;
                }
            }
        }
    }
    parsed.operands = args[index..].to_vec();
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn reads_flags_values_and_operands() {
        let parsed = parse(
            "ha:r:o:ed",
            &args(&["-ed", "-afoo", "-r", "-x", "pool/be", "-a"]),
        );
        assert_eq!(
            parsed.options,
            [
                Opt::Flag('e'),
                Opt::Flag('d'),
                Opt::Value('a', "foo".to_owned()),
                Opt::Value('r', "-x".to_owned()),
            ]
        );
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.operands, ["pool/be", "-a"]);
    }

    #[test]
    fn double_dash_and_dash_end_the_options() {
        assert_eq!(parse("a:", &args(&["--", "-a"])).operands, ["-a"]);
        assert_eq!(parse("a:", &args(&["-", "x"])).operands, ["-", "x"]);
        assert_eq!(
            parse("a:", &args(&["-a", "--"])).options,
            [Opt::Value('a', "--".to_owned())]
        );
        assert!(parse("a:", &args(&[])).operands.is_empty());
    }

    #[test]
    fn errors_stop_reading() {
        let parsed = parse("ha:", &args(&["-h", "-x", "-a", "v"]));
        assert_eq!(parsed.options, [Opt::Flag('h')]);
        assert_eq!(parsed.error, Some(Error::Illegal('x')));
        let parsed = parse("ha:", &args(&["-a"]));
        assert_eq!(parsed.error, Some(Error::MissingArgument('a')));
        assert_eq!(parse("a:", &args(&["-:"])).error, Some(Error::Illegal(':')));
        assert_eq!(
            Error::Illegal('x').message("/usr/bin/zbm-kcl"),
            "/usr/bin/zbm-kcl: illegal option -- x"
        );
        assert_eq!(
            Error::MissingArgument('a').message("zbm-kcl"),
            "zbm-kcl: option requires an argument -- a"
        );
    }

    /// What Bash's `getopts "ha:r:o:ed"` loop prints for each case, recorded
    /// from Bash 5.2 with this script:
    ///
    /// ```text
    /// while getopts "ha:r:o:ed" opt 2>/dev/null; do
    ///   case "$opt" in
    ///     \?) echo "error"; break ;;
    ///     *) echo "opt $opt ${OPTARG-}" ;;
    ///   esac
    /// done
    /// shift $((OPTIND-1)); for a in "$@"; do echo "operand $a"; done
    /// ```
    #[test]
    fn matches_bash_getopts() {
        let cases: &[(&[&str], &str)] = &[
            (
                &["-ed", "-afoo", "x"],
                "opt e \nopt d \nopt a foo\noperand x\n",
            ),
            (&["-a", "-r", "y"], "opt a -r\noperand y\n"),
            (&["--", "-a"], "operand -a\n"),
            (&["-", "-a", "v"], "operand -\noperand -a\noperand v\n"),
            (&["-e", "-q", "z"], "opt e \nerror\noperand z\n"),
            (
                &["-oout", "-d", "--", "w"],
                "opt o out\nopt d \noperand w\n",
            ),
            (&["-ea"], "opt e \nerror\n"),
            (&[], ""),
        ];
        for (case, bash) in cases {
            let parsed = parse("ha:r:o:ed", &args(case));
            let mut printed = String::new();
            for option in &parsed.options {
                match option {
                    Opt::Flag(letter) => printed.push_str(&format!("opt {letter} \n")),
                    Opt::Value(letter, value) => {
                        printed.push_str(&format!("opt {letter} {value}\n"));
                    }
                }
            }
            if parsed.error.is_some() {
                printed.push_str("error\n");
            }
            for operand in &parsed.operands {
                printed.push_str(&format!("operand {operand}\n"));
            }
            assert_eq!(printed, *bash, "args {case:?}");
        }
    }
}
