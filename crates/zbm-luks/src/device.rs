//! Finding LUKS volumes: resolving a device specification to its device,
//! telling whether a volume is open, and naming and describing volumes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use zbm_crypt::LuksVolumes;
use zbm_platform::Context;
use zbm_platform::files::{FileKind, Files};
use zbm_tools::devices::Devices;
use zbm_types::{DevicePath, MappingName, ParseError, Printable};

/// The `/dev/disk` directory of each `KEY=value` device specification.
const BY_KEY: [(&str, &str); 6] = [
    ("UUID", "/dev/disk/by-uuid/"),
    ("PARTUUID", "/dev/disk/by-partuuid/"),
    ("LABEL", "/dev/disk/by-label/"),
    ("PARTLABEL", "/dev/disk/by-partlabel/"),
    ("ID", "/dev/disk/by-id/"),
    ("PATH", "/dev/disk/by-path/"),
];

/// Whether `path` is a block device, following links, as `[ -b ]` tests.
pub fn is_block_device(files: &dyn Files, path: &str) -> bool {
    !path.is_empty()
        && files
            .metadata(Path::new(path))
            .is_ok_and(|metadata| metadata.kind == FileKind::BlockDevice)
}

/// `readlink -f` of an existing path; the path itself if that fails.
pub fn canonical(files: &dyn Files, path: &str) -> String {
    files.canonicalize(Path::new(path)).map_or_else(
        |_| path.to_owned(),
        |path| path.to_string_lossy().into_owned(),
    )
}

/// Whether `text` is a UUID, as the legacy code matches it with
/// `^[[:xdigit:]]{8}-([[:xdigit:]]{4}-){3}[[:xdigit:]]{12}$`.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// What `blkid -c /dev/null -l -o device -t KEY=value` prints, without the
/// final newlines.
fn blkid_device(context: &Context<'_>, key: &str, value: &str) -> String {
    Devices::new(context.runner(), context.files()).find(key, value)
}

/// The device node a device specification names: `/dev/...`, `UUID=`,
/// `PARTUUID=`, `LABEL=`, `PARTLABEL=`, `ID=`, `PATH=` (the key in any case),
/// a bare LUKS UUID or `luks-<UUID>`. Returns the canonical path of the block
/// device, or `None` (after logging an error for a specification it does not
/// understand).
pub fn resolve_device(context: &Context<'_>, spec: &str) -> Option<DevicePath> {
    if spec.is_empty() {
        return None;
    }
    let unrecognized = || {
        zbm_trace::error!("unrecognized LUKS device specification '{spec}'"; %spec);
        None
    };
    let (key, value, mut device) = if spec.starts_with("/dev/") {
        (String::new(), String::new(), spec.to_owned())
    } else if let Some((key, value)) = spec.split_once('=') {
        let key = key.to_uppercase();
        let Some((_, dir)) = BY_KEY.iter().find(|(name, _)| *name == key) else {
            return unrecognized();
        };
        let device = format!("{dir}{value}");
        (key, value.to_owned(), device)
    } else {
        // A bare UUID or the systemd-style luks-<UUID> mapping name.
        let value = spec.strip_prefix("luks-").unwrap_or(spec);
        if !is_uuid(value) {
            return unrecognized();
        }
        let device = format!("/dev/disk/by-uuid/{value}");
        ("UUID".to_owned(), value.to_owned(), device)
    };

    // LUKS and GPT UUIDs are lower case, FAT volume IDs upper case; tools such
    // as sgdisk print GPT UUIDs in upper case, so either is accepted.
    let lower = value.to_lowercase();
    let files = context.files();
    if !is_block_device(files, &device)
        && let Some(link) = lower_case_link(files, &key, &lower)
    {
        device = link;
    }

    // udev links may be missing or escaped differently; blkid finds the
    // device itself.
    if !is_block_device(files, &device)
        && matches!(key.as_str(), "UUID" | "PARTUUID" | "LABEL" | "PARTLABEL")
    {
        device = blkid_device(context, &key, &value);
        if device.is_empty() && value != lower && matches!(key.as_str(), "UUID" | "PARTUUID") {
            device = blkid_device(context, &key, &lower);
        }
    }

    is_block_device(files, &device)
        .then(|| canonical(files, &device))?
        .parse()
        .ok()
}

