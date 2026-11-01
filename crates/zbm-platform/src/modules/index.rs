//! What `modprobe` reads to load a module, read without it (ROADMAP.md
//! CD4): the kernel's index of its modules (`modules.dep`, `modules.alias`,
//! `modules.softdep`, `modules.builtin`), the configuration of
//! `modprobe.d` (`options`, `alias`, `softdep`, `blacklist`, `install`),
//! and the modules' parameters on the kernel's command line
//! (`name.parameter=value`). From them, [`Index::plan`] gives the files to
//! insert, in order, with their parameters.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A module's name as the kernel keeps it: `-` is `_`.
pub fn normalized(name: &str) -> String {
    name.replace('-', "_")
}

/// An alias's pattern as `modprobe` matches it: `-` is `_`, but in a
/// `[...]`, where it makes a range.
fn normalized_pattern(pattern: &str) -> String {
    let mut inside = false;
    pattern
        .chars()
        .map(|c| match c {
            '[' => {
                inside = true;
                c
            }
            ']' => {
                inside = false;
                c
            }
            '-' if !inside => '_',
            c => c,
        })
        .collect()
}

/// The name of the module in the file `path`: its file name without
/// `.ko` and a compression's extension.
pub fn name_of(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.split(".ko").next().unwrap_or(file);
    normalized(stem)
}

/// One module the index knows: its file, below the modules' directory,
/// and the files of what it needs, in the order `modules.dep` gives.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Entry {
    path: String,
    dependencies: Vec<String>,
}

/// The modules to load before and after one (`softdep`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Soft {
    pre: Vec<String>,
    post: Vec<String>,
}

/// The index and configuration of the modules of a kernel.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Index {
    /// The modules' directory, `/lib/modules/<release>`.
    root: PathBuf,
    modules: BTreeMap<String, Entry>,
    /// (pattern, module), the configuration's first, then the kernel's.
    aliases: Vec<(String, String)>,
    soft: BTreeMap<String, Soft>,
    builtin: BTreeSet<String>,
    options: BTreeMap<String, Vec<String>>,
    blacklist: BTreeSet<String>,
    /// Modules whose loading the configuration replaces with a command.
    installs: BTreeSet<String>,
}

/// One module to insert.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Step {
    /// Its name.
    pub name: String,
    /// Its file.
    pub path: PathBuf,
    /// Its parameters, separated by spaces.
    pub parameters: String,
    /// Whether loading the rest goes on when it fails: a `softdep`.
    pub optional: bool,
}

/// What it takes to load a module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Plan {
    /// Insert these files, in order; none when it is built in.
    Insert(Vec<Step>),
    /// The configuration runs a command in its place (`install`): only
    /// `modprobe` can.
    Command,
    /// No module of that name or alias.
    Unknown,
}

impl Index {
    /// The index of the modules in `root`, from the texts of its
    /// `modules.dep`, `modules.alias`, `modules.softdep` and
    /// `modules.builtin`, the files of `modprobe.d` in the order they are
    /// read, and the kernel's command line.
    pub fn parse(root: &Path, kernel: &KernelFiles<'_>, config: &[&str], cmdline: &str) -> Index {
        let mut index = Index {
            root: root.to_owned(),
            ..Index::default()
        };
        for line in kernel.dep.lines() {
            let Some((path, dependencies)) = line.split_once(':') else {
                continue;
            };
            let entry = Entry {
                path: path.trim().to_owned(),
                dependencies: dependencies.split_whitespace().map(str::to_owned).collect(),
            };
            index.modules.insert(name_of(path.trim()), entry);
        }
        for text in config {
            index.configure(text);
        }
        for line in kernel.alias.lines() {
            if let ["alias", pattern, module] = words(line).as_slice() {
                index
                    .aliases
                    .push((normalized_pattern(pattern), normalized(module)));
            }
        }
        for line in kernel.softdep.lines() {
            index.add_softdep(&words(line));
        }
        index.builtin = kernel
            .builtin
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(name_of)
            .collect();
        index.add_cmdline(cmdline);
        index
    }

