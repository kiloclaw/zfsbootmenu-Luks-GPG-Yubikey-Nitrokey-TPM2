use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pretty_assertions::assert_eq;
use zbm_platform::exec::{Output, Status};
use zbm_platform::files::Files;
use zbm_testkit::fakes::FakeSystem;

use super::choice::{Candidate, Density, MIN_COLUMNS, Request, order};
use super::edid::Edid;
use super::edid::tests::edid;
use super::font::Cell;
use super::font::tests::psf2;
use super::{FONTS, Outcome, Settings, run};

/// The Terminus fonts of the image: name, width, height.
const TERMINUS: [(&str, u16, u16); 6] = [
    ("ter-v12n.psf", 6, 12),
    ("ter-v14b.psf", 8, 14),
    ("ter-v20b.psf", 10, 20),
    ("ter-v24b.psf", 12, 24),
    ("ter-v28b.psf", 14, 28),
    ("ter-v32b.psf", 16, 32),
];

/// The Spleen fonts of an image built with `zfsbootmenu_console_font=spleen`.
const SPLEEN: [(&str, u16, u16); 4] = [
    ("spleen-8x16.psf", 8, 16),
    ("spleen-12x24.psf", 12, 24),
    ("spleen-16x32.psf", 16, 32),
    ("spleen-32x64.psf", 32, 64),
];

fn fonts() -> Vec<(PathBuf, Cell)> {
    family(&TERMINUS)
}

fn family(fonts: &[(&str, u16, u16)]) -> Vec<(PathBuf, Cell)> {
    fonts
        .iter()
        .map(|(name, width, height)| {
            let cell = Cell {
                width: *width,
                height: *height,
            };
            (Path::new("/").join(FONTS).join(name), cell)
        })
        .collect()
}

/// The heights of `candidates`, doubled ones marked with `d`.
fn heights(candidates: &[Candidate]) -> Vec<String> {
    candidates
        .iter()
        .map(|c| format!("{}{}", c.cell.height, if c.doubled { "d" } else { "" }))
        .collect()
}

fn density(screen: (u16, u16), mm: (u16, u16)) -> Option<Density> {
    let edid = Edid {
        width_mm: mm.0,
        height_mm: mm.1,
        mode: None,
    };
    Density::new(screen, &edid)
}

#[test]
fn zbm_autosize_is_on_off_or_a_height() {
    assert_eq!(Request::parse(None), Request::Auto);
    assert_eq!(Request::parse(Some("1")), Request::Auto);
    assert_eq!(Request::parse(Some("yes")), Request::Auto);
    assert_eq!(Request::parse(Some("0")), Request::Off);
    assert_eq!(Request::parse(Some("off")), Request::Off);
    assert_eq!(Request::parse(Some("")), Request::Off);
    assert_eq!(Request::parse(Some("24")), Request::Height(24));
    assert_eq!(Request::parse(Some(" 48 ")), Request::Height(48));
    assert!(Request::Auto.fits(Some(MIN_COLUMNS)));
    assert!(!Request::Auto.fits(Some(MIN_COLUMNS - 1)));
    assert!(!Request::Auto.fits(None));
    assert!(Request::Height(64).fits(Some(40)));
    assert!(Request::Height(64).fits(None));
    assert!(!Request::Off.fits(Some(200)));
}

#[test]
fn density_comes_from_pixels_and_millimetres() {
    let laptop = density((3840, 2400), (302, 189)).unwrap();
    let desktop = density((1920, 1080), (509, 286)).unwrap();

    assert_eq!(laptop.dpi(), 323);
    assert_eq!(laptop.pixels(45), 57);
    assert_eq!(desktop.dpi(), 96);
    assert_eq!(desktop.pixels(45), 17);
}

#[test]
fn sizes_that_do_not_add_up_give_no_density() {
    // An aspect ratio given as a size, and directions that disagree.
    assert_eq!(density((3840, 2160), (16, 9)), None);
    assert_eq!(density((1920, 1080), (530, 530)), None);
    // A projector-sized picture.
    assert_eq!(density((1920, 1080), (5000, 2800)), None);
    assert_eq!(density((0, 1080), (530, 300)), None);
}

#[test]
fn without_a_density_the_largest_font_comes_first() {
    let order = order(&fonts(), Request::Auto, None);

    assert_eq!(heights(&order), ["32", "28", "24", "20", "14", "12"]);
}

