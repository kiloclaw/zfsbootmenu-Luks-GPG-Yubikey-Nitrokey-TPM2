//! The decisions shadow mode compares, one function each. Each reads what
//! the legacy runtime did and works out what the Rust code would have done;
//! `None` when there is nothing to compare.

use std::path::Path;

use zbm_config::args::{Args, CmdlineDir};
use zbm_config::conf::{self, Conf};
use zbm_config::spec::cmdline::luks::LUKS;
use zbm_config::spec::properties::KERNEL;
use zbm_luks::config::{CRYPTTAB, DEFAULT_TRIES, Options, load_config};
use zbm_luks::device::{mapping_name, resolve_device};
use zbm_platform::exec::Runner;
use zbm_platform::{Files, System};
use zbm_state::State;
use zbm_zfs::{Stderr, ZpoolCommand};

use super::{Comparison, Selected};
use crate::boot::zfs_value;
use crate::commandline::ETC_HOSTID;
use crate::hooks::{HOOKS, executable_hooks};
use crate::import::{ImportSettings, choose_bootfs, imported_pools};
use crate::kernels;
use crate::keys::{self, KeySource, key_file, keysource_property};
use zbm_pool::{HostId, Requirement};
use zbm_types::HookStage;

/// `/etc/zfsbootmenu.conf`, or an empty one when it cannot be read.
fn configuration(files: &dyn Files) -> Conf {
    files
        .read_to_string(Path::new(conf::PATH))
        .ok()
        .and_then(|text| Conf::parse(&text).ok())
        .unwrap_or_default()
}

/// The host ID in `/etc/hostid`: its first four bytes as a little-endian
/// number, in eight hex digits.
fn etc_hostid(files: &dyn Files) -> Option<String> {
    let bytes = files.read(Path::new(ETC_HOSTID)).ok()?;
    let mut word = [0_u8; 4];
    for (target, byte) in word.iter_mut().zip(&bytes) {
        *target = *byte;
    }
    Some(format!("{:08x}", u32::from_le_bytes(word)))
}

/// The host ID: what the start-up stage wrote from `spl_hostid`, or the host
/// ID that pool import matched.
pub fn host_id(files: &dyn Files, state: &State<'_>) -> Option<Comparison> {
    let rust = if let Some(matched) = state.spl_hostid() {
        HostId::from_hex(&matched)?.to_string()
    } else {
        let given = configuration(files)
            .get("spl_hostid")
            .unwrap_or_default()
            .to_owned();
        if given.is_empty() {
            return None;
        }
        HostId::from_hex(&given)
            .unwrap_or_else(HostId::default_id)
            .to_string()
    };
    Some(Comparison {
        decision: "host ID",
        legacy: etc_hostid(files).unwrap_or_else(|| "none".to_owned()),
        rust,
    })
}

