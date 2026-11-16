//! Tests of the kernel search: the version order, the kernels and their
//! initramfs images, and the search against the legacy functions.

use super::*;
use zbm_platform::exec::{Command, Input, Stream};
use zbm_platform::exec::{Runner as _, SystemRunner};
use zbm_platform::files::RealFiles;
use zbm_testkit::fakes::FakeFiles;
use zbm_testkit::legacy::{Bash, LOG_STUBS, shell_quote};
use zbm_testkit::{Rng, TempDir, parity, repo_root};

/// The order of uutils' `LC_ALL=C sort -V` on [`generated_names`],
/// recorded in the build container (see [`uutils_sort`]).
const UUTILS_ORDER: &str = include_str!("../../tests/fixtures/uutils-sort-v.txt");

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
            let path = repo_root().join("crates/zbm-bootenv/tests/fixtures/uutils-sort-v.txt");
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
        let selector: KernelSelector = property.parse().unwrap();
        select(&kernels, Some(&selector))
            .map(|kernel| kernel.kernel.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(
        select(&kernels, None).unwrap().kernel,
        "/boot/vmlinuz-6.12.9"
    );
    assert_eq!(pick("6.6"), "/boot/vmlinuz-6.6.30");
    // The last match wins, and "." matches any character.
    assert_eq!(pick("6.1"), "/boot/vmlinuz-6.12.9");
    assert_eq!(pick("6\\.1\\.0"), "/boot/vmlinuz-6.1.0");
    assert_eq!(pick("^/boot/vmlinuz-6\\.6"), "/boot/vmlinuz-6.6.30");
    assert_eq!(pick("nothing"), "/boot/vmlinuz-6.12.9");
    assert_eq!(pick(""), "/boot/vmlinuz-6.12.9");
    assert_eq!(select(&[], None), None);
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
        let (dir, _) = boot();
        let kernel = format!("/boot/{kern}");
        let mut refusals = Refusals::new("tank/ROOT/x", &kernel);
        let rust = find_initramfs(&RealFiles, dir.path(), &kernel, &mut refusals)
            .map(|path| format!("$DIR{path}"));
        assert_eq!(rust, legacy, "case {case}: {names:?}");
    }
    let mut refusals = Refusals::new("tank/ROOT/x", "/boot/vmlinuz");
    let nowhere = Path::new("/nonexistent");
    assert_eq!(
        find_initramfs(&RealFiles, nowhere, "/boot/vmlinuz", &mut refusals),
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
                std::os::unix::fs::symlink("missing", dir.join("boot/vmlinuz-dangling")).unwrap();
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
        let rust: Vec<String> = kernel_files(&RealFiles, "tank/ROOT/x", dir.path())
            .iter()
            .map(|kernel| format!("$DIR{kernel}"))
            .collect();
        assert_eq!(rust, legacy, "case {case}");
    }
}

mod links;
