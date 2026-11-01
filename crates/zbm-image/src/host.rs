//! The build host, as the plan reads it: its files, its programs on `PATH`,
//! and what its programs answer (`zbm_tools::host`).

use std::path::{Component, Path, PathBuf};

use zbm_platform::{FileKind, Files, Runner};

/// The build host.
pub struct Host<'a> {
    /// Its files.
    pub files: &'a dyn Files,
    /// Its programs.
    pub runner: &'a dyn Runner,
    /// The directories of `PATH`, in order.
    pub path: Vec<PathBuf>,
    /// A directory of a short path, which does not exist yet, for what the
    /// plan builds on the host: the GnuPG keyring.
    pub scratch: PathBuf,
    /// The XKB data the keyboard layouts are compiled from.
    pub xkb: PathBuf,
    /// The compose rules of the dead keys.
    pub compose: PathBuf,
    /// The time, in seconds since 1970, of a build without
    /// `SOURCE_DATE_EPOCH`.
    pub now: i64,
}

/// What a walk finds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Found {
    /// A directory, with its mode.
    Dir(u32),
    /// A file.
    File,
    /// A symbolic link to the path.
    Link(PathBuf),
}

impl Host<'_> {
    /// The program `name` as `command -v` finds it: a path, if it names an
    /// executable file, or the first executable file of that name in a
    /// directory of `PATH`.
    pub fn which(&self, name: &str) -> Option<PathBuf> {
        if name.contains('/') {
            return Some(PathBuf::from(name)).filter(|path| self.is_executable(path));
        }
        self.path
            .iter()
            .map(|dir| dir.join(name))
            .find(|path| self.is_executable(path))
    }

    /// Whether `path` is an executable file (`[ -x ]` of a file).
    pub fn is_executable(&self, path: &Path) -> bool {
        self.files
            .metadata(path)
            .is_ok_and(|metadata| metadata.kind == FileKind::File && metadata.is_executable())
    }

    /// Whether `path` is executable, a file or a directory (`[ -x ]`).
    pub fn is_runnable(&self, path: &Path) -> bool {
        self.files
            .metadata(path)
            .is_ok_and(|metadata| metadata.is_executable())
    }

    /// Whether `path` exists, following links (`[ -e ]`).
    pub fn exists(&self, path: &Path) -> bool {
        self.files.exists(path)
    }

    /// Whether `path` is a file (`[ -f ]`).
    pub fn is_file(&self, path: &Path) -> bool {
        self.files.is_file(path)
    }

    /// Whether `path` is a directory (`[ -d ]`).
    pub fn is_dir(&self, path: &Path) -> bool {
        self.files.is_dir(path)
    }

    /// Whether `path` is a symbolic link (`[ -L ]`).
    pub fn is_link(&self, path: &Path) -> bool {
        self.files.read_link(path).is_ok()
    }

    /// The names in the directory `dir`, in byte order; none when it is
    /// not one.
    pub fn names(&self, dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = self
            .files
            .read_dir(dir)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| name.into_string().ok())
            .collect();
        names.sort();
        names
    }

    /// The paths that the shell pattern `pattern` names, in the order the
    /// shell gives them: each component may hold `*`, which matches any
    /// characters but a leading dot.
    pub fn glob(&self, pattern: &str) -> Vec<PathBuf> {
        let mut found = vec![PathBuf::from("/")];
        for component in pattern.split('/').filter(|part| !part.is_empty()) {
            let mut next = Vec::new();
            for dir in &found {
                if component.contains('*') {
                    next.extend(
                        self.names(dir)
                            .into_iter()
                            .filter(|name| matches(component, name))
                            .map(|name| dir.join(name)),
                    );
                } else {
                    let path = dir.join(component);
                    if self.files.exists(&path) || self.is_link(&path) {
                        next.push(path);
                    }
                }
            }
            found = next;
        }
        found
    }

    /// What is below the directory `dir`, depth first and in byte order,
    /// its links not followed.
    pub fn walk(&self, dir: &Path) -> Vec<(PathBuf, Found)> {
        let mut found = Vec::new();
        for name in self.names(dir) {
            let path = dir.join(name);
            if let Ok(target) = self.files.read_link(&path) {
                found.push((path, Found::Link(target)));
                continue;
            }
            match self.files.metadata(&path) {
                Ok(metadata) if metadata.kind == FileKind::Directory => {
                    found.push((path.clone(), Found::Dir(metadata.mode)));
                    found.extend(self.walk(&path));
                }
                Ok(metadata) if metadata.kind == FileKind::File => {
                    found.push((path, Found::File));
                }
                _ => {}
            }
        }
        found
    }

    /// The text of `path`, or nothing.
    pub fn read(&self, path: &Path) -> Option<String> {
        self.files.read_to_string(path).ok()
    }
}