/// `pool=readonly` for each pool, in the order given.
fn readonly_list(runner: &dyn Runner, pools: &[String]) -> String {
    pools
        .iter()
        .map(|pool| {
            let value = ZpoolCommand::new(runner)
                .text(
                    &["get", "-H", "-o", "value", "readonly", pool],
                    Stderr::Hidden,
                )
                .map(|output| output.stdout_text().trim().to_owned())
                .unwrap_or_default();
            format!("{pool}={value}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The pools imported and how: only the preferred pool when it is required
/// alone (`zbm.prefer=pool!!`), and every one read-only, as the start-up
/// program imports them.
pub fn pools(runner: &dyn Runner, files: &dyn Files) -> Option<Comparison> {
    let imported = imported_pools(runner);
    if imported.is_empty() {
        return None;
    }
    let settings = ImportSettings::from_conf(&configuration(files));
    let expected: Vec<String> = match &settings.prefer_pool {
        Some(pool) if settings.requirement == Requirement::Only => vec![pool.to_string()],
        _ => imported.clone(),
    };
    Some(Comparison {
        decision: "pools",
        legacy: readonly_list(runner, &imported),
        rust: expected
            .iter()
            .map(|pool| format!("{pool}=on"))
            .collect::<Vec<_>>()
            .join(" "),
    })
}

/// The mapping names of the LUKS volumes the legacy runtime opened: the
/// crypttab's name, or `luks-<UUID>` for volumes discovery found.
pub fn luks_mappings(system: &System<'_>, state: &State<'_>) -> Option<Comparison> {
    let text = system.files.read_to_string(&state.luks_mappings()).ok()?;
    let records: Vec<Vec<&str>> = text
        .lines()
        .map(|line| line.split('\t').collect())
        .filter(|fields: &Vec<&str>| fields.len() >= 3)
        .collect();
    if records.is_empty() {
        return None;
    }
    let crypttab = system.files.read_to_string(Path::new(CRYPTTAB)).ok();
    let zbm_luks = CmdlineDir::new(system.files, state.base()).lookup(&LUKS, &mut Vec::new());
    let (mode, _) = load_config(zbm_luks.as_deref(), crypttab.as_deref(), CRYPTTAB);
    // As the unlock use cases name them: the crypttab entry's name, or the
    // default name of its header (`header=`, or the device).
    let expected = |device: &str, auto: &str| {
        let configured = (auto != "1")
            .then(|| {
                mode.entries()
                    .iter()
                    .find(|entry| resolve_device(system, &entry.spec).as_deref() == Some(device))
            })
            .flatten();
        configured.map_or_else(
            || mapping_name(system, "", device),
            |entry| {
                let (options, _) = Options::parse(&entry.options, DEFAULT_TRIES);
                let header = options.header.unwrap_or_else(|| device.to_owned());
                mapping_name(system, &entry.name, &header)
            },
        )
    };
    let mut legacy = Vec::new();
    let mut rust = Vec::new();
    for record in &records {
        let (name, device, auto) = match record.as_slice() {
            [name, device, auto, ..] => (*name, *device, *auto),
            _ => continue,
        };
        legacy.push(format!("{device}={name}"));
        rust.push(format!("{device}={}", expected(device, auto)));
    }
    Some(Comparison {
        decision: "LUKS mappings",
        legacy: legacy.join(" "),
        rust: rust.join(" "),
    })
}

/// The default boot environment.
pub fn boot_environment(
    runner: &dyn Runner,
    files: &dyn Files,
    state: &State<'_>,
) -> Option<Comparison> {
    let legacy = state.bootfs()?;
    let settings = ImportSettings::from_conf(&configuration(files));
    Some(Comparison {
        decision: "default boot environment",
        legacy,
        rust: choose_bootfs(runner, &settings)
            .map_or_else(|| "none".to_owned(), |dataset| dataset.to_string()),
    })
}

/// The kernel and initramfs of the selected boot environment, as found in
/// it and chosen by `org.zfsbootmenu:kernel`. A kernel the user picked by
/// hand in the menu shows as a difference.
pub fn kernel(system: &System<'_>, selected: &Selected) -> Option<Comparison> {
    if selected.be.is_empty() || selected.mountpoint.is_empty() || selected.kernel.is_empty() {
        return None;
    }
    let discovery = kernels::discover(system.files, &selected.be, Path::new(&selected.mountpoint));
    let property =
        zfs_value(system.runner, KERNEL.name, &selected.be).unwrap_or_else(|| "-".to_owned());
    let rust = kernels::select(system.runner, &discovery.kernels, &property).map_or_else(
        || "none".to_owned(),
        |kernel| format!("{} {}", kernel.kernel, kernel.initramfs),
    );
    Some(Comparison {
        decision: "kernel",
        legacy: format!("{} {}", selected.kernel, selected.initramfs),
        rust,
    })
}

/// Where the legacy runtime took the key of `root` for `fs`, from what it
/// left: it makes `/zfsbootmenu/.keys/<source>` only for a key source it
/// accepted, before copying the key file there.
fn legacy_key_source(system: &System<'_>, state: &State<'_>, root: &str, fs: &str) -> KeySource {
    let keylocation =
        zfs_value(system.runner, "keylocation", root).unwrap_or_else(|| "prompt".into());
    if keylocation == "prompt" {
        return KeySource::Prompt;
    }
    if let Some(file) = key_file(&keylocation) {
        let path = format!("/{file}");
        if system.files.exists(Path::new(&path)) {
            return KeySource::File(path);
        }
        let cached = keysource_property(system.runner, fs).filter(|source| {
            system
                .files
                .exists(&state.base().join(".keys").join(source))
        });
        if let Some(source) = cached {
            return KeySource::Keysource(source);
        }
    }
    if zfs_value(system.runner, "keyformat", root).as_deref() == Some("passphrase") {
        KeySource::Prompt
    } else {
        KeySource::Nothing
    }
}

/// Where the key of the selected boot environment's encryption root came
/// from. Left out for an unencrypted boot environment, and when `load-key.d`
/// hooks may have unlocked it in their own way.
pub fn key_source(
    system: &System<'_>,
    state: &State<'_>,
    selected: &Selected,
) -> Option<Comparison> {
    if selected.be.is_empty()
        || !executable_hooks(system.files, Path::new(HOOKS), HookStage::LoadKey).is_empty()
    {
        return None;
    }
    let (root, rust) = keys::key_source(system.runner, system.files, &selected.be)?;
    Some(Comparison {
        decision: "key source",
        legacy: legacy_key_source(system, state, &root, &selected.be).to_string(),
        rust: rust.to_string(),
    })
}
