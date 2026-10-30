//! Which programs have a DRM card open (C66): when another program is the
//! card's master, as Plymouth may be, the console says which while it
//! waits for the card.

use std::path::Path;

/// A program that has a file open: its process ID and its name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Holder {
    /// Its process.
    pub pid: u32,
    /// Its name, as `/proc/<pid>/comm` has it.
    pub name: String,
}

impl std::fmt::Display for Holder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({})", self.name, self.pid)
    }
}

/// The processes of `proc` (`/proc`) other than this one that have `card`
/// open, by the links of their file descriptors; in order of their IDs.
pub fn holders(proc: &Path, card: &Path) -> Vec<Holder> {
    let card = std::fs::canonicalize(card).unwrap_or_else(|_| card.to_path_buf());
    let own = std::process::id();
    let Ok(entries) = std::fs::read_dir(proc) else {
        return Vec::new();
    };
    let mut found: Vec<Holder> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let dir = entry.path();
            let opened = std::fs::read_dir(dir.join("fd"))
                .ok()?
                .filter_map(Result::ok)
                .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|target| target == card));
            (opened && pid != own).then(|| Holder {
                pid,
                name: std::fs::read_to_string(dir.join("comm"))
                    .map(|name| name.trim().to_owned())
                    .unwrap_or_default(),
            })
        })
        .collect();
    found.sort_by_key(|holder| holder.pid);
    found
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_programs_with_the_card_open_are_named() {
        let dir = std::env::temp_dir().join(format!("zbm-display-holders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let card = dir.join("card0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&card, "").unwrap();
        let process = |pid: &str, name: &str, opened: &[&Path]| {
            let fds = dir.join("proc").join(pid).join("fd");
            std::fs::create_dir_all(&fds).unwrap();
            std::fs::write(dir.join("proc").join(pid).join("comm"), format!("{name}\n")).unwrap();
            for (index, target) in opened.iter().enumerate() {
                std::os::unix::fs::symlink(target, fds.join(index.to_string())).unwrap();
            }
        };
        process("412", "plymouthd", &[Path::new("/dev/null"), &card]);
        process("37", "Xorg", &[&card]);
        process("9", "sh", &[Path::new("/dev/null")]);
        process(&std::process::id().to_string(), "zbm", &[&card]);
        std::fs::create_dir_all(dir.join("proc/self")).unwrap();

        let found = holders(&dir.join("proc"), &card);

        let names: Vec<String> = found.iter().map(ToString::to_string).collect();
        assert_eq!(names, ["Xorg (37)", "plymouthd (412)"]);
        assert_eq!(holders(&dir.join("none"), &card), []);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
