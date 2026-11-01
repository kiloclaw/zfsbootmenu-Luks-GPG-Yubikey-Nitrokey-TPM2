//! The command line of the host tools (ADR 0013, point 5): one parser for
//! `generate-zbm` and `zbm-kcl`, which take the options their manual pages
//! document, in the spellings they document.
//!
//! - A short option is `-x`; short options may be grouped (`-ed`), and one
//!   that takes a value takes the rest of its word (`-afoo`) or the next
//!   argument, whatever it looks like (`-a -x`).
//! - A long option is `--name`, with its value as `--name=value` or the next
//!   argument; a negatable one is also `--no-name`.
//! - `--` ends the options. Operands may come anywhere ([`Order::Permute`])
//!   or end the options ([`Order::StopAtOperand`], as `getopts` reads them,
//!   stopping also at the first problem).
//!
//! The spellings that Perl's `Getopt::Long` also took, a unique prefix of a
//! long name (`--ver`), `+name`, a single dash before a long name (`-kernel`)
//! and `--noname`, still work for one release, each with a warning that names
//! the documented spelling (ADR 0013, answer 4); after it they are errors.

/// One option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Spec {
    /// Its long name, without `--`.
    pub long: Option<&'static str>,
    /// Its short name, without `-`.
    pub short: Option<char>,
    /// Whether it takes a value.
    pub takes_value: bool,
    /// Whether `--no-NAME` turns it off.
    pub negatable: bool,
}

impl Spec {
    /// A switch.
    pub const fn flag(long: Option<&'static str>, short: Option<char>) -> Spec {
        Spec {
            long,
            short,
            takes_value: false,
            negatable: false,
        }
    }

    /// An option with a value.
    pub const fn value(long: Option<&'static str>, short: Option<char>) -> Spec {
        Spec {
            long,
            short,
            takes_value: true,
            negatable: false,
        }
    }

    /// A switch that `--no-NAME` turns off.
    pub const fn negatable(long: &'static str, short: Option<char>) -> Spec {
        Spec {
            long: Some(long),
            short,
            takes_value: false,
            negatable: true,
        }
    }

    /// How the manual spells it: `--long`, else `-x`.
    fn spelling(&self) -> String {
        match (self.long, self.short) {
            (Some(long), _) => format!("--{long}"),
            (None, Some(short)) => format!("-{short}"),
            (None, None) => String::new(),
        }
    }
}

/// Where operands may be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Order {
    /// Anywhere among the options.
    Permute,
    /// The first operand ends the options, and the first problem ends
    /// the reading.
    StopAtOperand,
}

/// An option that was given.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Given {
    /// Which, by its place in the table.
    pub index: usize,
    /// Its value, for one that takes one.
    pub value: Option<String>,
    /// Whether it was turned off (`--no-NAME`).
    pub negated: bool,
}

/// What is wrong with an option.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Problem {
    /// An option that is not one, as given.
    Unknown(String),
    /// A prefix of several long names: the prefix and the names.
    Ambiguous(String, Vec<String>),
    /// An option that takes a value, at the end of the arguments.
    MissingValue(String),
    /// A switch given a value (`--debug=1`).
    UnwantedValue(String),
}

/// The parsed command line.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Parsed {
    /// The options given, in order.
    pub given: Vec<Given>,
    /// The operands, in order.
    pub operands: Vec<String>,
    /// Warnings about spellings the manual does not document.
    pub warnings: Vec<String>,
    /// What was wrong, in order.
    pub problems: Vec<Problem>,
}

/// A long option found for a name.
struct Long {
    index: usize,
    spec: Spec,
    negated: bool,
    /// The documented spelling, when the name given was not it.
    documented: Option<String>,
}

/// The parser of one command line.
struct Parser<'a> {
    program: &'a str,
    specs: &'a [Spec],
    parsed: Parsed,
}

