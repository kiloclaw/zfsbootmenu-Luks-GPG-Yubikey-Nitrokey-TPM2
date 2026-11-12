//! The plan of an image (ADR 0006): what goes into it, as a value, in the
//! order the builder installs it, and the manifest the builder's shim reads.
//!
//! The manifest has a line for each entry, its fields separated by tabs
//! (answer 1 of ADR 0006):
//!
//! | Line | Does |
//! | --- | --- |
//! | `dir DEST MODE` | makes a directory |
//! | `file SRC DEST MODE` | installs a file, the host's or the plan's |
//! | `link TARGET DEST` | makes a symbolic link |
//! | `hardlink TARGET DEST` | makes a hard link to a file of the image |
//! | `binary SRC DEST` | installs a program, with its libraries |
//! | `library SRC` | installs a library, with its libraries |
//! | `module NAME NEED HOST` | installs a kernel module |
//! | `rule NAME required\|optional` | installs a udev rule |
//! | `unomit NAME` | keeps dracut from omitting the driver |
//! | `hook STAGE PRIORITY SRC` | installs a dracut hook |
//! | `hostonly DEST` | marks a file host-only (dracut) |
//! | `firmware DIRS NAMES` | keeps only the files named in those directories |
//! | `info TEXT`, `warn TEXT` | says something |
//!
//! `NEED` is `required` or `optional`; `HOST` is `host`, or `any` for a
//! module that goes in even when dracut builds a host-only image. `MODE` is
//! octal, or `-` for the file's own; `unomit` is dracut's `omit_drivers`.
//! The text of a `warn` line starts with [`WARNING_MARK`].

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// What starts the text of each warning in the manifest. The shims print
/// the text as it is, in the builder's own words for a warning, and
/// `generate-zbm` finds the plan's warnings in the builder's output by this
/// mark.
pub(crate) const WARNING_MARK: &str = "[zfsbootmenu] ";

/// Whether a module or a rule must be there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Need {
    /// The build fails without it.
    Required,
    /// Left out when the build host has none.
    Optional,
}

impl Need {
    fn word(self) -> &'static str {
        match self {
            Need::Required => "required",
            Need::Optional => "optional",
        }
    }
}

/// One entry of a plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Entry {
    /// A directory.
    Dir {
        /// Where in the image.
        dest: PathBuf,
        /// Its mode.
        mode: u32,
    },
    /// A file of the build host.
    File {
        /// The build host's file.
        src: PathBuf,
        /// Where in the image.
        dest: PathBuf,
        /// Its mode; the source's when `None`.
        mode: Option<u32>,
    },
    /// A file the plan makes.
    Write {
        /// Where in the image.
        dest: PathBuf,
        /// Its mode.
        mode: u32,
        /// What it holds.
        contents: Vec<u8>,
    },
    /// A symbolic link.
    Link {
        /// What it points to.
        target: PathBuf,
        /// Where in the image.
        dest: PathBuf,
    },
    /// A hard link to a file of the image.
    HardLink {
        /// The file of the image.
        target: PathBuf,
        /// Where in the image.
        dest: PathBuf,
    },
    /// A program, with its libraries.
    Binary {
        /// The build host's program.
        src: PathBuf,
        /// Where in the image.
        dest: PathBuf,
    },
    /// A library, with its libraries.
    Library {
        /// The build host's library.
        src: PathBuf,
    },
    /// A kernel module.
    Module {
        /// Its name.
        name: String,
        /// Whether the build fails without it.
        need: Need,
        /// Whether it goes in even when dracut builds a host-only image.
        any_host: bool,
    },
    /// A udev rule.
    Rule {
        /// Its file's name.
        name: String,
        /// Whether the build fails without it.
        need: Need,
    },
    /// A driver dracut must not leave out.
    Unomit {
        /// The driver.
        name: String,
    },
    /// A dracut hook.
    Hook {
        /// The hook's stage, as `cmdline` or `pre-mount`.
        stage: String,
        /// Its priority.
        priority: u8,
        /// Its script.
        src: PathBuf,
    },
    /// A file dracut marks host-only.
    HostOnly {
        /// The file of the image.
        dest: PathBuf,
    },
    /// The firmware kept: in the directories `dirs` of the image's firmware
    /// trees, the files `names` alone, compressed or not.
    Firmware {
        /// The directories, below `lib/firmware`.
        dirs: Vec<String>,
        /// The files kept, below `lib/firmware`.
        names: Vec<String>,
    },
    /// Something to say.
    Info(String),
    /// Something to warn about.
    Warn(String),
}

impl Entry {
    /// Where the entry puts something in the image, if it has one place.
    fn dest(&self) -> Option<&Path> {
        match self {
            Entry::Dir { dest, .. }
            | Entry::File { dest, .. }
            | Entry::Write { dest, .. }
            | Entry::Link { dest, .. }
            | Entry::HardLink { dest, .. }
            | Entry::Binary { dest, .. } => Some(dest),
            _ => None,
        }
    }
}

