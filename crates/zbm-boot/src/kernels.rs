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
//! `sort -V` is GNU coreutils' `filevercmp` ([`filevercmp`]). Images built in
//! the build container get uutils' `sort` and mkinitcpio images BusyBox's; see
//! "Version order" in `crates/DESIGN.md` for how they differ.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use zbm_core::exec::{Command, Input, Runner, Stream};
use zbm_system::files::Files;

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

/// The length of the prefix of `s` without its file suffix: the longest
/// suffix matching `(\.[A-Za-z~][A-Za-z0-9~]*)*$`.
fn prefix_len(s: &[u8]) -> usize {
    let n = s.len();
    let mut prefix = 0;
    let mut i = 0;
    while i < n {
        i += 1;
        prefix = i;
        while i + 1 < n && s[i] == b'.' && (s[i + 1].is_ascii_alphabetic() || s[i + 1] == b'~') {
            i += 2;
            while i < n && (s[i].is_ascii_alphanumeric() || s[i] == b'~') {
                i += 1;
            }
        }
    }
    prefix
}

/// The weight of the byte at `pos` for version sorting: the end sorts before
/// everything but `~`, letters before other characters.
fn order(s: &[u8], pos: usize) -> i32 {
    match s.get(pos) {
        None => -1,
        Some(c) if c.is_ascii_digit() => 0,
        Some(c) if c.is_ascii_alphabetic() => i32::from(*c),
        Some(b'~') => -2,
        Some(c) => i32::from(*c) + 256,
    }
}

/// GNU's `verrevcmp`: compares runs of non-digits by [`order`] and runs of
/// digits by value.
fn verrevcmp(s1: &[u8], s2: &[u8]) -> i32 {
    let digit = |s: &[u8], pos: usize| s.get(pos).is_some_and(u8::is_ascii_digit);
    let (mut p1, mut p2) = (0, 0);
    while p1 < s1.len() || p2 < s2.len() {
        let mut first_diff = 0;
        while (p1 < s1.len() && !digit(s1, p1)) || (p2 < s2.len() && !digit(s2, p2)) {
            let (c1, c2) = (order(s1, p1), order(s2, p2));
            if c1 != c2 {
                return c1 - c2;
            }
            p1 += 1;
            p2 += 1;
        }
        while s1.get(p1) == Some(&b'0') {
            p1 += 1;
        }
        while s2.get(p2) == Some(&b'0') {
            p2 += 1;
        }
        while digit(s1, p1) && digit(s2, p2) {
            if first_diff == 0 {
                first_diff = i32::from(s1[p1]) - i32::from(s2[p2]);
            }
            p1 += 1;
            p2 += 1;
        }
        if digit(s1, p1) {
            return 1;
        }
        if digit(s2, p2) {
            return -1;
        }
        if first_diff != 0 {
            return first_diff;
        }
    }
    0
}

/// GNU coreutils' `filevercmp`, which `sort -V` uses.
pub fn filevercmp(a: &[u8], b: &[u8]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    // "." sorts first, then "..", then other names that start with ".", then
    // the rest.
    match (a[0] == b'.', b[0] == b'.') {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (true, true) => {
            for special in [&b"."[..], b".."] {
                match (a == special, b == special) {
                    (true, true) => return Ordering::Equal,
                    (true, false) => return Ordering::Less,
                    (false, true) => return Ordering::Greater,
                    _ => {}
                }
            }
        }
        (false, false) => {}
    }
    let (a_prefix, b_prefix) = (prefix_len(a), prefix_len(b));
    let one_pass = a_prefix == a.len() && b_prefix == b.len();
    let mut result = verrevcmp(&a[..a_prefix], &b[..b_prefix]);
    if result == 0 && !one_pass {
        result = verrevcmp(a, b);
    }
    result.cmp(&0)
}

