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
    // A name with spaces is no kernel argument: left out, and said.
    assert_eq!(result.stdout, "quiet loglevel=4\n");
    assert_eq!(
        result.stderr,
        "ERROR: left out '\"a  b\"', which is not a kernel argument\n"
    );
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
fn removes_by_name_or_whole_argument_and_leaves_out_what_is_not_one() {
    let dir = TempDir::new("zbm-kcl-mounts");
    let mounts = dir.write("mounts", "tmpfs / tmpfs rw 0 0\n");
    let runner = FakeRunner::new();
    runner.respond(
        "zfs get -H -o value org.zfsbootmenu:commandline pool/be",
        0,
        "console=tty0 console=ttyS0 quiet\n",
    );
    let result = run_with(
        &runner,
        &mounts,
        &["-r", "console=ttyS0", "-a", "x\"y", "-a", "rw", "pool/be"],
        b"",
    );
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(
        runner.lines().last().unwrap(),
        "zfs set org.zfsbootmenu:commandline=console=tty0 quiet rw pool/be"
    );
    assert_eq!(
        result.stderr,
        "ERROR: left out 'x\"y', which is not a kernel argument\n"
    );
}

#[test]
fn an_image_is_replaced_whole_and_keeps_its_mode() {
    let dir = TempDir::new("zbm-kcl-mounts");
    let mounts = dir.write("mounts", "tmpfs / tmpfs rw 0 0\n");
    let output = dir.write("out.efi", "old image");
    std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o600)).unwrap();
    let output_arg = output.display().to_string();
    let runner = FakeRunner::new();
    // objdump fails: nothing is written, and the old image stays.
    runner.respond_program("objdump", 1, "");
    let result = run_with(
        &runner,
        &mounts,
        &["-a", "quiet", "-o", &output_arg, "-"],
        b"MZ",
    );
    assert_eq!(result.code, 1);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old image");

    let image = dir.write("new.efi", "new image");
    assert_eq!(replace_image(&image, &output), Ok(()));
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "new image");
    let mode = std::fs::metadata(&output).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let missing = dir.join("none/out.efi");
    let failure = replace_image(&image, &missing).unwrap_err();
    assert!(failure.starts_with("cannot replace"), "{failure}");
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

#[test]
fn failures_to_save_or_edit_say_why_then_that_the_changes_were_not_saved() {
    let dir = TempDir::new("zbm-kcl-mounts");
    let mounts = dir.write("mounts", "tmpfs / tmpfs rw 0 0\n");

    let runner = FakeRunner::new();
    runner.respond("zfs set org.zfsbootmenu:commandline=quiet pool/be", 1, "");
    let set = run_with(&runner, &mounts, &["-a", "quiet", "pool/be"], b"");
    assert_eq!(set.code, 1);
    assert_eq!(
        set.stderr,
        "ERROR: failed to set KCL property on pool/be\nERROR: failed to save KCL changes\n"
    );

    let runner = FakeRunner::new();
    runner.respond("zfs inherit org.zfsbootmenu:commandline pool/be", 1, "");
    let clear = run_with(&runner, &mounts, &["-d", "pool/be"], b"");
    assert_eq!(
        clear.stderr,
        "ERROR: failed to clear KCL property on pool/be\nERROR: failed to save KCL changes\n"
    );

    let edit = run_with(&FakeRunner::new(), &mounts, &["-e", "pool/be"], b"");
    assert_eq!(
        (edit.code, edit.stderr.as_str()),
        (1, "ERROR: define $EDITOR to edit\n")
    );
}

#[test]
fn an_unreadable_image_cannot_take_a_new_command_line() {
    let dir = TempDir::new("zbm-kcl-mounts");
    let mounts = dir.write("mounts", "tmpfs / tmpfs rw 0 0\n");
    let runner = FakeRunner::new();
    runner.respond_program("objdump", 1, "");

    let result = run_with(&runner, &mounts, &["-a", "quiet", "-"], b"MZ");

    assert_eq!(result.code, 1);
    assert!(
        result.stderr.ends_with(
            "ERROR: failed to parse object file; is objdump available?\n\
             ERROR: failed to determine offset data for KCL\n\
             ERROR: failed to save KCL changes\n"
        ),
        "{}",
        result.stderr
    );
}