#[test]
fn with_a_density_the_closest_height_comes_first_doubled_or_not() {
    let laptop = density((3840, 2400), (302, 189));
    let desktop = density((1920, 1080), (509, 286));

    let dense = order(&fonts(), Request::Auto, laptop);
    let normal = order(&fonts(), Request::Auto, desktop);

    // 57 pixels: 28 doubled first; 12 and 14 doubled are 24 and 28, which
    // the plain fonts already give.
    assert_eq!(
        heights(&dense),
        [
            "56d", "64d", "48d", "40d", "32", "28", "24", "20", "14", "12"
        ]
    );
    // 17 pixels: 20 and 14 are as close; the larger wins.
    assert_eq!(heights(&normal)[..3], ["20", "14", "12"]);
}

#[test]
fn spleen_is_not_guessed_larger_than_32_pixels_but_given_when_asked() {
    let spleen = family(&SPLEEN);
    let laptop = density((3840, 2400), (302, 189));

    assert_eq!(
        heights(&order(&spleen, Request::Auto, None)),
        ["32", "24", "16"]
    );
    // 57 pixels: the native 32×64 is as close as 24 doubled, and comes first;
    // 16×32 doubled is the native 32×64 again, and left out.
    assert_eq!(
        heights(&order(&spleen, Request::Auto, laptop))[..3],
        ["64", "48d", "32"]
    );
    assert_eq!(
        heights(&order(&spleen, Request::Height(64), None)),
        ["64", "48d", "32", "24", "16", "128d"]
    );
    let tall_only = family(&SPLEEN[3..]);
    assert_eq!(heights(&order(&tall_only, Request::Auto, None)), ["64"]);
    assert_eq!(
        super::choice::smallest(&spleen).map(|c| c.cell.height),
        Some(16)
    );
}

#[test]
fn a_height_asked_for_comes_first() {
    assert_eq!(
        heights(&order(&fonts(), Request::Height(24), None))[0],
        "24"
    );
    assert_eq!(
        heights(&order(&fonts(), Request::Height(64), None))[0],
        "64d"
    );
    assert_eq!(heights(&order(&fonts(), Request::Height(2), None))[0], "12");
    assert!(order(&fonts(), Request::Off, None).is_empty());
}

/// A machine with the image's fonts, a framebuffer of `screen` and a console
/// on which `setfont` changes the cell, as the kernel does; with
/// `tall_fonts` false, it refuses fonts taller than 32 pixels.
struct Machine {
    fake: FakeSystem,
}

impl Machine {
    fn new(screen: (u16, u16), tall_fonts: bool) -> Machine {
        Machine::failing(screen, tall_fonts, None)
    }

    /// The machine of [`Machine::new`], on which `program` fails.
    fn failing(screen: (u16, u16), tall_fonts: bool, program: Option<&str>) -> Machine {
        let fake = FakeSystem::new();
        if let Some(program) = program {
            fake.runner.respond_program(program, 1, "");
        }
        for (name, width, height) in TERMINUS {
            let path = Path::new("/").join(FONTS).join(name);
            fake.files.file(path, psf2(width.into(), height.into()));
        }
        fake.files
            .file(Path::new("/").join(FONTS).join("README"), "fonts");
        fake.files
            .file(Path::new("/").join(FONTS).join("broken.psf"), "x");
        let geometry = format!("{},{}\n", screen.0, screen.1);
        fake.files
            .file("/sys/class/graphics/fb0/virtual_size", geometry);
        let cell: Rc<RefCell<Option<(u16, u16)>>> = Rc::default();
        let current = Rc::clone(&cell);
        fake.runner.on(move |command| {
            let line = command.display();
            let words: Vec<&str> = line.split_whitespace().collect();
            let output = |status: u8, text: String| Ok(Output::new(Status::exited(status), text));
            match words.first().copied() {
                Some("setfont") => {
                    let name = Path::new(words.last()?).file_name()?.to_str()?;
                    let (_, width, height) = TERMINUS.iter().find(|(n, ..)| *n == name)?;
                    let factor = if words.contains(&"-d") { 2 } else { 1 };
                    let (width, height) = (width * factor, height * factor);
                    if height > 32 && !tall_fonts {
                        return Some(output(1, String::new()));
                    }
                    *current.borrow_mut() = Some((width, height));
                    Some(output(0, String::new()))
                }
                Some("stty") => {
                    let (width, height) = (*current.borrow())?;
                    let size = format!("{} {}\n", screen.1 / height, screen.0 / width);
                    Some(output(0, size))
                }
                _ => None,
            }
        });
        Machine { fake }
    }

    /// Adds a connector whose display has an EDID of `mm`.
    fn display(self, connector: &str, mode: (u16, u16), mm: (u16, u16)) -> Machine {
        let dir = Path::new("/sys/class/drm").join(connector);
        self.fake.files.file(dir.join("status"), "connected\n");
        self.fake
            .files
            .file(dir.join("edid"), edid(mode.0, mode.1, mm, (0, 0)));
        self
    }

