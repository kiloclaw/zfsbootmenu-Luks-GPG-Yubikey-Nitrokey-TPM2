//! `zbm shadow`: shadow mode on a machine that boots with the legacy runtime
//! (`zbm_boot::shadow`).
//!
//! - `zbm shadow`, run by the `boot-sel.d` hook of images built with
//!   `zfsbootmenu_shadow=yes`, compares the legacy runtime's decisions with
//!   the Rust code's and prints each difference and the summary line
//!   `ZBM-SHADOW: <n> differences` on the console and to the kernel log. It
//!   only reads, never prompts, gives up after a few seconds and always exits
//!   with status 0, so it can never stop or slow a boot. With
//!   `zbm.shadow=efivar`, a summary with differences is also kept in an EFI
//!   variable, once per boot.
//! - `zbm shadow report`, on the booted system, prints that summary and
//!   removes the variable.

use std::path::Path;
use std::time::Duration;

use zbm_boot::shadow::{Selected, compare, efivar};
use zbm_config::args::{Args, CmdlineDir};
use zbm_config::spec::cmdline::SHADOW;
use zbm_platform::ReadOnlyRunner;
use zbm_platform::{Files, NoConsole, RealFiles, RealSystem, System};
use zbm_state::State;

use crate::{Io, USAGE};

/// How long shadow mode may run programs.
pub const BUDGET: Duration = Duration::from_secs(5);

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm shadow           compare the legacy runtime's decisions (boot-sel.d hook)
       zbm shadow report    print and clear the summary kept with zbm.shadow=efivar
";

/// `zbm shadow`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => {
            let real = RealSystem::new();
            check(&real.with_console(&NoConsole), None, io)
        }
        ["report"] => report(&RealFiles, Path::new(efivar::EFIVARS), io),
        _ => {
            let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
            USAGE
        }
    }
}

/// Compares the decisions on `system`, with the state directory `base` or the
/// one `zfsbootmenu.conf` names; always status 0.
pub fn check(system: &System<'_>, base: Option<&Path>, io: &mut Io) -> i32 {
    let conf = zbm_startup::preinit::read_conf(system, Path::new("/")).unwrap_or_default();
    let base = base.map_or_else(|| zbm_startup::preinit::base(&conf), Path::to_path_buf);
    let _log = system.kernel_log(conf.get("loglevel").unwrap_or_default(), &base);
    let state = State::new(system.files, &base);
    let runner = ReadOnlyRunner::new(system.runner, system.clock, BUDGET);
    let reading = System {
        runner: &runner,
        ..*system
    };
    let report = compare(
        &reading,
        &state,
        &Selected::from_environment(system.environment),
    );
    let _ = io.stdout.write_all(report.text().as_bytes());
    zbm_trace::notice!("{}", report.summary());
    if report.differences() > 0
        && CmdlineDir::new(system.files, state.base())
            .lookup(&SHADOW, &mut Vec::new())
            .as_deref()
            == Some("efivar")
    {
        let efivars = Path::new(efivar::EFIVARS);
        if let Err(error) = efivar::record(system.files, &state, efivars, &report.text()) {
            zbm_trace::warn!("unable to keep the shadow summary in an EFI variable: {error}");
        }
    }
    0
}

/// Prints and removes the summary kept in the EFI variable below `efivars`.
pub fn report(files: &dyn Files, efivars: &Path, io: &mut Io) -> i32 {
    match efivar::take(files, efivars) {
        Ok(Some(text)) => {
            let _ = io.stdout.write_all(text.as_bytes());
            0
        }
        Ok(None) => {
            let _ = writeln!(io.stdout, "no shadow report");
            0
        }
        Err(error) => {
            let _ = writeln!(
                io.stderr,
                "zbm shadow: unable to remove {}: {error} (try chattr -i)",
                efivar::path(efivars).display()
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeSystem;

    const BASE: &str = "/zfsbootmenu";

    /// The report of the machine of the first test.
    const EXPECTED: &str = concat!(
        "ZBM-SHADOW: host ID: legacy 00bab10c, rust deadbeef\n",
        "ZBM-SHADOW: 1 differences in 1 decisions (host ID)\n",
    );

    fn call(body: impl FnOnce(&mut Io) -> i32) -> (i32, String, String) {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = body(&mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        });
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn differences_are_printed_logged_and_kept_with_efivar() {
        let fake = FakeSystem::new();
        fake.files
            .dir(BASE)
            .dir(efivar::EFIVARS)
            .file("/etc/hostid", [0x0c, 0xb1, 0xba, 0x00])
            .file("/zfsbootmenu/spl_hostid", "deadbeef\n")
            .file("/zfsbootmenu/cmdline.d/zbm.shadow", "efivar\n")
            .file("/etc/zfsbootmenu.conf", "export loglevel='5'\n");

        let (code, stdout, _) = call(|io| check(&fake.system(), None, io));

        assert_eq!(code, 0);
        assert_eq!(stdout, EXPECTED);
        assert!(
            fake.kmsg()
                .contains("<5>ZFSBootMenu: ZBM-SHADOW: 1 differences in 1 decisions")
        );
        let (code, stdout, _) = call(|io| report(&fake.files, Path::new(efivar::EFIVARS), io));
        assert_eq!((code, stdout.as_str()), (0, EXPECTED));
        let (_, stdout, _) = call(|io| report(&fake.files, Path::new(efivar::EFIVARS), io));
        assert_eq!(stdout, "no shadow report\n");
    }

    #[test]
    fn a_machine_without_decisions_to_compare_has_no_differences() {
        let fake = FakeSystem::new();
        fake.files.dir(BASE);

        let (code, stdout, _) = call(|io| check(&fake.system(), None, io));

        assert_eq!(
            (code, stdout.as_str()),
            (0, "ZBM-SHADOW: 0 differences in 0 decisions\n")
        );
        assert_eq!(call(|io| run(&["bogus".to_owned()], io)).0, USAGE);
    }
}