/// The `/dev/disk/by-uuid` or `by-partuuid` link of the UUID `lower`, in
/// lower case, for the key `key`, when it is a block device.
fn lower_case_link(files: &dyn Files, key: &str, lower: &str) -> Option<String> {
    let dir = match key {
        "UUID" => "/dev/disk/by-uuid/",
        "PARTUUID" => "/dev/disk/by-partuuid/",
        _ => return None,
    };
    let link = format!("{dir}{lower}");
    is_block_device(files, &link).then_some(link)
}

/// Whether the device a specification names exists.
pub fn device_present(context: &Context<'_>, spec: &str) -> bool {
    resolve_device(context, spec).is_some()
}

/// How to tell whether a volume is open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenCheck {
    /// Something holds the device in `/sys/class/block/<device>/holders`, such
    /// as an active dm-crypt mapping.
    Sysfs,
    /// A file named after the device exists in this directory: the stand-in
    /// of the LUKS unit suites, whose stub `cryptsetup` keeps it.
    Markers(PathBuf),
}

/// Whether something holds `device`.
pub fn is_open(files: &dyn Files, device: &str, check: &OpenCheck) -> bool {
    let Ok(device) = files.canonicalize(Path::new(device)) else {
        return false;
    };
    let Some(name) = device.file_name() else {
        return false;
    };
    match check {
        OpenCheck::Sysfs => {
            let holders = Path::new("/sys/class/block").join(name).join("holders");
            // `for holder in .../holders/*; do [ -e "${holder}" ]`
            files.read_dir(&holders).is_ok_and(|names| {
                names.iter().any(|holder| {
                    !holder.as_encoded_bytes().starts_with(b".")
                        && files.exists(&holders.join(holder))
                })
            })
        }
        OpenCheck::Markers(dir) => files.exists(&dir.join(name)),
    }
}

/// The default mapping name of a LUKS device: `luks-<UUID>`, or
/// `luks-<device name>` when the UUID cannot be read.
///
/// # Errors
///
/// When that is not a mapping name: the UUID comes from the header, which
/// whoever wrote the device chose.
pub fn default_name(volumes: &dyn LuksVolumes, header: &Path) -> Result<MappingName, ParseError> {
    match volumes.uuid(header) {
        Some(uuid) if !uuid.is_empty() => format!("luks-{uuid}"),
        // `${device##*/}`: empty after a trailing slash.
        _ => format!(
            "luks-{}",
            header
                .to_string_lossy()
                .rsplit('/')
                .next()
                .unwrap_or_default()
        ),
    }
    .parse()
}

/// The name a volume is opened as: `name` when given,
/// else the default name of its LUKS `header`, which is the device itself
/// unless crypttab gives a detached one. The unlock use cases name volumes
/// with it.
///
/// # Errors
///
/// When the default name is not a mapping name.
pub fn mapping_name(
    volumes: &dyn LuksVolumes,
    name: Option<&MappingName>,
    header: &Path,
) -> Result<MappingName, ParseError> {
    match name {
        Some(name) => Ok(name.clone()),
        None => default_name(volumes, header),
    }
}

