//! The crash report the supervisor names.

use super::*;
use zbm_testkit::TempDir;

#[test]
fn the_newest_crash_report_of_the_program_is_named() {
    let dir = TempDir::new("zbm-supervisor");
    for name in [
        "crash-5-42.txt",
        "crash-5-42.1.txt",
        "crash-6-1042.txt",
        "crash-4-42.2.txt",
    ] {
        dir.write(name, "report\n");
    }
    let program = Program {
        path: PathBuf::new(),
        args: Vec::new(),
    };
    let supervisor = Supervisor {
        startup: program.clone(),
        shell: program,
        retry: Duration::ZERO,
        redirect_console: false,
        fallback: None,
        log: dir.join("kmsg"),
        base: dir.path().to_owned(),
        console: None,
        console_restart: Backoff::NEVER,
    };

    let report = supervisor.crash_report(42, 101 << 8);

    assert_eq!(report, Some(dir.join("crash-5-42.1.txt")));
}
