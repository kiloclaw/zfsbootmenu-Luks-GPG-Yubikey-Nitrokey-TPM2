//! `zbm debug luks` and `zbm debug pools`: the machine as `zbm-app` reads
//! it ([`zbm_app::App::snapshot`]).
//!
//! - `luks` prints each LUKS volume: its device, the mapping ZFSBootMenu
//!   opened it as, whether it is open, the key methods its tokens offer, and
//!   the pool on it.
//! - `pools` prints each imported pool (health, read-only or read/write, a
//!   checkpoint) and each boot environment (the pool's default, its
//!   encryption, and its command line with where it was set).

use std::path::Path;

use zbm_app::App;
use zbm_app::Encryption;
use zbm_app::model::{Origin, Snapshot, VolumeState};
use zbm_luks::unlocker::{Luks, Settings};
use zbm_platform::interaction::NoInteraction;
use zbm_platform::{Context, System};

use crate::Io;

/// The lines of `zbm debug luks`.
pub fn luks_lines(snapshot: &Snapshot) -> Vec<String> {
    if snapshot.volumes.is_empty() {
        return vec!["no LUKS volume".to_owned()];
    }
    snapshot
        .volumes
        .iter()
        .map(|volume| {
            let state = match &volume.state {
                VolumeState::Open => "open".to_owned(),
                VolumeState::Locked => "locked".to_owned(),
                VolumeState::Skipped => "skipped".to_owned(),
                VolumeState::Failed(why) => format!("failed: {why}"),
            };
            let methods: Vec<&str> = volume.methods.iter().map(|m| m.as_str()).collect();
            let mut line = format!(
                "{}\t{}\t{state}\tmethods: {}",
                volume.device,
                volume.name,
                methods.join(", ")
            );
            if let Some(pool) = &volume.pool {
                line.push_str("\tpool: ");
                line.push_str(pool.as_str());
            }
            line
        })
        .collect()
}

/// Where a command line was set, for people.
fn origin(origin: &Origin) -> String {
    match origin {
        Origin::Local => "set here".to_owned(),
        Origin::Inherited(from) => format!("from {from}"),
        Origin::Default => "default".to_owned(),
    }
}

/// The lines of `zbm debug pools`.
pub fn pool_lines(snapshot: &Snapshot) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    if snapshot.pools.is_empty() {
        lines.push("no pool is imported".to_owned());
    }
    for pool in &snapshot.pools {
        let mode = if pool.writable {
            "read/write"
        } else {
            "read-only"
        };
        let checkpoint = if pool.checkpoint { ", checkpoint" } else { "" };
        lines.push(format!(
            "{}\t{}\t{mode}{checkpoint}",
            pool.name,
            pool.health.as_str()
        ));
    }
    for environment in &snapshot.environments {
        let default = if environment.is_default {
            " (default)"
        } else {
            ""
        };
        let encryption = match environment.encryption {
            Encryption::None => "not encrypted",
            Encryption::Locked => "locked",
            Encryption::Unlocked => "unlocked",
        };
        let line = &environment.command_line;
        lines.push(format!(
            "  {}{default}\t{encryption}\tcommand line: {} ({})",
            environment.name,
            if line.value.is_empty() {
                "-"
            } else {
                &line.value
            },
            origin(&line.origin)
        ));
    }
    lines
}

/// `zbm debug luks` or `zbm debug pools` on `system`, with the LUKS
/// `settings`.
pub fn run_in(system: &System<'_>, settings: &Settings, report: &str, io: &mut Io) -> i32 {
    let luks = Luks {
        context: Context::new(*system),
        settings,
        interaction: &NoInteraction,
    };
    let app = App::new(Context::new(*system))
        .with_luks(luks)
        .with_base(&settings.base);
    let snapshot = match app.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm debug: {error}");
            return 1;
        }
    };
    let lines = if report == "luks" {
        luks_lines(&snapshot)
    } else {
        pool_lines(&snapshot)
    };
    for line in lines {
        let _ = writeln!(io.stdout, "{line}");
    }
    0
}

/// `zbm debug luks` or `zbm debug pools` on the machine, with the
/// configuration of `/etc/zfsbootmenu.conf`.
pub fn run(report: &str, io: &mut Io) -> i32 {
    let real = zbm_platform::RealSystem::new();
    let system = real.with_console(&zbm_platform::NoConsole);
    let conf = zbm_startup::preinit::read_conf(&system, Path::new("/")).unwrap_or_default();
    let settings = crate::luks::settings(&system, &conf);
    run_in(&system, &settings, report, io)
}

#[cfg(test)]
mod tests;