/// A short description of a device for prompts: the device, its label or
/// partition label in quotes, and its size. Whoever made the device chose
/// its label (attacker A2 of `docs/dev/threat-model.md`), so the label and
/// the device are shown as [`Printable`]: no control character in them
/// reaches the console.
pub fn describe_device(context: &Context<'_>, device: &str) -> String {
    let devices = Devices::new(context.runner(), context.files());
    let size = devices.size(device);
    let label = devices.tag(device, "LABEL");
    let partlabel = devices.tag(device, "PARTLABEL");
    let mut description = Printable(device).to_string();
    let name = if label.is_empty() { &partlabel } else { &label };
    if !name.is_empty() {
        let _ = write!(description, " \"{}\"", Printable(name));
    }
    if !size.is_empty() {
        description.push_str(&[" (", &size, ")"].concat());
    }
    description
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{legacy_function, legacy_luks, log};
    use zbm_crypt::Cryptsetup;
    use zbm_platform::exec::{Output, Status};
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::{log_lines, stub_lines};
    use zbm_testkit::{TempDir, parity};

    const UUID: &str = "0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1b";
    const UPPER: &str = "0F2C8BB6-2D5E-4F87-9A6B-9A7C1FEF8D1B";

    /// A machine with `/dev/sda1` (a LUKS volume with links by UUID, label and
    /// partition UUID) and `/dev/sdb`, whose `blkid` finds `LABEL=blk` and the
    /// upper-case partition UUID `ABC`, in lower case only.
    fn machine() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.files
            .block_device("/dev/sda1")
            .block_device("/dev/sdb")
            .file("/dev/notblock", "")
            .symlink(format!("/dev/disk/by-uuid/{UUID}"), "../../sda1")
            .symlink("/dev/disk/by-label/root", "../../sda1")
            .symlink("/dev/disk/by-partuuid/abc", "../../sda1")
            .symlink("/dev/disk/by-id/ata-disk", "../../sdb")
            .symlink("/dev/disk/by-path/pci-0", "../../sdb");
        fake.runner.on(|command| {
            if command.program() != "blkid" {
                return None;
            }
            let last = command.get_args().last()?.to_string_lossy().into_owned();
            let found = match last.as_str() {
                "LABEL=blk" | "PARTUUID=def" => "/dev/sdb\n\n",
                "LABEL=gone" => "/dev/missing\n",
                _ => "",
            };
            Some(Ok(Output::new(
                Status::exited(if found.is_empty() { 2 } else { 0 }),
                found,
            )))
        });
        fake
    }

    #[test]
    fn specifications_name_devices() {
        let fake = machine();
        let context = fake.context();
        let _log = log(&fake);
        let resolve = |spec: &str| resolve_device(&context, spec).map(|device| device.to_string());
        let sda = Some("/dev/sda1".to_owned());
        let sdb = Some("/dev/sdb".to_owned());
        assert_eq!(resolve("/dev/sda1"), sda);
        assert_eq!(resolve("/dev/disk/by-label/root"), sda);
        assert_eq!(resolve(&format!("UUID={UUID}")), sda);
        assert_eq!(resolve(&format!("uuid={UPPER}")), sda);
        assert_eq!(resolve(UUID), sda);
        assert_eq!(resolve(&format!("luks-{UPPER}")), sda);
        assert_eq!(resolve("Label=root"), sda);
        assert_eq!(resolve("PARTUUID=ABC"), sda);
        assert_eq!(resolve("ID=ata-disk"), sdb);
        assert_eq!(resolve("PATH=pci-0"), sdb);
        assert_eq!(resolve("LABEL=blk"), sdb);
        assert_eq!(resolve("PARTUUID=DEF"), sdb);
        assert_eq!(resolve("LABEL=gone"), None);
        assert_eq!(resolve("ID=missing"), None);
        assert_eq!(resolve("/dev/notblock"), None);
        assert_eq!(resolve(""), None);
        assert!(!fake.kmsg().contains("ERROR") && !fake.kmsg().contains("<3>"));
        assert_eq!(resolve("BOGUS=1"), None);
        assert_eq!(resolve("notauuid"), None);
        assert_eq!(
            resolve("luks-luks-0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1b"),
            None
        );
        assert_eq!(
            stub_lines(&fake.kmsg()),
            "ERROR: unrecognized LUKS device specification 'BOGUS=1'\n\
             ERROR: unrecognized LUKS device specification 'notauuid'\n\
             ERROR: unrecognized LUKS device specification \
             'luks-luks-0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1b'\n"
        );
        // blkid only for the keys it knows, and again in lower case for UUIDs.
        let blkid: Vec<String> = fake
            .runner
            .lines()
            .into_iter()
            .filter(|line| line.starts_with("blkid"))
            .collect();
        assert_eq!(
            blkid,
            [
                "blkid -c /dev/null -l -o device -t LABEL=blk",
                "blkid -c /dev/null -l -o device -t PARTUUID=DEF",
                "blkid -c /dev/null -l -o device -t PARTUUID=def",
                "blkid -c /dev/null -l -o device -t LABEL=gone",
            ]
        );
        assert!(device_present(&context, "LABEL=root"));
        assert!(!device_present(&context, "LABEL=none"));
    }

    /// The legacy function, with `/dev/disk` moved below `$ROOT` and `-b`
    /// replaced by `-e`, so that plain files and links stand in for devices.
    fn legacy_resolve() -> String {
        legacy_function("zfsbootmenu/lib/zfsbootmenu-luks.sh", "luks_resolve_device")
            .replace("\"/dev/disk/", "\"${ROOT}/dev/disk/")
            .replace("[ -b ", "[ -e ")
            .replace("[ ! -b ", "[ ! -e ")
    }

    /// What the legacy resolver prints for `spec`, without the directory that
    /// stands in for `/`, whether it succeeds, and its messages.
    fn legacy_resolution(spec: &str) -> (String, bool, String) {
        let root = TempDir::new("zbm-luks-resolve");
        let root_text = root.path().to_string_lossy().into_owned();
        // The same tree for Bash, with `blkid` answering as above.
        root.write("dev/sda1", "");
        root.write("dev/sdb", "");
        for (link, target) in [
            (format!("dev/disk/by-uuid/{UUID}"), "../../sda1"),
            ("dev/disk/by-label/root".to_owned(), "../../sda1"),
            ("dev/disk/by-partuuid/abc".to_owned(), "../../sda1"),
            ("dev/disk/by-id/ata-disk".to_owned(), "../../sdb"),
        ] {
            let link = root.join(link);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(target, link).unwrap();
        }
        let blkid = format!(
            r#"blkid() {{
                 case "${{@: -1}}" in
                   LABEL=blk|PARTUUID=def) printf '%s\n\n' "{root_text}/dev/sdb" ;;
                 esac
               }}"#
        );
        let legacy = legacy_luks()
            .code(&blkid)
            .code(&legacy_resolve())
            .env("ROOT", &root_text)
            .call("luks_resolve_device", &[spec], b"");
        (
            legacy.stdout_text().replace(&root_text, ""),
            legacy.status.is_success(),
            log_lines(&legacy.stderr_text()).replace(&root_text, ""),
        )
    }

    #[test]
    fn specifications_match_the_legacy_resolver() {
        let mut parity = parity!("specifications_match_the_legacy_resolver");
        // `/dev/...` stays outside `$ROOT`: the unit test above covers it.
        for spec in [
            format!("UUID={UUID}"),
            format!("uuid={UPPER}"),
            UUID.to_owned(),
            format!("luks-{UPPER}"),
            "Label=root".to_owned(),
            "PARTUUID=ABC".to_owned(),
            "ID=ata-disk".to_owned(),
            "LABEL=blk".to_owned(),
            "PARTUUID=DEF".to_owned(),
            "partlabel=none".to_owned(),
            "ID=missing".to_owned(),
            "BOGUS=1".to_owned(),
            "notauuid".to_owned(),
            "0f2c8bb6-2d5e-4f87-9a6b".to_owned(),
            "=x".to_owned(),
            String::new(),
        ] {
            let (expected, success, logged): (String, bool, String) =
                parity.legacy(format!("{spec:?}"), || legacy_resolution(&spec));
            let fake = machine();
            let _log = log(&fake);
            let rust = resolve_device(&fake.context(), &spec);
            assert_eq!(
                rust.map(|device| format!("{device}\n")).unwrap_or_default(),
                expected,
                "{spec:?}"
            );
            assert_eq!(success, !expected.is_empty(), "{spec:?}");
            assert_eq!(stub_lines(&fake.kmsg()), logged, "{spec:?}");
        }
    }

    #[test]
    fn open_volumes_have_holders() {
        let fake = FakeSystem::new();
        fake.files
            .block_device("/dev/sda1")
            .block_device("/dev/sdb")
            .symlink("/dev/disk/by-label/root", "../../sda1")
            .dir("/sys/class/block/sda1/holders")
            .symlink("/sys/class/block/sda1/holders/dm-0", "../../dm-0")
            .dir("/sys/class/block/dm-0")
            .dir("/sys/class/block/sdb/holders")
            .file("/sys/class/block/sdb/holders/.hidden", "");
        let files: &dyn Files = &fake.files;
        assert!(is_open(files, "/dev/disk/by-label/root", &OpenCheck::Sysfs));
        assert!(!is_open(files, "/dev/sdb", &OpenCheck::Sysfs));
        assert!(!is_open(files, "/dev/missing", &OpenCheck::Sysfs));

        let markers = OpenCheck::Markers(PathBuf::from("/fake/holders"));
        assert!(!is_open(files, "/dev/disk/by-label/root", &markers));
        fake.files.file("/fake/holders/sda1", "v\n");
        assert!(is_open(files, "/dev/disk/by-label/root", &markers));
    }

    /// Stand-ins for `cryptsetup`, `lsblk` and `blkid`, for both
    /// implementations: `$1` of each answer is the device.
    fn describe_machine() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.runner
            .respond("cryptsetup luksUUID /dev/sda1", 0, &format!("{UUID}\n"));
        fake.runner.respond("cryptsetup luksUUID /dev/sdb", 0, "\n");
        fake.runner.respond_program("cryptsetup", 1, "junk\n");
        fake.runner
            .respond("lsblk -dno SIZE /dev/sda1", 0, " 1.8T\n");
        fake.runner
            .respond("lsblk -dno SIZE /dev/sdb", 0, "5 G\n\n");
        fake.runner.respond_program("lsblk", 32, "");
        fake.runner.respond(
            "blkid -c /dev/null -s LABEL -o value /dev/sda1",
            0,
            "rpool\n",
        );
        fake.runner.respond(
            "blkid -c /dev/null -s PARTLABEL -o value /dev/sda1",
            0,
            "p\n",
        );
        fake.runner.respond(
            "blkid -c /dev/null -s PARTLABEL -o value /dev/sdb",
            0,
            "data disk\n",
        );
        fake.runner.respond_program("blkid", 2, "");
        fake
    }

    const DESCRIBE_STUBS: &str = r#"
        cryptsetup() { case "$2" in
          /dev/sda1) echo 0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1b ;;
          /dev/sdb) echo ;;
          *) echo junk; return 1 ;; esac; }
        lsblk() { case "$3" in
          /dev/sda1) echo " 1.8T" ;;
          /dev/sdb) printf '5 G\n\n' ;;
          *) return 32 ;; esac; }
        blkid() { case "$4 $7" in
          "LABEL /dev/sda1") echo rpool ;;
          "PARTLABEL /dev/sda1") echo p ;;
          "PARTLABEL /dev/sdb") echo "data disk" ;;
          *) return 2 ;; esac; }
    "#;

    #[test]
    fn a_header_cannot_name_its_mapping_out_of_the_mapper() {
        // The UUID comes from the header, which whoever wrote the device
        // chose: a path out of /dev/mapper, a second word, a terminal
        // escape sequence.
        for uuid in ["../../etc", "a b", "\u{1b}[2J"] {
            let fake = FakeSystem::new();
            fake.runner
                .respond("cryptsetup luksUUID /dev/sda1", 0, &format!("{uuid}\n"));
            assert!(
                default_name(&Cryptsetup::new(&fake.runner), Path::new("/dev/sda1")).is_err(),
                "{uuid}"
            );
        }
        let fake = FakeSystem::new();
        let given: MappingName = "cryptroot".parse().unwrap();
        assert_eq!(
            mapping_name(
                &Cryptsetup::new(&fake.runner),
                Some(&given),
                Path::new("/dev/sda1")
            ),
            Ok(given)
        );
    }

    #[test]
    fn names_and_descriptions_match_the_legacy_code() {
        let mut parity = parity!("names_and_descriptions_match_the_legacy_code");
        for device in ["/dev/sda1", "/dev/sdb", "/dev/mapper/x", "/dev/dir/"] {
            let fake = describe_machine();
            let context = fake.context();
            for (function, rust) in [
                (
                    "luks_default_name",
                    default_name(&Cryptsetup::new(context.runner()), Path::new(device))
                        .map_or_else(|_| String::new(), |name| name.to_string()),
                ),
                ("luks_describe_device", describe_device(&context, device)),
            ] {
                let legacy: String = parity.legacy(format!("{function} {device}"), || {
                    legacy_luks()
                        .code(DESCRIBE_STUBS)
                        .call(function, &[device], b"")
                        .stdout_text()
                });
                assert_eq!(format!("{rust}\n"), legacy, "{function} {device}");
            }
        }
        let fake = describe_machine();
        assert_eq!(
            default_name(&Cryptsetup::new(&fake.runner), Path::new("/dev/sda1"))
                .unwrap()
                .to_string(),
            format!("luks-{UUID}")
        );
        assert_eq!(
            describe_device(&fake.context(), "/dev/sda1"),
            "/dev/sda1 \"rpool\" (1.8T)"
        );
        assert_eq!(
            describe_device(&fake.context(), "/dev/sdb"),
            "/dev/sdb \"data disk\" (5G)"
        );
    }

    #[test]
    fn uuids_have_five_hex_groups() {
        assert!(is_uuid(UUID) && is_uuid(UPPER));
        assert!(!is_uuid("0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1"));
        assert!(!is_uuid("0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1bb"));
        assert!(!is_uuid("0f2c8bb6-2d5e-4f87-9a6b9a7c1fef8d1b"));
        assert!(!is_uuid("0g2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1b"));
        assert!(!is_uuid("0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1b-"));
    }
}