/// The plan of an image.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Plan {
    entries: Vec<Entry>,
}

impl Plan {
    /// Adds `entry`; an earlier file, link or program at its place goes, as
    /// the later one would replace it in the image.
    pub fn add(&mut self, entry: Entry) {
        if let Some(dest) = entry.dest().filter(|_| !matches!(entry, Entry::Dir { .. })) {
            let dest = dest.to_owned();
            self.entries.retain(|old| {
                matches!(old, Entry::Dir { .. }) || old.dest() != Some(dest.as_path())
            });
        }
        self.entries.push(entry);
    }

    /// A file of the build host at `dest`, with its own mode.
    pub fn file(&mut self, src: impl Into<PathBuf>, dest: impl Into<PathBuf>) {
        self.add(Entry::File {
            src: src.into(),
            dest: dest.into(),
            mode: None,
        });
    }

    /// A file of `contents` at `dest` with `mode`.
    pub fn write(&mut self, dest: impl Into<PathBuf>, mode: u32, contents: impl Into<Vec<u8>>) {
        self.add(Entry::Write {
            dest: dest.into(),
            mode,
            contents: contents.into(),
        });
    }

    /// Appends `text` to the file the plan writes at `dest`, or writes one
    /// of `text` with `mode`.
    pub fn append(&mut self, dest: impl Into<PathBuf>, mode: u32, text: &str) {
        let dest = dest.into();
        let written = self.entries.iter_mut().find_map(|entry| match entry {
            Entry::Write {
                dest: at, contents, ..
            } if *at == dest => Some(contents),
            _ => None,
        });
        match written {
            Some(contents) => contents.extend_from_slice(text.as_bytes()),
            None => self.write(dest, mode, text),
        }
    }

    /// A program of the build host at `dest`.
    pub fn binary(&mut self, src: impl Into<PathBuf>, dest: impl Into<PathBuf>) {
        self.add(Entry::Binary {
            src: src.into(),
            dest: dest.into(),
        });
    }

    /// A symbolic link at `dest` to `target`.
    pub fn link(&mut self, target: impl Into<PathBuf>, dest: impl Into<PathBuf>) {
        self.add(Entry::Link {
            target: target.into(),
            dest: dest.into(),
        });
    }

    /// A kernel module.
    pub fn module(&mut self, name: &str, need: Need, any_host: bool) {
        self.add(Entry::Module {
            name: name.to_owned(),
            need,
            any_host,
        });
    }

    /// Something to say.
    pub fn info(&mut self, text: impl Into<String>) {
        self.add(Entry::Info(text.into()));
    }

    /// Something to warn about.
    pub fn warn(&mut self, text: impl Into<String>) {
        self.add(Entry::Warn(text.into()));
    }

    /// The entries, in order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The file the plan writes at `dest`, if any.
    pub fn written(&self, dest: &Path) -> Option<&[u8]> {
        self.entries.iter().find_map(|entry| match entry {
            Entry::Write {
                dest: at, contents, ..
            } if at == dest => Some(contents.as_slice()),
            _ => None,
        })
    }

    /// Whether the plan puts something at `dest`.
    pub fn has(&self, dest: &Path) -> bool {
        self.entries.iter().any(|entry| entry.dest() == Some(dest))
    }

    /// The manifest, the files the plan writes named by `staged`: where the
    /// shim finds each.
    pub fn manifest(&self, staged: &dyn Fn(usize, &Path) -> PathBuf) -> String {
        let mut text = String::new();
        let path = |path: &Path| path.display().to_string();
        for (index, entry) in self.entries.iter().enumerate() {
            let line = match entry {
                Entry::Dir { dest, mode } => format!("dir\t{}\t{mode:04o}", path(dest)),
                Entry::File { src, dest, mode } => {
                    let mode = mode.map_or_else(|| "-".to_owned(), |mode| format!("{mode:04o}"));
                    format!("file\t{}\t{}\t{mode}", path(src), path(dest))
                }
                Entry::Write { dest, mode, .. } => {
                    format!(
                        "file\t{}\t{}\t{mode:04o}",
                        path(&staged(index, dest)),
                        path(dest)
                    )
                }
                Entry::Link { target, dest } => format!("link\t{}\t{}", path(target), path(dest)),
                Entry::HardLink { target, dest } => {
                    format!("hardlink\t{}\t{}", path(target), path(dest))
                }
                Entry::Binary { src, dest } => format!("binary\t{}\t{}", path(src), path(dest)),
                Entry::Library { src } => format!("library\t{}", path(src)),
                Entry::Module {
                    name,
                    need,
                    any_host,
                } => {
                    let host = if *any_host { "any" } else { "host" };
                    format!("module\t{name}\t{}\t{host}", need.word())
                }
                Entry::Rule { name, need } => format!("rule\t{name}\t{}", need.word()),
                Entry::Unomit { name } => format!("unomit\t{name}"),
                Entry::Hook {
                    stage,
                    priority,
                    src,
                } => format!("hook\t{stage}\t{priority:02}\t{}", path(src)),
                Entry::HostOnly { dest } => format!("hostonly\t{}", path(dest)),
                Entry::Firmware { dirs, names } => {
                    format!("firmware\t{}\t{}", dirs.join(" "), names.join(" "))
                }
                Entry::Info(text) => format!("info\t{}", one_line(text)),
                Entry::Warn(text) => format!("warn\t{WARNING_MARK}{}", one_line(text)),
            };
            let _ = writeln!(text, "{line}");
        }
        text
    }

