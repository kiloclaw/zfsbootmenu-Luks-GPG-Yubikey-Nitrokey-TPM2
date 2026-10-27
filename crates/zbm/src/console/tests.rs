//! The console applet's options, settings and fonts.

use pretty_assertions::assert_eq;

use super::*;

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn options_are_parsed_and_wrong_ones_refused() {
    assert_eq!(
        parse(&args(
            "--card /dev/dri/card1 --height 32 --mirror /dev/ttyS1"
        )),
        Some(Options {
            card: Some(PathBuf::from("/dev/dri/card1")),
            height: Some(32),
            psf: false,
            layout: None,
            num_lock: None,
            repeat: None,
            scrollback: None,
            mirror: Some(PathBuf::from("/dev/ttyS1")),
            scheme: None,
            large: false,
            status: false,
            bell: Ring::Visual,
            compose_key: None,
            device_layouts: Vec::new(),
        })
    );
    let keys = parse(&args("--numlock on --repeat 250,30")).unwrap();
    assert_eq!((keys.num_lock, keys.repeat), (Some(true), Some((250, 33))));
    assert_eq!(parse(&args("--numlock off")).unwrap().num_lock, Some(false));
    assert_eq!(parse(&args("--numlock yes")), None);
    assert_eq!(parse(&args("--repeat fast")), None);
    assert_eq!(parse(&args("--scrollback 0")).unwrap().scrollback, Some(0));
    assert_eq!(parse(&args("--scrollback 100001")), None);
    let look = parse(&args("--palette tango --large")).unwrap();
    assert_eq!(look.scheme.map(|scheme| scheme.name), Some("tango"));
    assert!(look.large);
    assert_eq!(parse(&args("--palette rainbow")), None);
    let more = parse(&args("--status --bell both")).unwrap();
    assert!(more.status);
    assert_eq!(more.bell, Ring::Both);
    assert_eq!(parse(&args("--bell loud")), None);
    assert_eq!(
        parse(&args("--compose ralt")).unwrap().compose_key,
        Some(100)
    );
    assert_eq!(parse(&args("--compose pause")), None);
    let rules = parse(&args(
        "--keyboard 046d:c31c=de --keyboard 1a2c:0e24=fr(azerty)",
    ))
    .unwrap();
    let names: Vec<String> = rules
        .device_layouts
        .iter()
        .map(|(id, name)| format!("{id}={name}"))
        .collect();
    assert_eq!(names, ["046d:c31c=de", "1a2c:0e24=fr(azerty)"]);
    assert_eq!(parse(&args("--keyboard 046d:c31c")), None);
    assert_eq!(parse(&args("--keyboard 046d=de")), None);
    assert_eq!(parse(&args("--keyboard 046d:c31c=d e")), None);
    // The names `zbm.console_palette` takes are the console's.
    let palettes = zbm_config::spec::cmdline::PALETTES;
    let schemes: Vec<&str> = zbm_console::scheme::SCHEMES
        .iter()
        .map(|scheme| scheme.name)
        .collect();
    assert_eq!(palettes, schemes);
    for bell in zbm_config::spec::cmdline::BELLS {
        assert!(Ring::parse(bell).is_some(), "{bell}");
    }
    let keys: Vec<&str> = zbm_display::keys::COMPOSE_KEYS
        .iter()
        .map(|&(name, _)| name)
        .collect();
    assert_eq!(zbm_config::spec::cmdline::COMPOSE_KEYS, keys);
    assert_eq!(parse(&[]), Some(Options::default()));
    assert!(parse(&args("--psf")).unwrap().psf);
    let layout = parse(&args("--layout de(nodeadkeys)")).unwrap().layout;
    assert_eq!(layout.as_deref(), Some("de(nodeadkeys)"));
    assert_eq!(parse(&args("--layout")), None);
    assert_eq!(parse(&args("--height tall")), None);
    assert_eq!(parse(&args("--mirror")), None);
    assert_eq!(parse(&args("--loud")), None);

    assert_eq!(parse_release(&[]), Some(RELEASE_TIMEOUT));
    assert_eq!(
        parse_release(&args("--timeout 500")),
        Some(Duration::from_millis(500))
    );
    assert_eq!(parse_release(&args("--timeout soon")), None);
    assert_eq!(parse_release(&args("--now")), None);
}

