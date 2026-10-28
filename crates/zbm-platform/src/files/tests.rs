use super::*;
use zbm_testkit::TempDir;

#[test]
fn real_files_follow_the_shell_commands() {
    let dir = TempDir::new("zbm-files");
    let files = RealFiles;
    let file = dir.join("state/bootfs");
    assert!(files.write(&file, b"x").is_err());
    files.create_dir_all(&dir.join("state")).unwrap();
    files.write(&file, b"tank/ROOT/one\n").unwrap();
    files.append(&file, b"tank/ROOT/two\n").unwrap();
    assert_eq!(
        files.read_to_string(&file).unwrap(),
        "tank/ROOT/one\ntank/ROOT/two\n"
    );
    assert!(files.exists(&file) && files.is_file(&file) && !files.is_dir(&file));
    assert!(files.is_dir(&dir.join("state")));

    let metadata = files.metadata(&file).unwrap();
    assert_eq!(metadata.kind, FileKind::File);
    assert_eq!(metadata.len, 28);
    assert!(!metadata.is_executable());
    files.set_mode(&file, 0o755).unwrap();
    let metadata = files.metadata(&file).unwrap();
    assert_eq!(metadata.mode, 0o755);
    assert!(metadata.is_executable());
    assert_eq!(
        files.metadata(Path::new("/dev/null")).unwrap().kind,
        FileKind::CharDevice
    );
    let random = files.read_prefix(Path::new("/dev/urandom"), 32).unwrap();
    assert_eq!(random.len(), 32);
    assert_eq!(files.read_prefix(&file, 4).unwrap(), b"tank");

    let lock = dir.join("state/lock");
    files.create_dir(&lock).unwrap();
    assert_eq!(
        files.create_dir(&lock).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    files.remove_dir(&lock).unwrap();
    assert!(!files.exists(&lock));

    files.copy(&file, &dir.join("state/copy")).unwrap();
    files
        .rename(&dir.join("state/copy"), &dir.join("state/moved"))
        .unwrap();
    files.remove_file(&dir.join("state/moved")).unwrap();
    assert!(files.remove_file(&dir.join("state/moved")).is_err());
}

#[test]
fn directories_list_in_byte_order() {
    let dir = TempDir::new("zbm-files");
    for name in ["b", "B", "a.completed", "10-x", "9-y"] {
        dir.write(name, "");
    }
    let names = RealFiles.read_dir(dir.path()).unwrap();
    assert_eq!(names, ["10-x", "9-y", "B", "a.completed", "b"]);
}

#[test]
fn replacing_leaves_no_temporary_file() {
    let dir = TempDir::new("zbm-files");
    let file = dir.join("kernels");
    RealFiles.write(&file, b"old").unwrap();
    RealFiles.replace(&file, b"new").unwrap();
    assert_eq!(RealFiles.read(&file).unwrap(), b"new");
    assert_eq!(RealFiles.read_dir(dir.path()).unwrap(), ["kernels"]);
    assert!(RealFiles.replace(Path::new("/"), b"").is_err());
    assert!(RealFiles.replace(&dir.join("missing/file"), b"").is_err());

    // A rename that fails takes its temporary file with it.
    dir.write("full/entry", "");
    assert!(RealFiles.replace(&dir.join("full"), b"x").is_err());
    assert_eq!(RealFiles.read_dir(dir.path()).unwrap(), ["full", "kernels"]);
}

#[test]
fn private_files_are_new_mode_0600_and_never_through_a_link() {
    let dir = TempDir::new("zbm-files");
    let key = dir.join("key");
    RealFiles.write_private(&key, b"secret").unwrap();
    assert_eq!(RealFiles.read(&key).unwrap(), b"secret");
    assert_eq!(RealFiles.metadata(&key).unwrap().mode, 0o600);
    let again = RealFiles.write_private(&key, b"other").unwrap_err();
    assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);

    dir.write("target", "kept");
    std::os::unix::fs::symlink(dir.join("target"), dir.join("link")).unwrap();
    std::os::unix::fs::symlink(dir.join("nowhere"), dir.join("dangling")).unwrap();
    for link in ["link", "dangling"] {
        let error = RealFiles.write_private(&dir.join(link), b"x").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{link}");
    }
    assert_eq!(RealFiles.read(&dir.join("target")).unwrap(), b"kept");
    assert!(!dir.join("nowhere").exists());
}

