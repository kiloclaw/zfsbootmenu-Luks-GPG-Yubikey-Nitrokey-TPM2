//! The functions of the library on a fake machine.

use zbm_platform::exec::{Input, Stream};
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// Runs `function` on `fake` with `args` and the configuration `conf`.
fn call(fake: &FakeSystem, conf: &str, function: &str, args: &[&str]) -> i32 {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    run(&fake.system(), &Conf::parse(conf), function, &args)
}

/// An image with the legacy library and a state directory.
fn image() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .dir("/zfsbootmenu")
        .file("/usr/share/zfsbootmenu/legacy/lib/zfsbootmenu-core.sh", "");
    fake
}

/// ADR 0017, point 10: a function without Rust code runs as the legacy
/// one, on Bash, with the caller's streams, the configuration where the
/// environment does not set a variable, and the function's status.
#[test]
fn other_functions_run_from_the_legacy_files() {
    let fake = image();
    fake.environment.set("BASE", "/elsewhere");
    fake.runner.respond_program("bash", 3, "");
    let conf = "export BASE=\"/zfsbootmenu\"\nexport loglevel=\"7\"\n";
    assert_eq!(call(&fake, conf, "import_pool", &["tank", "a b"]), 3);

    let calls = fake.runner.calls();
    let [bash] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    let args: Vec<String> = bash
        .get_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args[0], "-c");
    assert!(args[1].contains("builtin source \"${legacy}${1}\""));
    assert_eq!(
        &args[2..],
        [
            "/libexec/zfunc",
            "/usr/share/zfsbootmenu/legacy",
            "import_pool",
            "tank",
            "a b"
        ]
    );
    assert_eq!(bash.get_stdin(), &Input::Inherit);
    assert_eq!(bash.get_stdout(), Stream::Inherit);
    // The caller's BASE stays; the configuration fills in the rest.
    assert_eq!(bash.get_env(), [("loglevel".into(), Some("7".into()))]);
}

#[test]
fn without_the_legacy_files_or_bash_nothing_runs() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    assert_eq!(call(&fake, "", "import_pool", &[]), 1);
    assert!(fake.runner.lines().is_empty());

    let fake = image();
    fake.runner.on(|command| {
        (command.program() == "bash")
            .then(|| Err(std::io::Error::from(std::io::ErrorKind::NotFound)))
    });
    assert_eq!(call(&fake, "", "zreport", &[]), 127);
}

#[test]
fn a_restricted_pool_is_one_whole_line_of_degraded() {
    let fake = image();
    fake.environment.set("BASE", "/zfsbootmenu");
    // No file: grep's status.
    assert_eq!(call(&fake, "", "is_degraded", &["tank"]), 2);
    fake.files
        .file("/zfsbootmenu/degraded", "tank\nboot pool\n");
    assert_eq!(call(&fake, "", "is_degraded", &["tank"]), 0);
    assert_eq!(call(&fake, "", "is_degraded", &["boot pool"]), 0);
    assert_eq!(call(&fake, "", "is_degraded", &["tan"]), 1);
    assert_eq!(call(&fake, "", "is_degraded", &[]), 1);
    assert!(fake.runner.lines().is_empty(), "no legacy code ran");
}

/// ADR 0017, point 10, and ADR 0010: the recovery shell of a hook goes
/// through the gate: a locked image asks for the PIN first.
#[test]
fn the_emergency_shell_goes_through_the_gate() {
    use zbm_app::auth::{Authority, Lock};

    // This machine's image is not locked: the shell opens.
    let open = image();
    assert_eq!(call(&open, "", "emergency_shell", &[]), 0);
    assert!(open.console.output_text().contains("emergency shell"));

    let fake = image();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let locked = Authority::new(Lock::On(
        "$argon2id$v=19$m=65536,t=3,p=1$c2FsdA$aGFzaA".to_owned(),
    ));
    let shell = Shell::new(fake.system(), &state).with_authority(&locked);
    assert_eq!(open_shell(&shell, &["a hook asked".to_owned()]), 0);
    let lines = fake.runner.lines();
    assert!(
        lines.iter().any(|line| line.contains("lock shell")),
        "{lines:?}"
    );
    assert!(fake.console.output_text().contains("emergency shell"));
    assert!(fake.console.output_text().contains("a hook asked"));
}
