//! Kernels and initramfs images of a boot environment.
//!
//! The Rust counterparts of `find_be_kernels`, `find_be_initramfs` and
//! `select_kernel`: the kernels are the files `/boot/{vmlinux,vmlinuz,linux,
//! linuz,kernel}` and `/boot/<name>-*` of the mounted boot environment, in the
//! order `sort -V` gives their paths, each with the first initramfs found by
//! the legacy search; kernels without an initramfs are left out. The default
//! kernel is the last one, or the last one whose path matches the regular
//! expression in `org.zfsbootmenu:kernel`.
//!
//! The order is that of uutils' `sort -V` ([`version_sort`], in
//! `zbm_types::version`, from `uucore`), the coreutils of ZFSBootMenu's
//! images; see "Version order" in `crates/DESIGN.md` for how GNU coreutils
//! and BusyBox differ.

use std::path::{Path, PathBuf};

use zbm_platform::exec::{Command, Input, Runner, Stream};
use zbm_platform::files::Files;
pub use zbm_types::version::version_sort;

/// The kernel file names `find_be_kernels` looks for in `/boot`, each also
/// followed by `-` and anything.
const KERNEL_NAMES: [&str; 5] = ["vmlinux", "vmlinuz", "linux", "linuz", "kernel"];

/// A kernel of a boot environment, with paths relative to its root, as a line
/// of `environments/<dataset>/kernels` holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Kernel {
    /// The boot environment.
    pub dataset: String,
    /// The kernel, such as `/boot/vmlinuz-6.12.9_1`.
    pub kernel: String,
    /// Its initramfs, such as `/boot/initramfs-6.12.9_1.img`.
    pub initramfs: String,
}

impl Kernel {
    /// The line of the kernel list: the three fields separated by tabs.
    pub fn line(&self) -> String {
        format!("{}\t{}\t{}\n", self.dataset, self.kernel, self.initramfs)
    }

    /// A kernel from a line of the kernel list, read as `IFS=$'\t' read -r fs
    /// kernel initramfs` reads it: the third field takes the rest of the line.
    pub fn parse(line: &str) -> Kernel {
        let line = line.strip_suffix('\n').unwrap_or(line);
        let mut fields = line.splitn(3, '\t');
        let mut next = || fields.next().unwrap_or_default().to_owned();
        Kernel {
            dataset: next(),
            kernel: next(),
            initramfs: next(),
        }
    }
}

/// The kernel files of a boot environment mounted at `mnt`, as full paths in
/// version order: what the glob of `find_be_kernels` finds and `[ -e ]`
/// accepts, piped through `sort -V`.
pub fn kernel_files(files: &dyn Files, mnt: &Path) -> Vec<String> {
    let boot = mnt.join("boot");
    let names = files.read_dir(&boot).unwrap_or_default();
    let mut found: Vec<String> = names
        .iter()
        .filter_map(|name| name.to_str())
        .filter(|name| {
            KERNEL_NAMES.iter().any(|base| {
                *name == *base
                    || name
                        .strip_prefix(base)
                        .is_some_and(|rest| rest.starts_with('-'))
            })
        })
        .map(|name| boot.join(name))
        .filter(|path| files.exists(path))
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    version_sort(&mut found);
    found
}

/// The initramfs of the kernel at `kernel` (a full path), as
/// `find_be_initramfs` searches for it, or `None`.
pub fn find_initramfs(files: &dyn Files, kernel: &Path) -> Option<PathBuf> {
    if !files.exists(kernel) {
        return None;
    }
    let kern = kernel.file_name()?.to_str()?;
    let dir = kernel.parent()?;
    // The "version": from the first hyphen on, or empty
    let kver = kern.find('-').map_or("", |hyphen| &kern[hyphen..]);
    let candidates = [
        format!("initramfs-{kern}.img"),
        format!("initramfs{kver}.img"),
        format!("initrd.img-{kern}"),
        format!("initrd.img{kver}"),
        format!("initramfs-{kern}"),
        format!("initramfs{kver}"),
    ];
    let mut names: Vec<String> = candidates.to_vec();
    let compressions = ["", ".gz", ".bz2", ".xz", ".lzma", ".lz4", ".lzo", ".zstd"];
    for img in [".img", ""] {
        for compression in compressions {
            let ext = format!("{img}{compression}");
            for prefix in ["initramfs", "initrd"] {
                for label in [kern, kver] {
                    names.push(format!("{prefix}{label}{ext}"));
                    names.push(format!("{prefix}{ext}{label}"));
                }
            }
        }
    }
    names
        .into_iter()
        .map(|name| dir.join(name))
        .find(|path| files.exists(path))
}

