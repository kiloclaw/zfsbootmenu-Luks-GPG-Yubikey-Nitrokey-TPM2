//! Whether the supervisor starts the console on the display
//! (`zbm.console=drm`, ROADMAP.md #19), and with which arguments: the font
//! height of `zbm.autosize=<pixels>`, the console fonts instead of the
//! TrueType font (`zbm.console_font=psf`), the keyboard layout
//! (`zbm.keyboard_layout`), Num Lock (`zbm.numlock`), the key repeat
//! (`zbm.keyboard_repeat`), the scrollback (`zbm.console_scrollback`), the
//! colour scheme (`zbm.console_palette`), large text
//! (`zbm.console_large`), the status line (`zbm.console_status`), the bell
//! (`zbm.console_bell`), and the mirror of the VM tests.

use std::path::Path;

use zbm_config::args::{Args, ZbmArgs};
use zbm_config::conf::Conf;
use zbm_config::spec::OptionSpec;
use zbm_config::spec::cmdline::{
    AUTOSIZE, BELLS, COMPOSE_KEYS, CONSOLE_BELL, CONSOLE_LARGE, CONSOLE_MIRROR, CONSOLE_PALETTE,
    CONSOLE_SCROLLBACK, CONSOLE_STATUS, DISPLAY_CONSOLE, DISPLAY_CONSOLE_FONT, KEYBOARD_COMPOSE,
    KEYBOARD_LAYOUT, KEYBOARD_LAYOUTS, KEYBOARD_REPEAT, MAX_SCROLLBACK, NUM_LOCK, PALETTES,
    is_layout_name,
};
use zbm_platform::System;

use crate::autosize::choice::Request;
use crate::preinit;

/// The arguments of `zbm` that start the console the command line `kcl`
/// asks for (`console ...`); `None` for the kernel console.
pub fn arguments(kcl: &str) -> Option<Vec<String>> {
    let args = ZbmArgs::from_kcl(kcl);
    let mut messages = Vec::new();
    let console = args.lookup(&DISPLAY_CONSOLE, &mut messages)?;
    if console.trim() != "drm" {
        return None;
    }
    let mut arguments = vec!["console".to_owned()];
    let request = Request::parse(args.lookup(&AUTOSIZE, &mut messages).as_deref());
    if let Request::Height(pixels) = request {
        arguments.extend(["--height".to_owned(), pixels.to_string()]);
    }
    let font = args.lookup(&DISPLAY_CONSOLE_FONT, &mut messages);
    if font.as_deref().map(str::trim) == Some("psf") {
        arguments.push("--psf".to_owned());
    }
    if let Some(layout) = args
        .lookup(&KEYBOARD_LAYOUT, &mut messages)
        .filter(|layout| !layout.trim().is_empty())
    {
        arguments.extend(["--layout".to_owned(), layout.trim().to_owned()]);
    }
    for (spec, flag, is_valid) in VALUES {
        if let Some(value) = args
            .lookup(spec, &mut messages)
            .map(|value| value.trim().to_owned())
            .filter(|value| is_valid(value))
        {
            arguments.extend([flag.to_owned(), value]);
        }
    }
    let own = args.lookup(&KEYBOARD_LAYOUTS, &mut messages);
    for rule in own.as_deref().map(keyboard_layouts).unwrap_or_default() {
        arguments.extend(["--keyboard".to_owned(), rule.to_owned()]);
    }
    for (spec, flag) in [(&CONSOLE_LARGE, "--large"), (&CONSOLE_STATUS, "--status")] {
        if args.lookup(spec, &mut messages).as_deref().map(str::trim) == Some("on") {
            arguments.push(flag.to_owned());
        }
    }
    Some(arguments)
}

/// The rules of `value` (`046d:c31c=de,1a2c:0e24=fr`) that name a USB
/// keyboard, by vendor and product, and a layout.
fn keyboard_layouts(value: &str) -> Vec<&str> {
    let is_hex = |part: &str| part.len() == 4 && part.bytes().all(|b| b.is_ascii_hexdigit());
    value
        .trim()
        .split(',')
        .filter(|rule| {
            let Some((id, name)) = rule.split_once('=') else {
                return false;
            };
            let (vendor, product) = id.split_once(':').unwrap_or_default();
            is_hex(vendor) && is_hex(product) && is_layout_name(name)
        })
        .collect()
}