/// Sorts as `LC_ALL=C sort -V` does: by [`filevercmp`], then by bytes.
pub fn version_sort(items: &mut [String]) {
    items.sort_by(|a, b| {
        filevercmp(a.as_bytes(), b.as_bytes()).then_with(|| a.as_bytes().cmp(b.as_bytes()))
    });
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
    use zbm_core::exec::SystemRunner;
    use zbm_system::files::RealFiles;
    use zbm_testutil::fakes::FakeFiles;
    use zbm_testutil::legacy::{Bash, LOG_STUBS, shell_quote};
    use zbm_testutil::{Rng, TempDir};

    fn sorted(items: &[&str]) -> Vec<String> {
        let mut items: Vec<String> = items.iter().map(|item| (*item).to_owned()).collect();
        version_sort(&mut items);
        items
    }

    #[test]
    fn versions_sort_by_their_numbers() {
        assert_eq!(
            sorted(&[
                "vmlinuz-6.10.1",
                "vmlinuz-6.9",
                "vmlinuz-6.1.0-13-amd64",
                "vmlinuz"
            ]),
            [
                "vmlinuz",
                "vmlinuz-6.1.0-13-amd64",
                "vmlinuz-6.9",
                "vmlinuz-6.10.1"
            ]
        );
        assert_eq!(
            sorted(&["linux-6.1.0", "linux-6.1.0~rc3", "linux-6.1.0-rc3"]),
            ["linux-6.1.0~rc3", "linux-6.1.0", "linux-6.1.0-rc3"]
        );
        assert_eq!(
            sorted(&["a", "", ".b", ".", ".."]),
            ["", ".", "..", ".b", "a"]
        );
        assert_eq!(sorted(&["x-01", "x-1"]), ["x-01", "x-1"]);
        assert_eq!(filevercmp(b"a.tar.gz", b"a.tar.gz"), Ordering::Equal);
        assert_eq!(filevercmp(b"a-1.gz", b"a-2"), Ordering::Less);
        assert_eq!(prefix_len(b"name.tar.gz"), 4);
        assert_eq!(prefix_len(b"6.1"), 3);
    }

    fn have_gnu_sort() -> bool {
        let output = SystemRunner.run(&Command::new("sort").arg("--version").stderr(Stream::Null));
        output.is_ok_and(|output| output.stdout_text().contains("GNU coreutils"))
    }

    #[test]
    fn matches_gnu_sort_on_generated_names() {
        if !have_gnu_sort() {
            eprintln!("skipped: sort is not GNU coreutils");
            return;
        }
        let mut rng = Rng::from_env(21);
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
        let input = names.join("\n") + "\n";
        let output = SystemRunner
            .run(
                &Command::new("sort")
                    .arg("-V")
                    .env("LC_ALL", "C")
                    .stdin(Input::Bytes(input.into_bytes())),
            )
            .unwrap();
        let expected: Vec<String> = output.stdout_text().lines().map(str::to_owned).collect();
        version_sort(&mut names);
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
            .success()
            .then(|| output.stdout_text().trim_end_matches('\n').to_owned())
    }

    /// The source of one function of `zfsbootmenu-core.sh`.
    fn legacy_function(name: &str) -> String {
        let core = std::fs::read_to_string(zbm_testutil::legacy::path(
            "zfsbootmenu/lib/zfsbootmenu-core.sh",
        ))
        .unwrap();
        let start = core.find(&format!("\n{name}() {{\n")).unwrap() + 1;
        let end = start + core[start..].find("\n}\n").unwrap() + 3;
        core[start..end].to_owned()
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
        for case in 0..60 {
            let dir = TempDir::new("zbm-initramfs");
            let kern = *rng.choose(&kernels);
            let kver = kern.find('-').map_or("", |hyphen| &kern[hyphen..]);
            dir.write(format!("boot/{kern}"), "");
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
                dir.write(format!("boot/{name}"), "");
            }
            let kernel = dir.join(format!("boot/{kern}"));
            let rust =
                find_initramfs(&RealFiles, &kernel).map(|path| path.to_string_lossy().into_owned());
            let listing: Vec<String> = std::fs::read_dir(dir.join("boot"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(rust, legacy_initramfs(&kernel), "case {case}: {listing:?}");
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
        for case in 0..30 {
            let dir = TempDir::new("zbm-kernels");
            std::fs::create_dir(dir.join("boot")).unwrap();
            for _ in 0..rng.below(8) {
                let name = format!("{}{}", rng.choose(&KERNEL_NAMES), rng.choose(&suffixes));
                dir.write(format!("boot/{name}"), "");
            }
            if rng.below(3) == 0 {
                std::os::unix::fs::symlink("missing", dir.join("boot/vmlinuz-dangling")).unwrap();
            }
            // The loop of find_be_kernels
            let script = format!(
                r#"mnt={}; for k in "${{mnt}}/boot"/{{{{vm,}}linu{{x,z}},kernel}}{{,-*}}; do
                     [ -e "${{k}}" ] && echo "${{k}}"; done | LC_ALL=C sort -V"#,
                shell_quote(&dir.path().to_string_lossy())
            );
            let output = Bash::new().code(&script).call("true", &[], b"");
            let legacy: Vec<String> = output.stdout_text().lines().map(str::to_owned).collect();
            assert_eq!(kernel_files(&RealFiles, dir.path()), legacy, "case {case}");
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
        let missing = zbm_testutil::FakeRunner::new();
        missing.respond_program("bash", 127, "");
        assert_eq!(bash_regex_match(&missing, "x", "x"), None);
    }
}