    fn run(&self, request: Request) -> Outcome {
        let settings = Settings {
            console: "/dev/tty1".to_owned(),
            request,
            font_given: false,
        };
        run(&self.fake.system(), Path::new("/"), &settings)
    }
}

/// The font file, whether doubled, and the console size of a [`Outcome::Set`].
fn set(outcome: &Outcome) -> Option<(String, bool, (u16, u16))> {
    let Outcome::Set { font, size } = outcome else {
        return None;
    };
    let name = font.path.file_name()?.to_string_lossy().into_owned();
    Some((name, font.doubled, (*size)?))
}

fn font(name: &str, doubled: bool, size: (u16, u16)) -> (String, bool, (u16, u16)) {
    (name.to_owned(), doubled, size)
}

#[test]
fn a_4k_laptop_gets_a_doubled_font() {
    let machine = Machine::new((3840, 2400), true).display("card0-eDP-1", (3840, 2400), (302, 189));

    let outcome = machine.run(Request::Auto);

    assert_eq!(set(&outcome), Some(font("ter-v28b.psf", true, (137, 42))));
    assert_eq!(
        machine.fake.runner.lines(),
        [
            "setfont -C /dev/tty1 -d /usr/share/zfsbootmenu/fonts/ter-v28b.psf",
            "stty -F /dev/tty1 size",
        ]
    );
}

#[test]
fn where_the_kernel_refuses_tall_fonts_the_largest_plain_one_is_kept() {
    let machine =
        Machine::new((3840, 2400), false).display("card0-eDP-1", (3840, 2400), (302, 189));

    let outcome = machine.run(Request::Auto);

    assert_eq!(set(&outcome), Some(font("ter-v32b.psf", false, (240, 75))));
}

#[test]
fn a_desktop_monitor_gets_an_ordinary_font() {
    let machine = Machine::new((1920, 1080), true).display("card0-DP-1", (1920, 1080), (509, 286));

    let outcome = machine.run(Request::Auto);

    assert_eq!(set(&outcome), Some(font("ter-v20b.psf", false, (192, 54))));
}

#[test]
fn without_an_edid_the_legacy_rule_holds() {
    let full_hd = Machine::new((1920, 1080), true);
    let small = Machine::new((1366, 768), true);

    assert_eq!(
        set(&full_hd.run(Request::Auto)),
        Some(font("ter-v32b.psf", false, (120, 33)))
    );
    assert_eq!(
        set(&small.run(Request::Auto)),
        Some(font("ter-v24b.psf", false, (113, 32)))
    );
}

#[test]
fn the_first_connected_display_with_an_edid_counts() {
    let machine = Machine::new((3840, 2400), true)
        .display("card0-HDMI-A-1", (1920, 1080), (509, 286))
        .display("card0-eDP-1", (3840, 2400), (302, 189));
    machine
        .fake
        .files
        .file("/sys/class/drm/card0-DP-1/status", "disconnected\n");
    machine.fake.files.dir("/sys/class/drm/card0");
    machine
        .fake
        .files
        .file("/sys/class/drm/version", "drm 1.1.0\n");

    let outcome = machine.run(Request::Auto);

    // card0-HDMI-A-1 is a 96 dpi monitor with this 4K framebuffer on it:
    // directions agree, 192 dpi, 34 pixels.
    assert_eq!(set(&outcome), Some(font("ter-v32b.psf", false, (240, 75))));
}

#[test]
fn a_screen_too_small_for_110_columns_gets_the_smallest_font() {
    let machine = Machine::new((640, 480), true);

    let outcome = machine.run(Request::Auto);

    assert_eq!(set(&outcome), Some(font("ter-v12n.psf", false, (106, 40))));
}

#[test]
fn a_height_asked_for_is_kept_whatever_the_columns() {
    let machine = Machine::new((1920, 1080), true);

    assert_eq!(
        set(&machine.run(Request::Height(64))),
        Some(font("ter-v32b.psf", true, (60, 16)))
    );
}

#[test]
fn without_stty_the_columns_come_from_the_framebuffer() {
    let machine = Machine::failing((1366, 768), true, Some("stty"));

    let outcome = machine.run(Request::Auto);

    let Outcome::Set { font, size } = outcome else {
        panic!("not set: {outcome:?}");
    };
    assert_eq!(font.cell.height, 24);
    assert_eq!(size, None);
}

#[test]
fn a_height_asked_for_is_kept_when_the_size_is_unknown() {
    let machine = Machine::failing((1920, 1080), true, Some("stty"));
    let framebuffer = Path::new("/sys/class/graphics/fb0/virtual_size");
    machine.fake.files.remove_file(framebuffer).unwrap();

    let outcome = machine.run(Request::Height(24));

    let Outcome::Set { font, size } = outcome else {
        panic!("not set: {outcome:?}");
    };
    assert_eq!((font.cell.height, font.doubled, size), (24, false, None));
}

