//! The tests compile layouts from the machine's XKB data and compose rules
//! (`xkb-data` and `libx11-data` on Debian and Ubuntu).

use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeFiles;

use super::*;

/// Runs `keyboard-layouts` with `args`; its status, output and errors.
fn command(args: &[&str]) -> (i32, String, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
    let status = run(&args, &mut io);
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    (status, text(stdout), text(stderr))
}

#[test]
fn arguments_name_a_destination_and_layouts() {
    let request = parse(&["--xkb".into(), "/x".into(), "/out".into(), "de".into()]).unwrap();

    assert_eq!(request.xkb, PathBuf::from("/x"));
    assert_eq!(request.compose, PathBuf::from(COMPOSE));
    assert_eq!(request.destination, PathBuf::from("/out"));
    assert_eq!(request.layouts, ["de"]);
    for wrong in [&["/out"][..], &["--xkb"], &["--other", "/out", "de"], &[]] {
        let wrong: Vec<String> = wrong.iter().map(|&arg| arg.to_owned()).collect();
        assert_eq!(parse(&wrong), None, "{wrong:?}");
    }
    let (status, _, stderr) = command(&["/out"]);
    assert_eq!(status, crate::USAGE);
    assert!(
        stderr.starts_with("Usage: zbm keyboard-layouts"),
        "{stderr}"
    );
}

#[test]
fn the_command_compiles_the_layouts_and_the_dead_keys() {
    let out = zbm_testkit::TempDir::new("keyboard-layouts");
    let destination = out.path().join("layouts");
    let destination = destination.to_str().unwrap();

    let (status, stdout, stderr) = command(&[destination, "us", "de", "zz", "us"]);

    assert_eq!(status, 1, "{stderr}");
    assert!(stdout.starts_with("us: "), "{stdout}");
    assert_eq!(stdout.matches("us: ").count(), 1, "{stdout}");
    assert!(stdout.contains("\ndead-keys: "), "{stdout}");
    assert_eq!(stderr, "keyboard-layouts: zz: no such keyboard layout\n");
    let files = RealFiles;
    assert_eq!(available(&files, Path::new(destination)), ["us", "de"]);
    let mut german = load(&files, Path::new(destination), "de").unwrap();
    german.key(41, 1);
    german.key(41, 0);
    assert_eq!(german.key(18, 1).unwrap().char, Some('ê'));
    let (status, _, _) = command(&[destination, "us"]);
    assert_eq!(status, 0);
}

#[test]
fn missing_compose_rules_leave_dead_keys_out() {
    let files = FakeFiles::new();
    let request = Request {
        xkb: PathBuf::from(XKB),
        compose: PathBuf::from("/nowhere/Compose"),
        destination: PathBuf::from("/out"),
        layouts: vec!["fr".to_owned()],
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };

    assert!(compile_all(&files, &request, &mut io).unwrap());

    let stderr = String::from_utf8(stderr).unwrap();
    let mut io = Io {
        stdout: &mut Vec::new(),
        stderr: &mut Vec::new(),
    };
    assert!(stderr.contains("/nowhere/Compose"), "{stderr}");
    assert!(
        stderr.ends_with("; dead keys will type nothing\n"),
        "{stderr}"
    );
    assert_eq!(files.contents("/out/dead-keys"), None);
    let mut french = load(&files, Path::new("/out"), "fr").unwrap();
    assert_eq!(french.key(16, 1).unwrap().char, Some('a'));
    files.fail("/out", io::ErrorKind::PermissionDenied);
    assert!(compile_all(&files, &request, &mut io).is_err());
}

#[test]
fn layouts_the_image_lacks_do_not_load() {
    let files = FakeFiles::new();
    files
        .file("/layouts/de.xkb", b"xkb_keymap {")
        .file("/layouts/layouts", b"de\n../fr\nfr\n");

    assert_eq!(available(&files, Path::new("/layouts")), ["de", "fr"]);
    assert_eq!(
        available(&files, Path::new("/nowhere")),
        Vec::<String>::new()
    );
    let missing = load(&files, Path::new("/layouts"), "fr").unwrap_err();
    assert!(missing.starts_with("fr: not in this image"), "{missing}");
    let broken = load(&files, Path::new("/layouts"), "de").unwrap_err();
    assert!(broken.starts_with("de: "), "{broken}");
    let named = load(&files, Path::new("/layouts"), "../de").unwrap_err();
    assert!(named.contains("not a layout name"), "{named}");
}

/// An image's layouts directory with `names` compiled, in that order.
fn image_layouts(files: &FakeFiles, names: &[&str]) {
    for name in names {
        let keymap = compile::keymap(Path::new(XKB), name).unwrap();
        files.file(keymap_path(Path::new("/layouts"), name), keymap);
    }
    files.file("/layouts/layouts", format!("{}\n", names.join("\n")));
}

#[test]
fn the_console_starts_with_the_layout_asked_for_or_the_first() {
    let files = FakeFiles::new();
    let directory = Path::new("/layouts");
    let mut warnings = Vec::new();
    let mut warn = |text: String| warnings.push(text);

    assert!(first(&files, directory, None, &mut warn).is_none());
    image_layouts(&files, &["de", "fr"]);
    let asked = first(&files, directory, Some("fr"), &mut warn).unwrap();
    let missing = first(&files, directory, Some("gb"), &mut warn).unwrap();
    let default = first(&files, directory, None, &mut warn).unwrap();

    assert_eq!(
        (asked.name(), missing.name(), default.name()),
        ("fr", "de", "de")
    );
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("gb: not in this image"),
        "{warnings:?}"
    );
    assert!(
        warnings[0].ends_with("; the image's first layout instead"),
        "{warnings:?}"
    );
}

/// Runs `zbm console layout` with `args`; its status, output and errors.
fn switching(files: &dyn Files, base: &Path, args: &[&str]) -> (i32, String, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
    let status = switch(files, Path::new("/layouts"), base, &args, &mut io);
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    (status, text(stdout), text(stderr))
}

#[test]
fn switching_names_the_layouts_and_needs_a_console() {
    let files = FakeFiles::new();
    files.file("/layouts/layouts", "de\nfr\n");
    let base = zbm_testkit::TempDir::new("zbm-layout-switch");

    let shown = switching(&files, base.path(), &[]);
    let missing = switching(&files, base.path(), &["gb"]);
    let alone = switching(&files, base.path(), &["fr"]);
    let wrong = switching(&files, base.path(), &["fr", "de"]);

    let none = "keyboard: none, no console reads the keyboards\nlayouts: de fr\n";
    assert_eq!(shown, (0, none.to_owned(), String::new()));
    let not_here = "zbm console: gb: not in this image, which has: de fr\n";
    assert_eq!(missing, (1, String::new(), not_here.to_owned()));
    let no_console = "zbm console: no console on the display reads the keyboards\n";
    assert_eq!(alone, (1, String::new(), no_console.to_owned()));
    assert_eq!(wrong.0, crate::USAGE);
    let state = zbm_state::State::new(&RealFiles, base.path());
    state.set_console_layout("de").unwrap();
    assert_eq!(switching(&RealFiles, base.path(), &[]).0, 0);
    let (_, current, _) = switching(&RealFiles, base.path(), &[]);
    assert!(current.starts_with("keyboard: de\nlayouts: "), "{current}");
}
