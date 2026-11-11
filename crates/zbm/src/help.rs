//! `help [TOPIC]`, the recovery shell's help (ADR 0018, point 14): text
//! from the tables the man page is written from, as plain text.
//!
//! - `commands`: each name the image links to `zbm` with its applet's
//!   summary and usage ([`LINKED`], [`APPLETS`]), then the six functions
//!   of the shell library the shell offers ([`FUNCTIONS`]), then a line
//!   that points to `zbm --list`. The man page's "The Recovery Shell" lists
//!   the same, through the example `gen_commands`.
//! - `keys`: the menu's keys, as `zfsbootmenu(7)` lists them, in the key
//!   map `zbm.menu` sets.
//! - `options`: the kernel command line's options, as `zfsbootmenu(7)`
//!   lists them.
//!
//! Without a topic it shows the commands, then names the other topics; an
//! unknown topic prints the usage and exits 2. A reader that stops reading
//! early, as `head` does, ends it with 0, as it had what it wanted. On a
//! terminal the topic shows in the pager (`zbm_tui::menu::pager`), whose
//! keys are read with the same map; without one, or with `TERM=dumb`, its
//! lines are printed. The text is
//! the code's own: nothing in it comes from outside, so nothing is
//! escaped but the topic a user typed.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;

use zbm_config::conf::{self, Conf};
use zbm_config::docs::{self, SECTIONS};
use zbm_platform::{NoConsole, RealSystem, System};
use zbm_tui::menu::keymap::{Map, manual};
use zbm_tui::menu::pager::{self, Pager, Topic};
use zbm_types::{LINKED, Linked, Printable};

use crate::library_usage::FUNCTIONS;
use crate::table::{APPLETS, HELP};
use crate::{Io, USAGE};

/// Shows a topic with the key map it reads keys with: in the pager on a
/// terminal, else its lines printed to the writer.
type Show<'a> = &'a mut dyn FnMut(&Pager, Map, &mut dyn Write) -> io::Result<()>;

/// A topic of `help`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Subject {
    Commands,
    Keys,
    Options,
}

/// The topics, in the order `help` names them: the word that asks for each
/// and what it shows. The usage, the error for an unknown topic and the
/// lines after the commands name them from here.
const TOPICS: [(&str, Subject, &str); 3] = [
    (
        "commands",
        Subject::Commands,
        "the recovery shell's commands",
    ),
    ("keys", Subject::Keys, "the menu's keys"),
    (
        "options",
        Subject::Options,
        "the kernel command line's options",
    ),
];

/// The line after the commands, which points to the other commands of
/// `zbm`.
const LIST: &str = "**zbm --list** lists every command of **zbm**, also those the recovery shell \
                    does not offer, and **zbm** *command* **--help** prints the usage of one.\n";

/// `help` with `args` on this machine.
pub(crate) fn run(args: &[String], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    run_in(&real.with_console(&NoConsole), args, io, &mut pager::show)
}

/// `help` with `args` on `system`, which shows its topic with `show`.
fn run_in(system: &System<'_>, args: &[String], io: &mut Io, show: Show<'_>) -> i32 {
    let subject = match args {
        [] => None,
        [word] => {
            let Some((_, subject, _)) = TOPICS.iter().find(|(name, ..)| name == word) else {
                let _ = writeln!(
                    io.stderr,
                    "help: {} is no topic: {}",
                    Printable(word),
                    topic_names()
                );
                let _ = io.stderr.write_all(HELP.usage.as_bytes());
                return USAGE;
            };
            Some(*subject)
        }
        _ => {
            let _ = io.stderr.write_all(HELP.usage.as_bytes());
            return USAGE;
        }
    };
    let conf = system
        .files
        .read_to_string(Path::new(conf::PATH))
        .map(|text| Conf::parse(&text))
        .unwrap_or_default();
    let (_, map) = crate::menu::menu_asked(system, &conf);
    let pager = Pager {
        topic: match subject {
            None | Some(Subject::Commands) => Topic::Commands,
            Some(Subject::Keys) => Topic::Keys,
            Some(Subject::Options) => Topic::Options,
        },
        lines: lines(subject, map),
    };
    match show(&pager, map, io.stdout) {
        Ok(()) => 0,
        // A reader that stopped reading, as `head` does, had what it
        // wanted.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            let _ = writeln!(
                io.stderr,
                "help: unable to show the topic: {}",
                Printable(&error.to_string())
            );
            1
        }
    }
}