    /// The plan as the golden files keep it: the manifest, each written file
    /// followed by its contents, indented, binary contents as their size.
    pub fn golden(&self) -> String {
        let manifest =
            self.manifest(&|_, dest| Path::new("@").join(dest.strip_prefix("/").unwrap_or(dest)));
        let mut text = String::new();
        for (line, entry) in manifest.lines().zip(&self.entries) {
            let _ = writeln!(text, "{line}");
            if let Entry::Write { contents, .. } = entry {
                match std::str::from_utf8(contents) {
                    Ok(contents) => {
                        for line in contents.lines() {
                            let _ = writeln!(text, "    | {line}");
                        }
                    }
                    Err(_) => {
                        let _ = writeln!(text, "    | ({} bytes)", contents.len());
                    }
                }
            }
        }
        text
    }
}

/// `text` on one line: a tab or a newline would end its field.
fn one_line(text: &str) -> String {
    text.replace(['\t', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_manifest_has_a_line_for_each_entry_and_later_files_replace_earlier_ones() {
        let mut plan = Plan::default();
        plan.add(Entry::Dir {
            dest: "/etc/zfsbootmenu".into(),
            mode: 0o755,
        });
        plan.file("/usr/share/zfsbootmenu/lib/a.sh", "/lib/a.sh");
        plan.write("/lib/a.sh", 0o755, "#!/bin/sh\n");
        plan.binary("/usr/bin/zfs", "/usr/bin/zfs");
        plan.link("/libexec/zbm", "/bin/zluks");
        plan.add(Entry::HardLink {
            target: "/usr/bin/coreutils".into(),
            dest: "/usr/bin/cat".into(),
        });
        plan.add(Entry::Library {
            src: "/usr/lib/libfido2.so.1".into(),
        });
        plan.module("zfs", Need::Required, false);
        plan.module("i915", Need::Required, true);
        plan.add(Entry::Rule {
            name: "90-zfs.rules".into(),
            need: Need::Optional,
        });
        plan.add(Entry::Unomit { name: "drm".into() });
        plan.add(Entry::Hook {
            stage: "cmdline".into(),
            priority: 95,
            src: "/m/x.sh".into(),
        });
        plan.add(Entry::HostOnly {
            dest: "/etc/hostid".into(),
        });
        plan.add(Entry::Firmware {
            dirs: vec!["i915".into()],
            names: vec!["i915/a.bin".into(), "i915/b.bin".into()],
        });
        plan.info("two\tfields");
        plan.warn("a\nb");

        let manifest = plan.manifest(&|index, _| PathBuf::from(format!("/stage/{index}")));

        assert_eq!(
            manifest,
            "dir\t/etc/zfsbootmenu\t0755\n\
             file\t/stage/1\t/lib/a.sh\t0755\n\
             binary\t/usr/bin/zfs\t/usr/bin/zfs\n\
             link\t/libexec/zbm\t/bin/zluks\n\
             hardlink\t/usr/bin/coreutils\t/usr/bin/cat\n\
             library\t/usr/lib/libfido2.so.1\n\
             module\tzfs\trequired\thost\n\
             module\ti915\trequired\tany\n\
             rule\t90-zfs.rules\toptional\n\
             unomit\tdrm\n\
             hook\tcmdline\t95\t/m/x.sh\n\
             hostonly\t/etc/hostid\n\
             firmware\ti915\ti915/a.bin i915/b.bin\n\
             info\ttwo fields\n\
             warn\t[zfsbootmenu] a b\n"
        );
        assert_eq!(
            plan.written(Path::new("/lib/a.sh")),
            Some(&b"#!/bin/sh\n"[..])
        );
        assert!(plan.has(Path::new("/bin/zluks")) && !plan.has(Path::new("/bin/none")));
        let golden = plan.golden();
        assert!(
            golden.contains("file\t@/lib/a.sh\t/lib/a.sh\t0755\n    | #!/bin/sh\n"),
            "{golden}"
        );
        let mut binary = Plan::default();
        binary.file("/etc/hostid", "/etc/hostid");
        binary.add(Entry::File {
            src: "/x".into(),
            dest: "/y".into(),
            mode: Some(0o600),
        });
        binary.write("/etc/hostid", 0o644, vec![0xff, 0xfe]);
        assert_eq!(
            binary.golden(),
            "file\t/x\t/y\t0600\nfile\t@/etc/hostid\t/etc/hostid\t0644\n    | (2 bytes)\n"
        );
    }
}