#[test]
fn wrong_options_show_the_usage() {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };

    assert_eq!(run(&args("--loud"), &mut io), USAGE);
    // No console runs on the machine of the tests: nothing to hand back.
    assert_eq!(run(&args("release --timeout 10"), &mut io), 0);
    assert_eq!(run(&args("release --now"), &mut io), USAGE);
    assert_eq!(run(&args("reattach --timeout 10"), &mut io), 0);
    assert_eq!(run(&args("reattach --now"), &mut io), USAGE);
    let text = String::from_utf8_lossy(&stderr).into_owned();
    assert_eq!(text.matches("Usage: zbm console").count(), 3, "{text}");
}

#[test]
fn the_console_draws_with_the_truetype_font_at_the_console_font_s_height() {
    let files = zbm_testkit::fakes::FakeFiles::new();
    let fonts = Path::new("/").join(FONTS);
    let terminus = include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v20b.psf");
    files.file(fonts.join("ter-v20b.psf"), terminus.as_slice());
    let options = Options::default();
    let psf = Options {
        psf: true,
        ..Options::default()
    };
    let grid = |options: &Options| console_grid(&files, Path::new("/"), options, (1280, 800), None);

    // Without a TrueType font, the console font.
    let bitmap = grid(&options).unwrap();
    assert_eq!((bitmap.name(), bitmap.cell()), ("ter-v20b.psf", (10, 20)));

    let dejavu = include_bytes!("../../../../legacy/zfsbootmenu/fonts/dejavu/DejaVuSansMono.ttf");
    files.file(fonts.join(TTF), dejavu.as_slice());
    let outline = grid(&options).unwrap();
    assert_eq!(outline.name(), TTF);
    assert_eq!(outline.cell().1, 20);
    assert_eq!(grid(&psf).unwrap().name(), "ter-v20b.psf");

    assert_eq!(outline.font_count(), 1);
    // A height asked for while the console runs: the TrueType font in
    // cells that high; the console font nearest it, here doubled.
    let asked = console_grid(&files, Path::new("/"), &options, (1280, 800), Some(32));
    assert_eq!(asked.unwrap().cell().1, 32);
    let asked = console_grid(&files, Path::new("/"), &psf, (1280, 800), Some(32));
    assert_eq!(asked.unwrap().cell().1, 40);

    // Its faces and fallback fonts, those that are fonts.
    files.file(fonts.join(STYLED[0].1), dejavu.as_slice());
    let fallback = fonts.join(FALLBACK);
    files.file(fallback.join("b.ttf"), dejavu.as_slice());
    files.file(fallback.join("a.ttf"), "not a font");
    assert_eq!(grid(&options).unwrap().font_count(), 3);
    assert_eq!(grid(&psf).unwrap().font_count(), 1);

    // Large text: cells twice as high, or half again as high on a
    // display too narrow for that.
    let large = Options {
        large: true,
        ..Options::default()
    };
    let large_grid = |size| console_grid(&files, Path::new("/"), &large, size, None);
    assert_eq!(large_grid((1280, 800)).unwrap().cell().1, 40);
    assert_eq!(large_grid((700, 500)).unwrap().cell().1, 30);
    assert_eq!(large_grid((400, 300)).unwrap().cell().1, 20);

    // A file that is no font leaves the console font.
    files.file(fonts.join(TTF), "not a font");
    assert_eq!(grid(&options).unwrap().name(), "ter-v20b.psf");
}
