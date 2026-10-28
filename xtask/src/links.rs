//! `cargo xtask check-links [<binary>]`: the boot runtime links no C library
//! but glibc (ROADMAP.md CD1, decision 12), so that a missing or changed
//! library cannot stop a boot, and a C dependency cannot slip in through a
//! crate. `readelf -d` lists the libraries the release `zbm` needs.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::process::Program;

/// The binary that is checked when none is given, built first.
pub const DEFAULT_BINARY: &str = "target/release/zbm";

/// The libraries `zbm` may need: glibc's, its dynamic linker, and GCC's
/// unwinder (`libgcc_s`), which the Rust standard library needs to unwind a
/// panic.
pub const ALLOWED: &[&str] = &[
    "libc.so.6",
    "libm.so.6",
    "libpthread.so.0",
    "libdl.so.2",
    "librt.so.1",
    "libutil.so.1",
    "ld-linux-x86-64.so.2",
    "ld-linux-aarch64.so.1",
    "libgcc_s.so.1",
];

/// The libraries `readelf -d` says a binary needs: its `(NEEDED)` lines,
/// `Shared library: [NAME]`.
pub fn needed(dynamic: &str) -> Vec<String> {
    dynamic
        .lines()
        .filter(|line| line.contains("(NEEDED)"))
        .filter_map(|line| {
            let start = line.find('[')? + 1;
            let end = line.rfind(']')?;
            line.get(start..end).map(str::to_owned)
        })
        .collect()
}

/// The problems of a binary that needs `libraries`: one for each library
/// that is not [`ALLOWED`].
pub fn problems(libraries: &[String]) -> Vec<String> {
    libraries
        .iter()
        .filter(|library| !ALLOWED.contains(&library.as_str()))
        .map(|library| format!("zbm links {library}, a library other than glibc's"))
        .collect()
}

/// Runs `cargo xtask check-links`: builds the release `zbm` unless `binary`
/// is given, and checks the libraries it needs.
///
/// # Errors
///
/// [`Error::Check`] when it needs a library other than glibc's, and the
/// error of building it or of `readelf`.
pub fn run(root: &Path, binary: Option<&Path>) -> Result<()> {
    let binary: PathBuf = if let Some(binary) = binary {
        binary.to_owned()
    } else {
        Program::new("cargo", root)
            .args([
                "build",
                "--release",
                "--locked",
                "-p",
                "zbm",
                "--bin",
                "zbm",
            ])
            .run()?;
        root.join(DEFAULT_BINARY)
    };
    let dynamic = Program::new("readelf", root)
        .args(["-d"])
        .args([&binary])
        .output()?;
    let libraries = needed(&dynamic);
    if libraries.is_empty() {
        return Err(Error::Format(format!(
            "{}: readelf lists no library; is it a dynamic executable?",
            binary.display()
        )));
    }
    let problems = problems(&libraries);
    if !problems.is_empty() {
        return Err(Error::Check {
            check: "check-links",
            problems,
        });
    }
    println!(
        "check-links: {} needs {}",
        binary.display(),
        libraries.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `readelf -d` prints of a release `zbm`, shortened.
    const DYNAMIC: &str = "\nDynamic section at offset 0x2c1d50 contains 30 entries:\n\
  Tag        Type                         Name/Value\n\
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n\
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]\n\
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]\n\
 0x000000000000000c (INIT)               0x2a000\n";

    #[test]
    fn glibc_and_the_unwinder_are_allowed_and_nothing_else() {
        let libraries = needed(DYNAMIC);
        assert_eq!(libraries, ["libc.so.6", "libgcc_s.so.1", "libm.so.6"]);
        assert!(problems(&libraries).is_empty());

        let more = [libraries, vec!["libcryptsetup.so.12".to_owned()]].concat();
        assert_eq!(
            problems(&more),
            ["zbm links libcryptsetup.so.12, a library other than glibc's"]
        );
    }
}