#[test]
fn the_kernel_log_is_read_without_waiting_at_its_end() {
    // Where the kernel log may not be read, there is nothing to check.
    let Ok(raw) = RealFiles.read_available(Path::new("/dev/kmsg"), 1 << 20) else {
        return;
    };
    let text = String::from_utf8_lossy(&raw);
    let lines = text.lines().filter(|line| !line.starts_with(' ')).count();
    assert_eq!(crate::dmesg::records(&raw).len(), lines, "{text}");
    let file = TempDir::new("zbm-files");
    file.write("log", "plain");
    let plain = RealFiles.read_available(&file.join("log"), 3).unwrap();
    assert_eq!(plain, b"pla");
}

#[test]
fn a_lock_is_held_until_it_is_dropped_and_shows_to_others() {
    let dir = TempDir::new("zbm-files");
    let path = dir.join("state.lock");
    assert!(!RealFiles.is_locked(&path));
    let held = RealFiles.lock(&path, LockWait::Block).unwrap().unwrap();
    assert_eq!(RealFiles.metadata(&path).unwrap().mode, 0o600);
    assert!(RealFiles.is_locked(&path));
    assert!(RealFiles.lock(&path, LockWait::Try).unwrap().is_none());
    drop(held);
    assert!(!RealFiles.is_locked(&path));
    assert!(RealFiles.lock(&path, LockWait::Try).unwrap().is_some());
}

#[test]
fn special_files_have_their_kinds() {
    let dir = TempDir::new("zbm-files");
    let kind = |path: &Path| RealFiles.metadata(path).unwrap().kind;
    let _socket = std::os::unix::net::UnixListener::bind(dir.join("socket")).unwrap();
    let fifo = std::process::Command::new("mkfifo")
        .arg(dir.join("fifo"))
        .status()
        .unwrap();
    assert!(fifo.success());

    assert_eq!(kind(&dir.join("socket")), FileKind::Socket);
    assert_eq!(kind(&dir.join("fifo")), FileKind::Other);
    assert_eq!(kind(Path::new("/dev/null")), FileKind::CharDevice);
    let block = std::fs::read_dir("/dev")
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_type().is_ok_and(|kind| kind.is_block_device()));
    if let Some(block) = block {
        assert_eq!(kind(&block.path()), FileKind::BlockDevice);
    }
}

#[test]
fn canonical_paths_have_no_links() {
    let dir = TempDir::new("zbm-files");
    dir.write("target", "");
    std::os::unix::fs::symlink(dir.join("target"), dir.join("link")).unwrap();
    let canonical = RealFiles.canonicalize(&dir.join("./link")).unwrap();
    assert_eq!(
        canonical,
        RealFiles.canonicalize(&dir.join("target")).unwrap()
    );
}

#[test]
fn the_mount_system_calls_report_their_errors() {
    // A mount point that does not exist: nothing can be mounted on it or
    // unmounted from it, whoever runs the test.
    let missing = Path::new("/nonexistent/zbm-mount-test");
    assert!(
        RealFiles
            .mount_zfs("tank/ROOT/void", missing, true)
            .is_err()
    );
    assert!(
        RealFiles
            .mount_zfs("tank/ROOT/void", missing, false)
            .is_err()
    );
    assert!(RealFiles.unmount(missing).is_err());
    let nul = Path::new("/nonexistent/a\0b");
    assert!(RealFiles.mount_zfs("tank", nul, true).is_err());
}