/// Whether a value is one the console takes.
type IsValid = fn(&str) -> bool;

/// The options with a value that the console gets as they are, with its
/// argument for each, when the value is right.
const VALUES: [(&OptionSpec, &str, IsValid); 7] = [
    (&NUM_LOCK, "--numlock", |value| {
        matches!(value, "on" | "off")
    }),
    (&KEYBOARD_REPEAT, "--repeat", |value| {
        repeat(value).is_some()
    }),
    (&CONSOLE_SCROLLBACK, "--scrollback", |value| {
        value
            .parse::<usize>()
            .is_ok_and(|lines| lines <= MAX_SCROLLBACK)
    }),
    (&CONSOLE_PALETTE, "--palette", |value| {
        PALETTES.contains(&value)
    }),
    (&CONSOLE_BELL, "--bell", |value| BELLS.contains(&value)),
    (&KEYBOARD_COMPOSE, "--compose", |value| {
        COMPOSE_KEYS.contains(&value)
    }),
    // A copy of the screen, passphrase prompts included, is for test builds
    // alone.
    (&CONSOLE_MIRROR, "--mirror", |value| {
        cfg!(any(test, feature = "console-mirror")) && !value.is_empty()
    }),
];

/// The key repeat that `value` (`250,30`) asks for: the delay and the
/// period in milliseconds, from a delay in milliseconds and a rate a
/// second; `None` when it is not two numbers, or the rate is 0.
pub fn repeat(value: &str) -> Option<(u32, u32)> {
    let (delay, rate) = value.trim().split_once(',')?;
    let delay: u32 = delay.trim().parse().ok()?;
    let rate: u32 = rate.trim().parse().ok().filter(|&rate| rate > 0)?;
    Some((delay, 1000 / rate.min(1000)))
}