impl Parser<'_> {
    /// The option whose long name is `name`, with its place in the table.
    fn exact(&self, name: &str) -> Option<(usize, Spec)> {
        self.specs
            .iter()
            .copied()
            .enumerate()
            .find(|(_, spec)| spec.long == Some(name))
    }

    /// The long option `name` names: exactly, or (warned) as `noNAME` or a
    /// unique prefix.
    fn long(&mut self, name: &str) -> Option<Long> {
        if let Some((index, spec)) = self.exact(name) {
            return Some(Long {
                index,
                spec,
                negated: false,
                documented: None,
            });
        }
        let negations = [name.strip_prefix("no-"), name.strip_prefix("no")];
        for (form, base) in negations.into_iter().enumerate() {
            let Some((index, spec)) = base
                .and_then(|base| self.exact(base))
                .filter(|(_, spec)| spec.negatable)
            else {
                continue;
            };
            let documented = (form == 1).then(|| format!("--no-{}", base.unwrap_or_default()));
            return Some(Long {
                index,
                spec,
                negated: true,
                documented,
            });
        }
        let hits: Vec<(usize, Spec)> = self
            .specs
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, spec)| spec.long.is_some_and(|long| long.starts_with(name)))
            .collect();
        match hits.as_slice() {
            [(index, spec)] if !name.is_empty() => Some(Long {
                index: *index,
                spec: *spec,
                negated: false,
                documented: Some(spec.spelling()),
            }),
            [] | [_] => {
                self.parsed.problems.push(Problem::Unknown(name.to_owned()));
                None
            }
            _ => {
                let names = hits.iter().map(|(_, spec)| spec.spelling()).collect();
                self.parsed
                    .problems
                    .push(Problem::Ambiguous(name.to_owned(), names));
                None
            }
        }
    }

    /// Warns that `given` is not the documented spelling `documented`.
    fn warn(&mut self, given: &str, documented: &str) {
        self.parsed.warnings.push(format!(
            "{}: {given} is not how the manual spells this option; use {documented}, as {given} \
             stops working in the next release",
            self.program
        ));
    }

    /// A long option, `name` as given after `--`, `+` or `-`, with
    /// `rest` for its value.
    fn take_long(&mut self, arg: &str, start: &str, rest: &mut std::collections::VecDeque<String>) {
        let body = &arg[start.len()..];
        let (name, inline) = match body.split_once('=') {
            Some((name, value)) if !name.is_empty() => (name, Some(value.to_owned())),
            _ => (body, None),
        };
        let Some(found) = self.long(name) else {
            return;
        };
        let spec = found.spec;
        let documented = if found.negated {
            format!("--no-{}", spec.long.unwrap_or_default())
        } else {
            spec.spelling()
        };
        if start != "--" || found.documented.is_some() {
            let given = arg.split('=').next().unwrap_or(arg).to_owned();
            self.warn(&given, &found.documented.unwrap_or(documented.clone()));
        }
        self.finish(
            (found.index, spec),
            found.negated,
            inline,
            &documented,
            rest,
        );
    }

    /// Records the option `spec`, at `index` in the table, its value taken
    /// from `inline` or `rest`.
    fn finish(
        &mut self,
        (index, spec): (usize, Spec),
        negated: bool,
        inline: Option<String>,
        shown: &str,
        rest: &mut std::collections::VecDeque<String>,
    ) {
        let value = if spec.takes_value {
            let Some(value) = inline.or_else(|| rest.pop_front()) else {
                self.parsed
                    .problems
                    .push(Problem::MissingValue(shown.to_owned()));
                return;
            };
            Some(value)
        } else if inline.is_some() {
            self.parsed
                .problems
                .push(Problem::UnwantedValue(shown.to_owned()));
            return;
        } else {
            None
        };
        self.parsed.given.push(Given {
            index,
            value,
            negated,
        });
    }

    /// A group of short options after `-`.
    fn take_short(&mut self, group: &str, rest: &mut std::collections::VecDeque<String>) {
        for (at, c) in group.char_indices() {
            let found = self
                .specs
                .iter()
                .copied()
                .enumerate()
                .find(|(_, spec)| spec.short == Some(c));
            let Some((index, spec)) = found else {
                self.parsed.problems.push(Problem::Unknown(c.to_string()));
                return;
            };
            if spec.takes_value {
                let attached = group.get(at + c.len_utf8()..).unwrap_or_default();
                let inline = (!attached.is_empty()).then(|| attached.to_owned());
                self.finish((index, spec), false, inline, &format!("-{c}"), rest);
                return;
            }
            self.parsed.given.push(Given {
                index,
                value: None,
                negated: false,
            });
        }
    }

    /// Whether `-word` names a long option rather than a group of short ones:
    /// its name, before any `=`, has more than one character and is a long
    /// name or the unique start of one.
    fn is_single_dash_long(&self, word: &str) -> bool {
        let name = word.split('=').next().unwrap_or(word);
        if name.chars().count() < 2 {
            return false;
        }
        let starts = self
            .specs
            .iter()
            .filter(|spec| spec.long.is_some_and(|long| long.starts_with(name)))
            .count();
        starts == 1 || self.specs.iter().any(|spec| spec.long == Some(name))
    }
}

/// Reads `args` with the options `specs`; `program` names the tool in
/// warnings.
pub fn parse(program: &str, specs: &[Spec], order: Order, args: &[String]) -> Parsed {
    let mut parser = Parser {
        program,
        specs,
        parsed: Parsed::default(),
    };
    let mut rest: std::collections::VecDeque<String> = args.iter().cloned().collect();
    while let Some(arg) = rest.pop_front() {
        if order == Order::StopAtOperand && !parser.parsed.problems.is_empty() {
            break;
        }
        if arg == "--" {
            parser.parsed.operands.extend(rest.drain(..));
            break;
        }
        let has_long = specs.iter().any(|spec| spec.long.is_some());
        if arg.starts_with("--") {
            parser.take_long(&arg, "--", &mut rest);
        } else if arg.starts_with('+') && arg.len() > 1 && has_long {
            parser.take_long(&arg, "+", &mut rest);
        } else if let Some(word) = arg.strip_prefix('-').filter(|word| !word.is_empty()) {
            if parser.is_single_dash_long(word) {
                parser.take_long(&arg, "-", &mut rest);
            } else {
                parser.take_short(word, &mut rest);
            }
        } else if order == Order::StopAtOperand {
            parser.parsed.operands.push(arg);
            parser.parsed.operands.extend(rest.drain(..));
            break;
        } else {
            parser.parsed.operands.push(arg);
        }
    }
    parser.parsed
}

#[cfg(test)]
mod tests;