/// Whether `name` matches the shell pattern `pattern` of `*` and plain
/// characters.
pub fn matches(pattern: &str, name: &str) -> bool {
    if name.starts_with('.') && !pattern.starts_with('.') {
        return false;
    }
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// `path` without `.` and `..`, as far as they go.
pub fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;
    use zbm_testkit::fakes::FakeFiles;

    use super::*;

    #[test]
    fn programs_globs_and_walks_are_found_as_the_shell_finds_them() {
        let files = FakeFiles::new();
        files
            .file_with_mode("/usr/bin/zfs", "", 0o755)
            .file_with_mode("/bin/zfs", "", 0o755)
            .file("/usr/bin/plain", "")
            .file("/usr/lib/x86_64/libgcc_s.so.1", "")
            .file("/usr/lib/x86_64/.libgcc_s.so", "")
            .file("/usr/lib/i686/libgcc_s.so.1", "")
            .file("/etc/zfs/compat/a", "")
            .symlink("/etc/zfs/compat/b", "a")
            .symlink("/usr/lib/dangling", "nowhere");
        let runner = FakeRunner::new();
        let host = Host {
            files: &files,
            runner: &runner,
            path: vec!["/usr/bin".into(), "/bin".into()],
            scratch: "/tmp/scratch".into(),
            xkb: "/xkb".into(),
            compose: "/compose".into(),
            now: 0,
        };

        assert_eq!(host.which("zfs"), Some("/usr/bin/zfs".into()));
        assert_eq!(host.which("/bin/zfs"), Some("/bin/zfs".into()));
        assert_eq!(host.which("plain"), None);
        assert_eq!(host.which("/usr/bin/plain"), None);
        assert!(host.is_runnable(Path::new("/usr/bin/zfs")));
        assert_eq!(
            host.glob("/usr/lib/*/libgcc_s.so*"),
            [
                PathBuf::from("/usr/lib/i686/libgcc_s.so.1"),
                PathBuf::from("/usr/lib/x86_64/libgcc_s.so.1"),
            ]
        );
        assert_eq!(
            host.glob("/usr/lib/dangling"),
            [PathBuf::from("/usr/lib/dangling")]
        );
        assert!(host.glob("/none/*").is_empty());
        let walked = host.walk(Path::new("/etc/zfs"));
        assert_eq!(walked[0].0, Path::new("/etc/zfs/compat"));
        assert!(matches!(walked[0].1, Found::Dir(_)));
        assert_eq!(walked[1], ("/etc/zfs/compat/a".into(), Found::File));
        assert_eq!(
            walked[2],
            ("/etc/zfs/compat/b".into(), Found::Link("a".into()))
        );
        assert!(host.read(Path::new("/etc/zfs/compat/a")).is_some());
        assert!(host.is_dir(Path::new("/etc/zfs")) && host.is_file(Path::new("/usr/bin/plain")));
        assert!(host.exists(Path::new("/bin/zfs")));
    }

    #[test]
    fn patterns_match_as_the_shell_matches_them() {
        assert!(matches("clevis-*", "clevis-luks-unlock"));
        assert!(matches("*", "a"));
        assert!(!matches("*", ".hidden"));
        assert!(matches("a*b*c", "aXbYc"));
        assert!(!matches("a*b*c", "aXbY"));
        assert!(!matches("a*bc", "abc".get(..2).unwrap()));
        assert!(matches("plain", "plain") && !matches("plain", "plainer"));
        assert!(!matches("x*", "y"));
        assert!(!matches("a*b*c", "aXc"));
        assert_eq!(
            normalized(Path::new("/bin/../lib/./a")),
            Path::new("/lib/a")
        );
    }
}
