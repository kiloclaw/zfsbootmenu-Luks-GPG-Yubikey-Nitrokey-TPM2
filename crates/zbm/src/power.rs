//! `reboot`, `poweroff` (also `shutdown`) and `firmware-setup`: the recovery
//! shell's ways to reset the machine ([`zbm_app::power`]), which export the
//! writable pools first. Each returns only when the machine did not reset:
//! it then says why on standard error and exits 1.

use zbm_app::model::{Event, Outcome};
use zbm_app::power::{self, Reset};
use zbm_platform::{NoConsole, RealSystem, System};
use zbm_types::Printable;

use crate::table::{FIRMWARE_SETUP, POWEROFF, REBOOT};
use crate::{Applet, Io, NOT_FOUND, USAGE};

/// How `applet` resets the machine; none for an applet that does not.
fn reset_of(applet: &Applet) -> Option<Reset> {
    [
        (&REBOOT, Reset::Reboot),
        (&POWEROFF, Reset::PowerOff),
        (&FIRMWARE_SETUP, Reset::FirmwareSetup),
    ]
    .into_iter()
    .find(|(power, _)| power.name == applet.name)
    .map(|(_, reset)| reset)
}

/// Resets the machine of `system` as `applet` does; any argument is a
/// usage error.
pub(crate) fn run_in(system: &System<'_>, applet: &Applet, args: &[String], io: &mut Io) -> i32 {
    let Some(reset) = reset_of(applet) else {
        return NOT_FOUND;
    };
    if !args.is_empty() {
        let _ = io.stderr.write_all(applet.usage.as_bytes());
        return USAGE;
    }
    let events = power::reset(system, reset);
    if let Some(Event::Finished(Outcome::Failed { what, why, .. })) = events.last() {
        let _ = writeln!(io.stderr, "{}: {}", applet.name, Printable(what));
        for reason in why {
            let _ = writeln!(io.stderr, "  {}", Printable(reason));
        }
    }
    1
}

/// Runs `applet` on this machine.
fn run(applet: &Applet, args: &[String], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    run_in(&real.with_console(&NoConsole), applet, args, io)
}

/// `reboot`.
pub(crate) fn reboot(args: &[String], io: &mut Io) -> i32 {
    run(&REBOOT, args, io)
}

/// `poweroff`, and `shutdown`.
pub(crate) fn poweroff(args: &[String], io: &mut Io) -> i32 {
    run(&POWEROFF, args, io)
}

/// `firmware-setup`.
pub(crate) fn firmware_setup(args: &[String], io: &mut Io) -> i32 {
    run(&FIRMWARE_SETUP, args, io)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeSystem;

    use super::*;

    /// Runs the applet `name` resolves to on `fake` with `args`, as a link
    /// of that name runs it: its exit status, and what it wrote on
    /// standard error.
    fn call(fake: &FakeSystem, name: &str, args: &[&str]) -> (i32, String) {
        let (applet, first) = crate::find(crate::APPLETS, name).unwrap();
        assert!(first.is_empty(), "{name}");
        call_applet(fake, applet, args)
    }

    /// Runs `applet` on `fake` with `args`.
    fn call_applet(fake: &FakeSystem, applet: &Applet, args: &[&str]) -> (i32, String) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = run_in(
            &fake.system(),
            applet,
            &args,
            &mut Io {
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        );
        assert!(stdout.is_empty());
        (code, String::from_utf8(stderr).unwrap())
    }

    /// A machine whose pool `tank` is imported read/write.
    fn machine() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.files.dir("/proc");
        fake.runner.respond("zpool list -H -o name", 0, "tank\n");
        fake.runner
            .respond("zpool get -H -o value readonly tank", 0, "off\n");
        fake
    }

    /// Whether `fake` exported `tank`.
    fn has_exported(fake: &FakeSystem) -> bool {
        fake.runner
            .lines()
            .contains(&"zpool export tank".to_owned())
    }

    #[test]
    fn firmware_setup_on_bios_says_why_and_exits_1() {
        let fake = machine();

        let (code, stderr) = call(&fake, "firmware-setup", &[]);

        assert_eq!(code, 1);
        assert_eq!(
            stderr,
            "firmware-setup: unable to reboot into the firmware setup\n  \
             the firmware does not say that it can open its setup\n"
        );
        assert!(fake.files.text("/proc/sysrq-trigger").is_none());
        assert!(!has_exported(&fake));
    }

    #[test]
    fn each_name_resets_the_machine_as_it_says() {
        for (name, trigger, applet) in [
            ("reboot", "b", "reboot"),
            ("poweroff", "o", "poweroff"),
            ("shutdown", "o", "poweroff"),
        ] {
            let fake = machine();

            let (code, stderr) = call(&fake, name, &[]);

            assert_eq!(code, 1, "{name}");
            assert_eq!(
                fake.files.text("/proc/sysrq-trigger").as_deref(),
                Some(trigger),
                "{name}"
            );
            assert!(has_exported(&fake), "{name}");
            assert_eq!(
                stderr,
                format!("{applet}: unable to reset the machine\n  the machine did not reset\n")
            );
        }
    }

    #[test]
    fn an_argument_to_reboot_is_a_usage_error() {
        let fake = machine();

        let (code, stderr) = call(&fake, "reboot", &["now"]);

        assert_eq!((code, stderr.as_str()), (USAGE, REBOOT.usage));
        assert!(fake.files.text("/proc/sysrq-trigger").is_none());
        assert!(fake.runner.lines().is_empty());
    }

    #[test]
    fn an_applet_that_is_not_a_power_one_resets_nothing() {
        let fake = machine();
        let (zfunc, _) = crate::find(crate::APPLETS, "zfunc").unwrap();

        let (code, stderr) = call_applet(&fake, zfunc, &[]);

        assert_eq!((code, stderr.as_str()), (NOT_FOUND, ""));
        assert!(fake.files.text("/proc/sysrq-trigger").is_none());
        assert!(fake.runner.lines().is_empty());
    }
}
