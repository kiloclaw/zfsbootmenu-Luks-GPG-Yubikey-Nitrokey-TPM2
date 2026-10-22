//! The programs of a [`FakeMachine`](super::FakeMachine), answering from its
//! model.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use zbm_core::exec::{Command, Input, Output, Status};
use zbm_system::files::Files as _;

use super::{BUSY, HOLDERS, LuksVolume, MAPS, Model, base};
use crate::FakeRunner;
use crate::fakes::FakeFiles;

/// Adds the responders of the machine's programs to `runner`.
pub(super) fn install(runner: &FakeRunner, files: &FakeFiles, model: &Rc<RefCell<Model>>) {
    let files = files.clone();
    let model = Rc::clone(model);
    runner.on(move |command| {
        let args = args(command);
        let model = model.borrow();
        let output = match command.program().to_string_lossy().as_ref() {
            "cryptsetup" => cryptsetup(&files, &model.volumes, command, &args),
            "blkid" => blkid(&files, &model.volumes, &args)?,
            "lsblk" => Output::new(Status::exited(0), "1G\n"),
            "zpool" => zpool(&model, &args)?,
            _ => return None,
        };
        Some(Ok(output))
    });
}

/// The arguments of `command` as text.
fn args(command: &Command) -> Vec<String> {
    command
        .get_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// An output with exit status `code` and nothing on standard output.
fn status(code: u8) -> Output {
    Output::new(Status::exited(code), "")
}

/// The argument at `index`, or the empty string.
fn arg(args: &[String], index: usize) -> &str {
    args.get(index).map_or("", String::as_str)
}

/// `cryptsetup`, with the exit codes of `cryptsetup(8)`.
fn cryptsetup(
    files: &FakeFiles,
    volumes: &[LuksVolume],
    command: &Command,
    args: &[String],
) -> Output {
    let volume = |device: &str| volumes.iter().find(|volume| volume.device == device);
    let last = |back: usize| arg(args, args.len().saturating_sub(back));
    match arg(args, 0) {
        "isLuks" => status(u8::from(volume(arg(args, 1)).is_none())),
        "luksUUID" => volume(arg(args, 1)).map_or_else(
            || status(1),
            |volume| Output::new(Status::exited(0), format!("{}\n", volume.uuid)),
        ),
        "luksDump" => volume(last(1))
            .and_then(|volume| volume.metadata.clone())
            .filter(|_| args.iter().any(|arg| arg == "--dump-json-metadata"))
            .map_or_else(|| status(1), |json| Output::new(Status::exited(0), json)),
        "open" => {
            let (device, name) = (last(2), last(1));
            if files.exists(Path::new(&format!("/dev/mapper/{name}"))) {
                return status(5);
            }
            match volume(device) {
                Some(volume) if volume.passphrase == key(files, command, args) => {
                    open(files, volume, name);
                    status(0)
                }
                _ => Output::new(
                    Status::exited(2),
                    "No key available with this passphrase.\n",
                ),
            }
        }
        "close" => close(files, volumes, arg(args, 1)),
        _ => status(0),
    }
}

/// The key `cryptsetup open` was given: standard input with
/// `--key-file=-`, else the key file's contents.
fn key(files: &FakeFiles, command: &Command, args: &[String]) -> Vec<u8> {
    let path = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--key-file="))
        .unwrap_or_default();
    if path == "-" {
        return match command.get_stdin() {
            Input::Secret(secret) => secret.expose().to_vec(),
            Input::Bytes(bytes) => bytes.clone(),
            Input::Inherit | Input::Null => Vec::new(),
        };
    }
    files.read(Path::new(path)).unwrap_or_default()
}

/// Leaves `volume` open as `name`.
pub(super) fn open(files: &FakeFiles, volume: &LuksVolume, name: &str) {
    files.block_device(format!("/dev/mapper/{name}"));
    files.file(
        format!("{HOLDERS}/{}", base(&volume.device)),
        format!("{name}\n"),
    );
    files.file(format!("{MAPS}/{name}"), &volume.content);
}

/// `cryptsetup close NAME`: 5 when busy, 4 when not open.
fn close(files: &FakeFiles, volumes: &[LuksVolume], name: &str) -> Output {
    let mapper = format!("/dev/mapper/{name}");
    if files.exists(Path::new(&format!("{BUSY}/{name}"))) {
        return status(5);
    }
    if !files.exists(Path::new(&mapper)) {
        return status(4);
    }
    let _ = files.remove_file(Path::new(&mapper));
    let _ = files.remove_file(Path::new(&format!("{MAPS}/{name}")));
    for volume in volumes {
        let marker = format!("{HOLDERS}/{}", base(&volume.device));
        let holder = files.read_to_string(Path::new(&marker)).ok();
        if holder.as_deref() == Some(&format!("{name}\n")) {
            let _ = files.remove_file(Path::new(&marker));
        }
    }
    status(0)
}

/// `blkid -t TYPE=crypto_LUKS` and `blkid -p DEVICE`; other uses are left to
/// other responders.
fn blkid(files: &FakeFiles, volumes: &[LuksVolume], args: &[String]) -> Option<Output> {
    if args.iter().any(|arg| arg == "TYPE=crypto_LUKS") {
        let list: String = volumes
            .iter()
            .flat_map(|volume| [volume.device.as_str(), "\n"])
            .collect();
        return Some(Output::new(Status::exited(0), list));
    }
    if args.iter().any(|arg| arg == "-p") {
        let name = base(args.last()?);
        let content = files
            .read_to_string(Path::new(&format!("{MAPS}/{name}")))
            .ok()?;
        return Some(Output::new(Status::exited(0), format!("{content}\n")));
    }
    None
}

/// `zpool list` and `zpool status POOL`; other uses are left to other
/// responders.
fn zpool(model: &Model, args: &[String]) -> Option<Output> {
    match arg(args, 0) {
        "list" => {
            let names: String = model
                .pools
                .iter()
                .flat_map(|pool| [pool.name.as_str(), "\n"])
                .collect();
            Some(Output::new(Status::exited(0), names))
        }
        "status" => {
            let name = args.last()?;
            let pool = model.pools.iter().find(|pool| &pool.name == name)?;
            Some(Output::new(
                Status::exited(0),
                format!(
                    "  pool: {}\nconfig:\n\t  {}  ONLINE 0 0 0\n",
                    pool.name, pool.vdev
                ),
            ))
        }
        _ => None,
    }
}