/// The arguments of the console the image below `root` starts, from the
/// command line that start-up saved; `None` for the kernel console, and
/// when the control terminal is not a virtual console. In an image with
/// graphics drivers, the console draws on the card of the GPU `zbm.gpu`
/// chooses (`--card`, [`crate::graphics::console_card`]).
pub fn from_image(system: &System<'_>, root: &Path, conf: &Conf) -> Option<Vec<String>> {
    let options = preinit::saved_options(system, root, conf);
    if !preinit::is_virtual_console(&options.control_term) {
        return None;
    }
    let kcl = preinit::saved_commandline(system, root, conf);
    let mut arguments = arguments(&kcl)?;
    let gpu = crate::graphics::option(&kcl);
    if let Some(card) = crate::graphics::console_card(system.files, root, gpu.as_deref()) {
        arguments.extend(["--card".to_owned(), card.display().to_string()]);
    }
    Some(arguments)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeSystem;

    use super::*;

    /// The console's arguments `line`, as [`arguments`] gives them.
    fn words(line: &str) -> Option<Vec<String>> {
        let words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        (!words.is_empty()).then_some(words)
    }

    /// The console for the options in `line`, saved one per line.
    fn console(line: &str) -> Option<Vec<String>> {
        arguments(&line.replace(' ', "\n"))
    }

    #[test]
    fn the_command_line_chooses_the_console_and_its_arguments() {
        assert_eq!(arguments(""), None);
        assert_eq!(console("zbm.console=kernel"), None);
        assert_eq!(console("zbm.console=drm"), words("console"));
        assert_eq!(console("zbm.console=drm zbm.autosize=0"), words("console"));
        // A value the option does not document is the default.
        assert_eq!(console("zbm.console=fb"), None);
        assert_eq!(
            console(
                "zbm.console=drm zbm.autosize=48 zbm.console_font=psf zbm.keyboard_layout=de \
                 zbm.console_mirror=/dev/ttyS1"
            ),
            words("console --height 48 --psf --layout de --mirror /dev/ttyS1")
        );
        assert_eq!(
            console("zbm.console=drm zbm.console_mirror= zbm.keyboard_layout="),
            words("console")
        );
        assert_eq!(
            console(
                "zbm.console=drm zbm.numlock=on zbm.keyboard_repeat=250,30 zbm.console_scrollback=0"
            ),
            words("console --numlock on --repeat 250,30 --scrollback 0")
        );
        assert_eq!(
            console(
                "zbm.console=drm zbm.console_palette=tango zbm.console_large=on \
                 zbm.console_status=on zbm.console_bell=both zbm.keyboard_compose=menu"
            ),
            words("console --palette tango --bell both --compose menu --large --status")
        );
        assert_eq!(
            console("zbm.console=drm zbm.console_palette=rainbow zbm.console_large=off"),
            words("console")
        );
        assert_eq!(
            console("zbm.console=drm zbm.keyboard_layouts=046d:c31c=de,bad,1a2c:0e24=fr(azerty)"),
            words("console --keyboard 046d:c31c=de --keyboard 1a2c:0e24=fr(azerty)")
        );
        assert_eq!(
            console("zbm.console=drm zbm.keyboard_layouts=46d:c31c=de,046d:c31c="),
            words("console")
        );
        // Wrong values are left out: the console keeps the keyboards' own.
        assert_eq!(
            console("zbm.console=drm zbm.numlock=yes zbm.keyboard_repeat=fast"),
            words("console")
        );
        // So are values the console would refuse, which would keep it from
        // starting at all.
        assert_eq!(
            console(
                "zbm.console=drm zbm.console_scrollback=200000 \
                 zbm.keyboard_layouts=046d:c31c=de(,046d:c31d=(de),046d:c31e=de)x"
            ),
            words("console")
        );
        assert_eq!(
            console("zbm.console=drm zbm.console_scrollback=100000"),
            words("console --scrollback 100000")
        );
    }

    #[test]
    fn the_key_repeat_is_a_delay_and_a_rate() {
        assert_eq!(repeat("250,30"), Some((250, 33)));
        assert_eq!(repeat(" 500 , 1 "), Some((500, 1000)));
        assert_eq!(repeat("200,5000"), Some((200, 1)));
        for wrong in ["", "250", "250,0", "a,30", "250,30,1", "-1,30"] {
            assert_eq!(repeat(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn the_image_s_saved_command_line_and_terminal_decide() {
        let fake = FakeSystem::new();
        fake.files
            .file("/zfsbootmenu/zbm.cmdline", "zbm.console=drm\n")
            .dir("/zfsbootmenu");
        let conf = |term: &str| Conf::parse(&format!("export control_term=\"{term}\"\n"));

        let system = fake.system();
        assert_eq!(
            from_image(&system, Path::new("/"), &conf("/dev/tty1")),
            words("console")
        );
        assert_eq!(
            from_image(&system, Path::new("/"), &conf("/dev/ttyS0")),
            None
        );
    }

    #[test]
    fn with_graphics_drivers_the_console_draws_on_the_chosen_gpu() {
        let fake = FakeSystem::new();
        let gpu = |address: &str, vendor: &str, card: &str, status: &str| {
            let dir = format!("/sys/bus/pci/devices/{address}");
            fake.files
                .file(format!("{dir}/class"), "0x030000\n")
                .file(format!("{dir}/vendor"), format!("{vendor}\n"))
                .file(format!("{dir}/device"), "0x1234\n")
                .dir(format!("{dir}/drm/{card}"))
                .file(format!("/sys/class/drm/{card}-HDMI-A-1/status"), status);
        };
        gpu("0000:00:02.0", "0x8086", "card0", "connected\n");
        gpu("0000:03:00.0", "0x1002", "card1", "connected\n");
        fake.files
            .file("/etc/zfsbootmenu/graphics-drivers", "amdgpu i915\n")
            .file("/zfsbootmenu/zbm.cmdline", "zbm.console=drm\nzbm.gpu=amd\n");
        let conf = Conf::parse("export control_term=\"/dev/tty1\"\n");

        let system = fake.system();
        assert_eq!(
            from_image(&system, Path::new("/"), &conf),
            words("console --card /dev/dri/card1")
        );

        // Without zbm.gpu, the first GPU with a display.
        fake.files
            .file("/zfsbootmenu/zbm.cmdline", "zbm.console=drm\n");
        assert_eq!(
            from_image(&system, Path::new("/"), &conf),
            words("console --card /dev/dri/card0")
        );
    }
}
