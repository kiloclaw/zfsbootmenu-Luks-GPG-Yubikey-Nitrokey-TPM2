use super::*;
use zbm_testkit::{FakeRunner, TempDir};

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_with(runner: &FakeRunner, mounts: &Path, args: &[&str], stdin: &[u8]) -> Run {
    let temp = TempDir::new("zbm-kcl-unit");
    let context = Context {
        runner,
        program: "zbm-kcl".to_owned(),
        mounts: mounts.to_owned(),
        temp_dir: temp.path().to_owned(),
        editor: None,
        path: None,
    };
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = run(
        &context,
        &args,
        &mut Io {
            stdin: &mut &stdin[..],
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    let leftovers = std::fs::read_dir(temp.path()).unwrap().count();
    assert_eq!(leftovers, 0, "temporary files are removed");
    Run {
        code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

#[test]
fn prints_the_property_of_the_root_filesystem() {
    let dir = TempDir::new("zbm-kcl-mounts");
    let mounts = dir.write("mounts", "rpool/ROOT/void / zfs rw 0 0\n");
    let runner = FakeRunner::new();
    runner.respond(
        "zfs get -H -o value org.zfsbootmenu:commandline rpool/ROOT/void",
        0,
        "quiet  \"a  b\" loglevel=4\n",
    );
    let result = run_with(&runner, &mounts, &[], b"");
    assert_eq!(result.code, 0);
    assert_eq!(result.stdout, "quiet \"a b\" loglevel=4\n");
    assert_eq!(
        runner.lines(),
        [
            "zfs list -o name -H rpool/ROOT/void",
            "zfs get -H -o value org.zfsbootmenu:commandline rpool/ROOT/void",
        ]
    );
}

#[test]
fn appends_and_removes_on_a_dataset() {
    let dir = TempDir::new("zbm-kcl-mounts");
    let mounts = dir.write("mounts", "tmpfs / tmpfs rw 0 0\n");
    let runner = FakeRunner::new();
    runner.respond(
        "zfs get -H -o value org.zfsbootmenu:commandline pool/be",
        0,
        "quiet loglevel=4 rw\n",
    );
    let result = run_with(
        &runner,
        &mounts,
        &["-r", "loglevel", "-a", "loglevel=7 nomodeset", "pool/be"],
        b"",
    );
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(
        runner.lines().last().unwrap(),
        "zfs set org.zfsbootmenu:commandline=quiet rw loglevel=7 nomodeset pool/be"
    );

    let runner = FakeRunner::new();
    let result = run_with(&runner, &mounts, &["-d", "pool/be"], b"");
    assert_eq!(result.code, 0);
    assert_eq!(
        runner.lines().last().unwrap(),
        "zfs inherit org.zfsbootmenu:commandline pool/be"
    );
}

#[test]
fn reports_bad_options_and_sources() {
    let dir = TempDir::new("zbm-kcl-mounts");
    let mounts = dir.write("mounts", "tmpfs / tmpfs rw 0 0\n");
    let runner = FakeRunner::new();
    let result = run_with(&runner, &mounts, &["-x"], b"");
    assert_eq!(result.code, 1);
    assert_eq!(result.stderr, "zbm-kcl: illegal option -- x\n");
    assert!(result.stdout.starts_with("USAGE: zbm-kcl [-a <arg>]"));

    assert_eq!(run_with(&runner, &mounts, &["-h", "-x"], b"").code, 0);

    let result = run_with(&runner, &mounts, &[], b"");
    assert_eq!(result.code, 1);
    assert_eq!(result.stderr, "ERROR: root does not appear to be ZFS\n");

    runner.respond("zfs list -o name -H nope", 1, "");
    let result = run_with(&runner, &mounts, &["nope"], b"");
    assert_eq!(result.code, 1);
    assert_eq!(
        result.stderr,
        "ERROR: KCL source is not a ZFS filesystem or EFI executable\n"
    );

    let result = run_with(&runner, &mounts, &["-e", "-"], b"MZ");
    assert_eq!(result.code, 1);
    assert_eq!(result.stderr, "ERROR: cannot edit KCL in streaming mode\n");
}

#[test]
fn editors_are_found_as_command_v_finds_them() {
    let dir = TempDir::new("zbm-kcl-editor");
    let editor = dir.write("ed", "");
    std::fs::set_permissions(&editor, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    assert!(command_exists(&editor.display().to_string(), None));
    assert!(!command_exists(
        "ed",
        Some(std::ffi::OsStr::new(":/nonexistent"))
    ));
    assert!(!command_exists("ed", None));
}

#[test]
fn strips_comments_and_stops_at_an_empty_line() {
    assert_eq!(
        strip_kcl("quiet # comment\n# whole line\nroot=x\t#x\n   \nlast\n\nignored\n"),
        "quiet root=x     last"
    );
    assert_eq!(strip_kcl("a#b"), "a");
    assert_eq!(strip_kcl(""), "");
    assert_eq!(read_arguments("a\n b \n"), ["a", "b"]);
}

#[test]
fn script_name_follows_bash() {
    let dir = TempDir::new("zbm-kcl-path");
    let program = dir.write("zbm-kcl", "");
    std::fs::set_permissions(
        &program,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let path = OsString::from(format!("/nonexistent:{}", dir.path().display()));
    assert_eq!(
        script_name("zbm-kcl", Some(&path)),
        program.display().to_string()
    );
    assert_eq!(script_name("./zbm-kcl", Some(&path)), "./zbm-kcl");
    assert_eq!(script_name("other", Some(&path)), "other");
}