/// A path relative to the mount point `mnt`, with one leading `/`, as
/// `find_be_kernels` strips the mount point.
fn relative(path: &str, mnt: &str) -> String {
    let rest = path.strip_prefix(mnt).unwrap_or(path);
    format!("/{}", rest.strip_prefix('/').unwrap_or(rest))
}

/// What `find_be_kernels` finds in a boot environment mounted at `mnt`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Discovery {
    /// The kernels with an initramfs, in version order.
    pub kernels: Vec<Kernel>,
    /// The `<kernel>.kcl` files next to the kernels (whether or not they have an
    /// initramfs), with the kernel each belongs to, to be copied to the command
    /// line cache.
    pub kcl_files: Vec<(String, PathBuf)>,
}

/// Finds the kernels of `dataset`, mounted at `mnt`.
pub fn discover(files: &dyn Files, dataset: &str, mnt: &Path) -> Discovery {
    let mnt_text = mnt.to_string_lossy();
    let mut discovery = Discovery::default();
    for path in kernel_files(files, mnt) {
        let kernel = relative(&path, &mnt_text);
        if let Some(initramfs) = find_initramfs(files, Path::new(&path)) {
            discovery.kernels.push(Kernel {
                dataset: dataset.to_owned(),
                kernel: kernel.clone(),
                initramfs: relative(&initramfs.to_string_lossy(), &mnt_text),
            });
        }
        // "${mnt}/${kpath}.kcl"
        let kcl = PathBuf::from(format!("{mnt_text}/{kernel}.kcl"));
        if files.exists(&kcl) {
            discovery.kcl_files.push((kernel, kcl));
        }
    }
    discovery
}

/// Whether `text` matches the extended regular expression `pattern`, as
/// Bash's `[[ text =~ pattern ]]` decides it. Bash runs the match, so the
/// regular expressions mean exactly what they mean to the legacy code; `None`
/// if Bash cannot be run.
pub fn bash_regex_match(runner: &dyn Runner, text: &str, pattern: &str) -> Option<bool> {
    let command = Command::new("bash")
        .args(["-c", "[[ $1 =~ $2 ]]", "bash", text, pattern])
        .stdin(Input::Null)
        .stdout(Stream::Null)
        .stderr(Stream::Null)
        .env("LC_ALL", "C");
    let output = runner.run(&command).ok()?;
    match output.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        // 2: the pattern is not a valid regular expression, which matches nothing
        Some(2) => Some(false),
        _ => None,
    }
}