/// The names of the topics as a list: "commands, keys or options".
fn topic_names() -> String {
    let mut text = String::new();
    for (at, (name, ..)) in TOPICS.iter().enumerate() {
        if at + 1 == TOPICS.len() {
            text.push_str(" or ");
        } else if at > 0 {
            text.push_str(", ");
        }
        text.push_str(name);
    }
    text
}

/// What the page of no topic ends with: the topics other than the
/// commands, each with what it shows.
fn other_topics() -> String {
    let mut text = "Other topics:\n".to_owned();
    for (name, subject, about) in &TOPICS {
        if *subject != Subject::Commands {
            let asked = format!("zbm help {name}");
            let _ = writeln!(text, "  {asked:<18} {about}");
        }
    }
    text
}

/// The lines of `subject`, the keys in the key map `map`; without a
/// topic, the commands, then the other topics.
fn lines(subject: Option<Subject>, map: Map) -> Vec<String> {
    let text = match subject {
        None => format!("{}\n{}", docs::plain(&commands()), other_topics()),
        Some(Subject::Commands) => docs::plain(&commands()),
        Some(Subject::Keys) => docs::plain(&manual::help_page(map)),
        Some(Subject::Options) => SECTIONS
            .iter()
            .find(|section| section.name == "cmdline")
            .map(docs::Section::render_plain)
            .unwrap_or_default(),
    };
    text.lines().map(str::to_owned).collect()
}

/// The recovery shell's commands, as reStructuredText in the form of the
/// man page's generated sections: an entry for each applet the image
/// links, headed by its linked names, with its summary and its usage;
/// then one for each function of the shell library the shell offers; then
/// the line that points to `zbm --list`.
pub(crate) fn commands() -> String {
    let mut entries: Vec<String> = Vec::new();
    let mut listed: Vec<&str> = Vec::new();
    for linked in LINKED {
        let Some((applet, _)) = crate::find(APPLETS, linked.name) else {
            continue;
        };
        if listed.contains(&applet.name) {
            continue;
        }
        listed.push(applet.name);
        let names: Vec<String> = LINKED
            .iter()
            .filter(|other| {
                crate::find(APPLETS, other.name).is_some_and(|(of, _)| of.name == applet.name)
            })
            .map(heading)
            .collect();
        entries.push(entry(&names, applet.summary, applet.usage));
    }
    for function in &FUNCTIONS {
        let names = [function.name.to_owned()];
        entries.push(entry(&names, function.summary, function.usage));
    }
    entries.push(LIST.to_owned());
    entries.join("\n")
}

/// The name of `linked` as an entry's heading: as the shell runs it, its
/// directory too where that is not `/bin`.
fn heading(linked: &Linked) -> String {
    if linked.dir == "/bin" {
        linked.name.to_owned()
    } else {
        format!("{}/{}", linked.dir, linked.name)
    }
}

/// One entry: the names in bold, then the summary as a sentence and the
/// usage as a code block, indented. A summary written for `zbm --list`
/// may end with a name in brackets, as "(also shutdown)": under a heading
/// that names it already, the brackets are left out.
fn entry(names: &[String], summary: &str, usage: &str) -> String {
    let summary = without_names(summary, names);
    let mut chars = summary.chars();
    let first: String = chars
        .next()
        .into_iter()
        .flat_map(char::to_uppercase)
        .collect();
    let bold: Vec<String> = names.iter().map(|name| format!("**{name}**")).collect();
    let mut text = format!(
        "{}\n\n  {first}{}.\n\n  .. code-block::\n\n",
        bold.join(", "),
        chars.as_str()
    );
    for line in usage.lines() {
        if !line.is_empty() {
            text.push_str("    ");
            text.push_str(line);
        }
        text.push('\n');
    }
    text
}

/// `summary` without the brackets it ends with, where they hold one of
/// `names`, with or without "also" before it; else `summary`.
fn without_names<'a>(summary: &'a str, names: &[String]) -> &'a str {
    let Some((before, bracketed)) = summary.rsplit_once(" (") else {
        return summary;
    };
    let inside = bracketed.strip_suffix(')').unwrap_or_default();
    let name = inside.strip_prefix("also ").unwrap_or(inside);
    if names.iter().any(|other| other == name) {
        before
    } else {
        summary
    }
}

#[cfg(test)]
mod tests;