    /// Adds the lines of a file of `modprobe.d`.
    fn configure(&mut self, text: &str) {
        for line in logical_lines(text) {
            let words = words(&line);
            match words.as_slice() {
                ["options", module, parameters @ ..] => self
                    .options
                    .entry(normalized(module))
                    .or_default()
                    .extend(parameters.iter().map(|&word| word.to_owned())),
                ["alias", pattern, module] => self
                    .aliases
                    .push((normalized_pattern(pattern), normalized(module))),
                ["blacklist", module] => {
                    self.blacklist.insert(normalized(module));
                }
                ["install", module, ..] => {
                    self.installs.insert(normalized(module));
                }
                ["softdep", ..] => self.add_softdep(&words),
                _ => {}
            }
        }
    }

    /// Adds `softdep MODULE pre: A B post: C`.
    fn add_softdep(&mut self, words: &[&str]) {
        let ["softdep", module, rest @ ..] = words else {
            return;
        };
        let soft = self.soft.entry(normalized(module)).or_default();
        let mut post = false;
        for &word in rest {
            match word {
                "pre:" => post = false,
                "post:" => post = true,
                name if post => soft.post.push(normalized(name)),
                name => soft.pre.push(normalized(name)),
            }
        }
    }

    /// Adds the parameters of the kernel's command line `cmdline`:
    /// `module.parameter=value`, and `modprobe.blacklist=a,b`.
    fn add_cmdline(&mut self, cmdline: &str) {
        for word in cmdline.split_whitespace() {
            let Some((module, parameter)) = word.split_once('.') else {
                continue;
            };
            if module == "modprobe"
                && let Some(names) = parameter.strip_prefix("blacklist=")
            {
                self.blacklist.extend(
                    names
                        .split(',')
                        .filter(|name| !name.is_empty())
                        .map(normalized),
                );
            } else if !module.is_empty() && !parameter.is_empty() && !module.contains('=') {
                self.options
                    .entry(normalized(module))
                    .or_default()
                    .push(parameter.to_owned());
            }
        }
    }

    /// Whether the index knows any module: an image without `modules.dep`
    /// has none, and only `modprobe` might load one.
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.builtin.is_empty()
    }

    /// The modules `name` stands for: itself, or what the aliases match,
    /// but not the blacklisted.
    fn resolve(&self, name: &str) -> Vec<String> {
        let name = normalized(name);
        if self.modules.contains_key(&name) || self.builtin.contains(&name) {
            return vec![name];
        }
        let mut found: Vec<String> = Vec::new();
        for (pattern, module) in &self.aliases {
            if matches_glob(pattern.as_bytes(), name.as_bytes())
                && !self.blacklist.contains(module)
                && !found.contains(module)
            {
                found.push(module.clone());
            }
        }
        found
    }

    /// How to load `name` with the parameters `extra` besides its own.
    pub fn plan(&self, name: &str, extra: &[&str]) -> Plan {
        let modules = self.resolve(name);
        if modules.is_empty() {
            return Plan::Unknown;
        }
        let mut steps = Vec::new();
        let mut seen = BTreeSet::new();
        for module in &modules {
            if !self.can_add(module, false, &mut steps, &mut seen) {
                return Plan::Command;
            }
        }
        // The parameters asked for go to the module asked for.
        let asked = normalized(name);
        if let Some(step) = steps.iter_mut().rev().find(|step| step.name == asked) {
            for parameter in extra {
                if !step.parameters.is_empty() {
                    step.parameters.push(' ');
                }
                step.parameters.push_str(parameter);
            }
        }
        Plan::Insert(steps)
    }

    /// Adds the steps of `module`: its `softdep`s before, what it needs,
    /// itself, its `softdep`s after; whether it could: not when the
    /// configuration replaces one of them with a command.
    fn can_add(
        &self,
        module: &str,
        optional: bool,
        steps: &mut Vec<Step>,
        seen: &mut BTreeSet<String>,
    ) -> bool {
        if !seen.insert(module.to_owned()) {
            return true;
        }
        if self.installs.contains(module) {
            return false;
        }
        if self.builtin.contains(module) {
            return true;
        }
        let Some(entry) = self.modules.get(module) else {
            return true;
        };
        let soft = self.soft.get(module).cloned().unwrap_or_default();
        for pre in &soft.pre {
            if !self.can_add(pre, true, steps, seen) {
                return false;
            }
        }
        // `modules.dep` lists what a module needs, what those need after
        // them: the last goes in first.
        for dependency in entry.dependencies.iter().rev() {
            let name = name_of(dependency);
            if self.installs.contains(&name) {
                return false;
            }
            if seen.insert(name.clone()) {
                steps.push(self.step(&name, dependency, optional));
            }
        }
        steps.push(self.step(module, &entry.path, optional));
        for post in &soft.post {
            if !self.can_add(post, true, steps, seen) {
                return false;
            }
        }
        true
    }

    /// The step of `name`, in `file`.
    fn step(&self, name: &str, file: &str, optional: bool) -> Step {
        let path = if Path::new(file).is_absolute() {
            PathBuf::from(file)
        } else {
            self.root.join(file)
        };
        Step {
            name: name.to_owned(),
            path,
            parameters: self
                .options
                .get(name)
                .map(|options| options.join(" "))
                .unwrap_or_default(),
            optional,
        }
    }

    /// The file of `name`, for a module inserted on its own.
    pub fn file(&self, name: &str) -> Option<PathBuf> {
        let entry = self.modules.get(&normalized(name))?;
        Some(self.step(name, &entry.path, false).path)
    }
}