/// The kernel `select_kernel` chooses: the last one, or the last one whose
/// kernel path matches `property`, the value of `org.zfsbootmenu:kernel` (`-`
/// when unset). `None` without kernels.
pub fn select<'a>(
    runner: &dyn Runner,
    kernels: &'a [Kernel],
    property: &str,
) -> Option<&'a Kernel> {
    let mut chosen = kernels.last()?;
    if property != "-" {
        for kernel in kernels {
            if bash_regex_match(runner, &kernel.kernel, property) == Some(true) {
                chosen = kernel;
            }
        }
    }
    Some(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_platform::exec::SystemRunner;
    use zbm_platform::files::RealFiles;
    use zbm_testkit::fakes::FakeFiles;
    use zbm_testkit::legacy::{Bash, LOG_STUBS, shell_quote};
    use zbm_testkit::{Rng, TempDir, parity, repo_root};

    /// The order of uutils' `LC_ALL=C sort -V` on [`generated_names`],
    /// recorded in the build container (see [`uutils_sort`]).
    const UUTILS_ORDER: &str = include_str!("../tests/fixtures/uutils-sort-v.txt");

    /// Kernel-like paths, in byte order: random ones and real ones.
    fn generated_names() -> Vec<String> {
        let mut rng = Rng::new(21);
        let alphabet = ['0', '1', '9', '.', '-', '_', '~', 'a', 'z', 'X', '+', '/'];
        let mut names: Vec<String> = (0..2000)
            .map(|_| {
                let base = rng.choose(&KERNEL_NAMES);
                format!("/mnt/boot/{base}-{}", rng.string(&alphabet, 12))
            })
            .collect();
        names.extend(
            [
                "6.1.0-13-amd64",
                "6.12.9_1",
                "6.6.30-lts",
                "6.8.0-45-generic",
                "6.10.1-arch1-1",
            ]
            .iter()
            .map(|version| format!("/mnt/boot/vmlinuz-{version}")),
        );
        names.sort();
        names.dedup();
        names
    }

    /// The order the host's `sort -V` gives `names`, if it is uutils'.
    fn uutils_sort(names: &[String]) -> Option<Vec<String>> {
        let version = SystemRunner
            .run(&Command::new("sort").arg("--version").stderr(Stream::Null))
            .ok()?;
        if !version.stdout_text().contains("uutils coreutils") {
            return None;
        }
        let input = names.join("\n") + "\n";
        let sort = Command::new("sort")
            .arg("-V")
            .env("LC_ALL", "C")
            .stdin(Input::Bytes(input.into_bytes()));
        let output = SystemRunner.run(&sort).ok()?;
        Some(output.stdout_text().lines().map(str::to_owned).collect())
    }

    /// GNU `sort -V` puts `kernel-0` first; uutils, the images' coreutils,
    /// puts it last. No distribution names kernels like this.
    #[test]
    fn the_order_is_that_of_uutils_where_gnu_differs() {
        let mut names = vec!["kernel-0".to_owned(), "kernel-.X".to_owned()];
        version_sort(&mut names);
        assert_eq!(names, ["kernel-.X", "kernel-0"]);
    }

    /// The order of the images' `sort -V`, as recorded. Where the host's
    /// `sort` is uutils, as in the build container, the recording is checked
    /// against it; `ZBM_RECORD_UUTILS_ORDER=1` rewrites it there.
    #[test]
    fn matches_uutils_sort_on_generated_names() {
        let mut names = generated_names();
        if let Some(live) = uutils_sort(&names) {
            if std::env::var_os("ZBM_RECORD_UUTILS_ORDER").is_some() {
                let path = repo_root().join("crates/zbm-boot/tests/fixtures/uutils-sort-v.txt");
                std::fs::write(path, live.join("\n") + "\n").unwrap();
                return;
            }
            assert_eq!(live.join("\n") + "\n", UUTILS_ORDER);
        }
        version_sort(&mut names);
        let expected: Vec<&str> = UUTILS_ORDER.lines().collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn kernels_and_their_initramfs() {
        let files = FakeFiles::new();
        let mnt = Path::new("/zfsbootmenu/environments/tank/ROOT/void/mnt");
        for name in [
            "vmlinuz-6.12.9_1",
            "initramfs-6.12.9_1.img",
            "vmlinuz-6.6.30_1",
            "initramfs-6.6.30_1.img",
            "vmlinuz-6.1.0",
            "config-6.1.0",
            "linux-lts",
            "initrd.img-linux-lts",
            "vmlinuz.old",
            "kernel",
        ] {
            files.file(mnt.join("boot").join(name), "");
        }
        files.file(mnt.join("boot/vmlinuz-6.12.9_1.kcl"), "quiet\n");
        files.file(mnt.join("boot/vmlinuz-6.1.0.kcl"), "ro\n");
        let discovery = discover(&files, "tank/ROOT/void", mnt);
        let kernels: Vec<(String, String)> = discovery
            .kernels
            .iter()
            .map(|kernel| (kernel.kernel.clone(), kernel.initramfs.clone()))
            .collect();
        assert_eq!(
            kernels,
            [
                (
                    "/boot/linux-lts".to_owned(),
                    "/boot/initrd.img-linux-lts".to_owned()
                ),
                (
                    "/boot/vmlinuz-6.6.30_1".to_owned(),
                    "/boot/initramfs-6.6.30_1.img".to_owned()
                ),
                (
                    "/boot/vmlinuz-6.12.9_1".to_owned(),
                    "/boot/initramfs-6.12.9_1.img".to_owned()
                ),
            ]
        );
        // vmlinuz-6.1.0 has no initramfs, but its command line is still cached.
        let kcl: Vec<&str> = discovery
            .kcl_files
            .iter()
            .map(|(kernel, _)| kernel.as_str())
            .collect();
        assert_eq!(kcl, ["/boot/vmlinuz-6.1.0", "/boot/vmlinuz-6.12.9_1"]);
        assert_eq!(
            discovery.kernels[2].line(),
            "tank/ROOT/void\t/boot/vmlinuz-6.12.9_1\t/boot/initramfs-6.12.9_1.img\n"
        );
        assert_eq!(
            Kernel::parse(&discovery.kernels[2].line()),
            discovery.kernels[2]
        );
        assert_eq!(Kernel::parse("a\tb\tc\td").initramfs, "c\td");
        assert!(
            discover(&files, "x", Path::new("/missing"))
                .kernels
                .is_empty()
        );
    }

    #[test]
    fn kernel_selection_uses_the_property() {
        let kernels: Vec<Kernel> = [
            "/boot/vmlinuz-6.1.0",
            "/boot/vmlinuz-6.6.30",
            "/boot/vmlinuz-6.12.9",
        ]
        .iter()
        .map(|kernel| Kernel {
            dataset: "tank/ROOT/void".to_owned(),
            kernel: (*kernel).to_owned(),
            initramfs: "/boot/initramfs.img".to_owned(),
        })
        .collect();
        let pick = |property: &str| {
            select(&SystemRunner, &kernels, property)
                .map(|kernel| kernel.kernel.as_str())
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(pick("-"), "/boot/vmlinuz-6.12.9");
        assert_eq!(pick("6.6"), "/boot/vmlinuz-6.6.30");
        // The last match wins, and "." matches any character.
        assert_eq!(pick("6.1"), "/boot/vmlinuz-6.12.9");
        assert_eq!(pick("6\\.1\\.0"), "/boot/vmlinuz-6.1.0");
        assert_eq!(pick("^/boot/vmlinuz-6\\.6"), "/boot/vmlinuz-6.6.30");
        assert_eq!(pick("nothing"), "/boot/vmlinuz-6.12.9");
        assert_eq!(pick("("), "/boot/vmlinuz-6.12.9");
        assert_eq!(pick(""), "/boot/vmlinuz-6.12.9");
        assert_eq!(select(&SystemRunner, &[], "-"), None);
    }

    /// Runs the legacy `find_be_initramfs` on a real file.
    fn legacy_initramfs(kernel: &Path) -> Option<String> {
        let output = Bash::new()
            .code(LOG_STUBS)
            .code(&legacy_function("find_be_initramfs"))
            .call("find_be_initramfs", &[&kernel.to_string_lossy()], b"");
        output
            .status
            .is_success()
            .then(|| output.stdout_text().trim_end_matches('\n').to_owned())
    }

    /// The source of one function of `zfsbootmenu-core.sh`.
    fn legacy_function(name: &str) -> String {
        let core = std::fs::read_to_string(zbm_testkit::legacy::path(
            "zfsbootmenu/lib/zfsbootmenu-core.sh",
        ))
        .unwrap();
        let start = core.find(&format!("\n{name}() {{\n")).unwrap() + 1;
        let end = start + core[start..].find("\n}\n").unwrap() + 3;
        core[start..end].to_owned()
    }

    /// `path` with the directory written as `$DIR`, so that it does not
    /// depend on where the test ran.
    fn without_dir(path: &str, dir: &TempDir) -> String {
        path.replace(&*dir.path().to_string_lossy(), "$DIR")
    }

    #[test]
    fn initramfs_search_matches_the_legacy_function() {
        let mut rng = Rng::from_env(22);
        let kernels = [
            "vmlinuz-6.12.9_1",
            "vmlinuz",
            "linux-lts",
            "vmlinuz-6.1.0-13-amd64",
        ];
        let decorations = ["initramfs", "initrd", "initrd.img", "initramfs-"];
        let extensions = ["", ".img", ".gz", ".img.zstd", ".xz", ".lz4"];
        let mut parity = parity!("initramfs_search_matches_the_legacy_function");
        for case in 0..60 {
            let kern = *rng.choose(&kernels);
            let kver = kern.find('-').map_or("", |hyphen| &kern[hyphen..]);
            let mut names = vec![kern.to_owned()];
            for _ in 0..rng.below(4) {
                let label = if rng.below(2) == 0 { kern } else { kver };
                let name = if rng.below(2) == 0 {
                    format!(
                        "{}{label}{}",
                        rng.choose(&decorations),
                        rng.choose(&extensions)
                    )
                } else {
                    format!(
                        "{}{}{label}",
                        rng.choose(&decorations),
                        rng.choose(&extensions)
                    )
                };
                names.push(name);
            }
            // The directory with the names, and the kernel in it.
            let boot = || {
                let dir = TempDir::new("zbm-initramfs");
                for name in &names {
                    dir.write(format!("boot/{name}"), "");
                }
                let kernel = dir.join(format!("boot/{kern}"));
                (dir, kernel)
            };
            let legacy: Option<String> = parity.legacy(format!("{names:?}"), || {
                let (dir, kernel) = boot();
                legacy_initramfs(&kernel).map(|path| without_dir(&path, &dir))
            });
            let (dir, kernel) = boot();
            let rust = find_initramfs(&RealFiles, &kernel)
                .map(|path| without_dir(&path.to_string_lossy(), &dir));
            assert_eq!(rust, legacy, "case {case}: {names:?}");
        }
        assert_eq!(
            find_initramfs(&RealFiles, Path::new("/nonexistent/vmlinuz")),
            None
        );
    }

    #[test]
    fn kernel_listing_matches_the_legacy_pipeline() {
        let mut rng = Rng::from_env(23);
        let suffixes = [
            "", "-6.1", "-6.10", "-6.9.1", "-lts", "-6.1~rc1", ".old", "-0", "-x-2",
        ];
        let mut parity = parity!("kernel_listing_matches_the_legacy_pipeline");
        for case in 0..30 {
            let names: Vec<String> = (0..rng.below(8))
                .map(|_| format!("{}{}", rng.choose(&KERNEL_NAMES), rng.choose(&suffixes)))
                .collect();
            let dangling = rng.below(3) == 0;
            let boot = || {
                let dir = TempDir::new("zbm-kernels");
                std::fs::create_dir(dir.join("boot")).unwrap();
                for name in &names {
                    dir.write(format!("boot/{name}"), "");
                }
                if dangling {
                    std::os::unix::fs::symlink("missing", dir.join("boot/vmlinuz-dangling"))
                        .unwrap();
                }
                dir
            };
            let input = format!("{names:?}, dangling link {dangling}");
            let legacy: Vec<String> = parity.legacy(input, || {
                let dir = boot();
                // The loop of find_be_kernels
                let script = format!(
                    r#"mnt={}; for k in "${{mnt}}/boot"/{{{{vm,}}linu{{x,z}},kernel}}{{,-*}}; do
                         [ -e "${{k}}" ] && echo "${{k}}"; done | LC_ALL=C sort -V"#,
                    shell_quote(&dir.path().to_string_lossy())
                );
                let output = Bash::new().code(&script).call("true", &[], b"");
                output
                    .stdout_text()
                    .lines()
                    .map(|line| without_dir(line, &dir))
                    .collect()
            });
            let dir = boot();
            let rust: Vec<String> = kernel_files(&RealFiles, dir.path())
                .iter()
                .map(|path| without_dir(path, &dir))
                .collect();
            assert_eq!(rust, legacy, "case {case}");
        }
    }

    #[test]
    fn regular_expressions_are_bash_ones() {
        let runner = SystemRunner;
        assert_eq!(
            bash_regex_match(&runner, "/boot/vmlinuz-6.1", "6\\.1$"),
            Some(true)
        );
        assert_eq!(
            bash_regex_match(&runner, "/boot/vmlinuz-6.1", "^6"),
            Some(false)
        );
        assert_eq!(
            bash_regex_match(&runner, "a b", "a[[:space:]]b"),
            Some(true)
        );
        assert_eq!(bash_regex_match(&runner, "x", "("), Some(false));
        let missing = zbm_testkit::FakeRunner::new();
        missing.respond_program("bash", 127, "");
        assert_eq!(bash_regex_match(&missing, "x", "x"), None);
    }
}
