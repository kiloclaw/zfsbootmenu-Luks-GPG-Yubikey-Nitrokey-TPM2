//! Whether the supervisor starts the console on the display
//! (`zbm.console=drm`, ROADMAP.md #19), and with which arguments: the font
//! height of `zbm.autosize=<pixels>`, the console fonts instead of the
//! TrueType font (`zbm.console_font=psf`), the keyboard layout
//! (`zbm.keyboard_layout`), and the mirror of the VM tests.

use std::path::Path;

use zbm_config::args::{Args, ZbmArgs};
use zbm_config::conf::Conf;
use zbm_config::spec::cmdline::{
    AUTOSIZE, CONSOLE_MIRROR, DISPLAY_CONSOLE, DISPLAY_CONSOLE_FONT, KEYBOARD_LAYOUT,
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
    if let Some(mirror) = args
        .lookup(&CONSOLE_MIRROR, &mut messages)
        .filter(|mirror| !mirror.trim().is_empty())
    {
        arguments.extend(["--mirror".to_owned(), mirror.trim().to_owned()]);
    }
    Some(arguments)
}

/// The arguments of the console the image below `root` starts, from the
/// command line that start-up saved; `None` for the kernel console, and
/// when the control terminal is not a virtual console.
pub fn from_image(system: &System<'_>, root: &Path, conf: &Conf) -> Option<Vec<String>> {
    let options = preinit::saved_options(system, root, conf);
    if !preinit::is_virtual_console(&options.control_term) {
        return None;
    }
    arguments(&preinit::saved_commandline(system, root, conf))
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
    }

    #[test]
    fn the_image_s_saved_command_line_and_terminal_decide() {
        let fake = FakeSystem::new();
        fake.files
            .file("/zfsbootmenu/zbm.cmdline", "zbm.console=drm\n")
            .dir("/zfsbootmenu");
        let conf = |term: &str| Conf::parse(&format!("export control_term=\"{term}\"\n")).unwrap();

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
}