/// The texts of a kernel's index of its modules.
#[derive(Clone, Copy, Debug, Default)]
pub struct KernelFiles<'a> {
    /// `modules.dep`.
    pub dep: &'a str,
    /// `modules.alias`.
    pub alias: &'a str,
    /// `modules.softdep`.
    pub softdep: &'a str,
    /// `modules.builtin`.
    pub builtin: &'a str,
}

/// The words of `line`, without a comment.
fn words(line: &str) -> Vec<&str> {
    let line = line.split('#').next().unwrap_or_default();
    line.split_whitespace().collect()
}

/// The lines of `text`, a line that ends in `\` joined with the next.
fn logical_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut joined = String::new();
    for line in text.lines() {
        if let Some(start) = line.strip_suffix('\\') {
            joined.push_str(start);
            joined.push(' ');
        } else {
            joined.push_str(line);
            lines.push(std::mem::take(&mut joined));
        }
    }
    if !joined.is_empty() {
        lines.push(joined);
    }
    lines
}

/// Whether `text` matches `pattern`, with `*`, `?` and `[...]` as the
/// shell's: what aliases are matched with.
fn matches_glob(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(b'?') => {
                p += 1;
                t += 1;
            }
            Some(b'[') => match class(pattern, p, text.get(t).copied()) {
                Some((true, end)) => {
                    p = end;
                    t += 1;
                }
                _ => match star {
                    Some((sp, st)) => {
                        p = sp + 1;
                        t = st + 1;
                        star = Some((sp, st + 1));
                    }
                    None => return false,
                },
            },
            Some(&c) if text.get(t) == Some(&c) => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    p = sp + 1;
                    t = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    pattern
        .get(p..)
        .is_some_and(|rest| rest.iter().all(|&c| c == b'*'))
}

/// Whether `c` is in the class `[...]` that starts at `start` of
/// `pattern`, and where the pattern goes on after it; `None` when the
/// class does not end.
fn class(pattern: &[u8], start: usize, c: Option<u8>) -> Option<(bool, usize)> {
    let c = c?;
    let mut at = start + 1;
    let negated = matches!(pattern.get(at), Some(b'!' | b'^'));
    if negated {
        at += 1;
    }
    let mut matched = false;
    let mut first = true;
    loop {
        let &here = pattern.get(at)?;
        if here == b']' && !first {
            return Some((matched != negated, at + 1));
        }
        first = false;
        if pattern.get(at + 1) == Some(&b'-') && pattern.get(at + 2).is_some_and(|&end| end != b']')
        {
            let end = *pattern.get(at + 2)?;
            matched |= (here..=end).contains(&c);
            at += 3;
        } else {
            matched |= here == c;
            at += 1;
        }
    }
}

#[cfg(test)]
mod tests;