#[test]
fn when_no_font_loads_it_fails() {
    let machine = Machine::failing((1920, 1080), true, Some("setfont"));

    assert_eq!(machine.run(Request::Auto), Outcome::Failed);
}

#[test]
fn serial_consoles_chosen_fonts_and_off_are_left_alone() {
    let machine = Machine::new((1920, 1080), true);
    let system = machine.fake.system();
    let settings = |console: &str, request, font_given| Settings {
        console: console.to_owned(),
        request,
        font_given,
    };
    let root = Path::new("/");

    let serial = run(&system, root, &settings("/dev/ttyS0", Request::Auto, false));
    let given = run(&system, root, &settings("/dev/tty1", Request::Auto, true));
    let off = run(&system, root, &settings("/dev/tty1", Request::Off, false));
    machine.fake.files.file("/consolefont.psfu", "");
    let mkinitcpio = run(
        &system,
        root,
        &settings("/dev/tty1", Request::Height(32), false),
    );

    for outcome in [serial, given, off, mkinitcpio] {
        assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
    }
    assert_eq!(machine.fake.runner.lines(), Vec::<String>::new());
}

#[test]
fn an_image_without_fonts_is_left_alone() {
    let fake = FakeSystem::new();
    let settings = Settings {
        console: "/dev/tty1".to_owned(),
        request: Request::Auto,
        font_given: false,
    };

    let outcome = run(&fake.system(), Path::new("/"), &settings);

    assert_eq!(outcome, Outcome::Skipped("no fonts in the image"));
}

#[test]
fn the_settings_come_from_the_image() {
    let fake = FakeSystem::new();
    fake.files.file(
        "/etc/zfsbootmenu.conf",
        "export BASE=\"/zfsbootmenu\"\nexport control_term=\"/dev/tty1\"\n",
    );
    fake.files
        .file("/zfsbootmenu/zbm.cmdline", "quiet\nzbm.autosize=48\n");
    let other = FakeSystem::new();
    other.files.file(
        "/etc/zfsbootmenu.conf",
        "export control_term=\"/dev/ttyS0\"\n",
    );
    other.files.file(
        "/zfsbootmenu/zbm.cmdline",
        "rd.vconsole.font=ter-v24b\nzbm.autosize=off\n",
    );

    let conf =
        |fake: &FakeSystem| crate::preinit::read_conf(&fake.system(), Path::new("/")).unwrap();
    let image = Settings::from_image(&fake.system(), Path::new("/"), &conf(&fake));
    let serial = Settings::from_image(&other.system(), Path::new("/"), &conf(&other));

    assert_eq!(
        image,
        Settings {
            console: "/dev/tty1".to_owned(),
            request: Request::Height(48),
            font_given: false,
        }
    );
    assert_eq!(
        serial,
        Settings {
            console: "/dev/ttyS0".to_owned(),
            request: Request::Off,
            font_given: true,
        }
    );
}

#[test]
fn the_fonts_of_the_image_have_the_cells_their_names_say() {
    let root = zbm_testkit::repo_root().join("legacy/zfsbootmenu/fonts");
    let files = zbm_platform::files::RealFiles;

    for (dir, table) in [
        (root.clone(), &TERMINUS[..]),
        (root.join("spleen"), &SPLEEN[..]),
    ] {
        let found: Vec<(PathBuf, Cell)> = super::fonts(&files, &dir)
            .into_iter()
            .map(|(path, cell)| {
                (
                    Path::new("/").join(FONTS).join(path.file_name().unwrap()),
                    cell,
                )
            })
            .collect();
        let mut expected = family(table);
        expected.sort();
        assert_eq!(found, expected, "{}", dir.display());
    }
}

#[test]
fn the_hook_reads_the_image_and_sizes_the_console_given() {
    let machine = Machine::new((1366, 768), true);
    let files = &machine.fake.files;
    files.file(
        "/etc/zfsbootmenu.conf",
        "export control_term=\"/dev/ttyS0\"\nexport loglevel=\"7\"\n",
    );
    files.file("/zfsbootmenu/zbm.cmdline", "zbm.autosize=1\n");
    let system = machine.fake.system();

    let serial = super::hook(&system, Path::new("/"), None).unwrap();
    let tty1 = super::hook(&system, Path::new("/"), Some("/dev/tty1".to_owned())).unwrap();

    assert!(matches!(serial, Outcome::Skipped(_)), "{serial:?}");
    assert_eq!(set(&tty1), Some(font("ter-v24b.psf", false, (113, 32))));
    let without = FakeSystem::new();
    assert!(super::hook(&without.system(), Path::new("/"), None).is_err());
}
