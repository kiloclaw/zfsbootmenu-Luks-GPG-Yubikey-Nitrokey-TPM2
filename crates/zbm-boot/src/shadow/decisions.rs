//! The decisions shadow mode compares, one function each. Each reads what
//! the legacy runtime did and works out what the Rust code would have done;
//! `None` when there is nothing to compare.

use std::path::Path;

use zbm_cmdline::conf::{self, Conf};
use zbm_core::exec::{Command, Runner, Stream};
use zbm_luks::config::{CRYPTTAB, load_config};
use zbm_luks::device::{default_name, resolve_device};
use zbm_state::State;
use zbm_system::{Files, System};

use super::{Comparison, Selected};
use crate::boot::zfs_value;
use crate::commandline::ETC_HOSTID;
use crate::hostid::{DEFAULT_HOSTID, normalize};
use crate::import::{Settings, choose_bootfs, imported_pools};
use crate::kernels;

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
        normalize(&matched)?
    } else {
        let given = configuration(files)
            .get("spl_hostid")
            .unwrap_or_default()
            .to_owned();
        if given.is_empty() {
            return None;
        }
        normalize(&given).unwrap_or_else(|| DEFAULT_HOSTID.to_owned())
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
            let command = Command::new("zpool")
                .args(["get", "-H", "-o", "value", "readonly", pool])
                .stderr(Stream::Null);
            let value = runner
                .run(&command)
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
    let settings = Settings::from_conf(&configuration(files));
    let expected: Vec<String> =
        if settings.require_pool == "only" && !settings.prefer_pool.is_empty() {
            vec![settings.prefer_pool.clone()]
        } else {
            imported.clone()
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
    let zbm_luks = state.zbm_arg("zbm.luks");
    let (mode, _) = load_config(zbm_luks.as_deref(), crypttab.as_deref(), CRYPTTAB);
    let expected = |device: &str, auto: &str| {
        let configured = (auto != "1")
            .then(|| {
                mode.entries()
                    .iter()
                    .find(|entry| resolve_device(system, &entry.spec).as_deref() == Some(device))
            })
            .flatten()
            .filter(|entry| !entry.name.is_empty());
        configured.map_or_else(|| default_name(system, device), |entry| entry.name.clone())
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
    let settings = Settings::from_conf(&configuration(files));
    Some(Comparison {
        decision: "default boot environment",
        legacy,
        rust: choose_bootfs(runner, &settings).unwrap_or_else(|| "none".to_owned()),
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
    let property = zfs_value(system.runner, "org.zfsbootmenu:kernel", &selected.be)
        .unwrap_or_else(|| "-".to_owned());
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
