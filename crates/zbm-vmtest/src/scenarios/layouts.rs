//! Main vdev layouts on LUKS, created with ZFS 2.4.4.
//!
//! - `main`: single disk, 2-way mirror, 3-way mirror, striped mirrors,
//!   RAIDZ1 (13 disks; root on the 2-way mirror);
//! - `parity`: RAIDZ2, RAIDZ3 with all default features, dRAID
//!   `draid2:3d:1s` (15 disks; root on dRAID).
//!
//! Pool names are the layout names prefixed with "t" (`tsingle`,
//! `tmirror2`, ...): ZFS reserves names that start with mirror, raidz,
//! draid or spare.
//!
//! Each run creates the pools in ZFSBootMenu's recovery shell
//! (`zbm.luks=off`), then boots again with the default automatic unlocking
//! and checks that one passphrase opens every volume, that every pool is
//! imported with its layout, that the teardown closes everything, and that
//! the ZFS 2.4.4 boot environment unlocks, imports, writes to and scrubs
//! every pool. In the parity group, `traidz2` also gets the read-only
//! compatible features `fast_dedup`, `block_cloning_endian` and
//! `physical_rewrite` activated.
//!
//! `<group>-oldzfs` boots the pools of a group with an image that has an
//! older ZFS (`ZBMTEST_OLD_KERNEL`, `ZBMTEST_OLD_INITRD`; skipped when not
//! set): pools whose new features are only enabled import normally,
//! `traidz2` imports read-only with a warning, and the ZFS 2.4.4 boot
//! environment still boots.

use crate::boot::{Boot, Follow, check_be, check_teardown, follow_boot, pool_block};
use crate::error::{Error, Result};
use crate::expect::regex;
use crate::images::{ImageSpec, hostid_arg, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::{FAST_PBKDF, PW};
use crate::vm::{Sh, Vm};

use super::{disks, payload};

/// The options of the pools.
const OPTS: &str = "-o ashift=12 -o autotrim=on -o compatibility=openzfs-2.4-linux \
                    -O compression=lz4 -O acltype=posixacl -O xattr=sa -O relatime=on -m none";

/// A pool of a group: its name, how it is created (`OPTS` is filled in) and
/// the vdevs its status shows.
struct Pool {
    name: &'static str,
    create: &'static str,
    vdevs: &'static [&'static str],
}

/// A group of layouts.
struct Group {
    disks: usize,
    root: &'static str,
    pools: &'static [Pool],
}

const MAIN: Group = Group {
    disks: 13,
    root: "tmirror2",
    pools: &[
        Pool {
            name: "tsingle",
            create: "zpool create -f OPTS tsingle ${V[0]}",
            vdevs: &["tsingle "],
        },
        Pool {
            name: "tmirror2",
            create: "zpool create -f OPTS tmirror2 mirror ${V[1]} ${V[2]}",
            vdevs: &["mirror-0"],
        },
        Pool {
            name: "tmirror3",
            create: "zpool create -f OPTS tmirror3 mirror ${V[3]} ${V[4]} ${V[5]}",
            vdevs: &["mirror-0"],
        },
        Pool {
            name: "traid10",
            create: "zpool create -f OPTS traid10 mirror ${V[6]} ${V[7]} mirror ${V[8]} ${V[9]}",
            vdevs: &["mirror-0", "mirror-1"],
        },
        Pool {
            name: "traidz1",
            create: "zpool create -f OPTS traidz1 raidz1 ${V[10]} ${V[11]} ${V[12]}",
            vdevs: &["raidz1-0"],
        },
    ],
};

const PARITY: Group = Group {
    disks: 15,
    root: "tdraid",
    pools: &[
        Pool {
            name: "traidz2",
            create: "zpool create -f OPTS traidz2 raidz2 ${V[@]:0:4}",
            vdevs: &["raidz2-0"],
        },
        // No compatibility property: every feature ZFS 2.4.4 enables by
        // default.
        Pool {
            name: "traidz3",
            create: "zpool create -f -o ashift=12 -O compression=zstd -O checksum=blake3 -m none \
                     traidz3 raidz3 ${V[@]:4:5}",
            vdevs: &["raidz3-0"],
        },
        Pool {
            name: "tdraid",
            create: "zpool create -f OPTS tdraid draid2:3d:1s ${V[@]:9:6}",
            vdevs: &["draid2:3d:6c:1s-0", "draid2-0-0"],
        },
    ],
};

/// Runs `main`, `parity` or `<group>-oldzfs`.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub fn run(test: &Test, scenario: &str) -> Result<()> {
    if let Some(name) = scenario.strip_suffix("-oldzfs") {
        return oldzfs(test, name);
    }
    let group = group(scenario)?;
    setup(test, scenario, group)?;
    verify(test, scenario, group)
}

fn group(name: &str) -> Result<&'static Group> {
    match name {
        "main" => Ok(&MAIN),
        "parity" => Ok(&PARITY),
        _ => Err(Error::Setup(format!("unknown layout group {name}"))),
    }
}

/// Creates the pools in the recovery shell.
fn setup(test: &Test, name: &str, group: &Group) -> Result<()> {
    let settings = test.settings();
    let mut machine_disks = disks(settings, name, 0..group.disks, true)?;
    machine_disks.push(payload(settings)?);
    let image = zbm_image(settings, &ImageSpec::new("auto"))?;
    let machine = Machine::boot(&image, "console=ttyS0 zbm.luks=off zbm.retry_delay=3")
        .with_disks(machine_disks);
    let mut vm = Vm::start(test, "setup", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    let (_, out) = vm.sh_show("zfs version")?;
    test.check(
        out.contains("zfs-2.4.4") && out.contains("zfs-kmod-2.4.4"),
        "ZFSBootMenu runs ZFS 2.4.4 (userland and module)",
    );
    let devices: Vec<String> = (0..group.disks)
        .map(|index| format!("/dev/disk/by-id/virtio-d{index}"))
        .collect();
    let mkluks = format!(
        "V=(); U=(); mkluks {PW} '{FAST_PBKDF}' {}",
        devices.join(" ")
    );
    vm.must_with(&mkluks, Sh::default().timeout(3600))?;
    for pool in group.pools {
        let (rc, _) = vm.sh(&pool.create.replace("OPTS", OPTS))?;
        test.check(rc == 0, &format!("zpool create {}", pool.name));
        let name = pool.name;
        vm.must(&format!(
            "zfs create -o mountpoint=/data/{name} {name}/data && \
             head -c 8388608 /dev/urandom > /data/{name}/blob && zpool sync {name}"
        ))?;
    }
    if name == "parity" {
        parity_features(test, &mut vm)?;
    }
    let root = group.root;
    vm.must(&format!("mkbe {root}"))?;
    vm.must(&format!("becmdline {root}/ROOT/be \"${{U[@]}}\""))?;
    vm.sh_show("zpool status -P")?;
    for pool in group.pools {
        vm.sh_show(&format!("features {}", pool.name))?;
    }
    vm.must("finish")?;
    vm.poweroff()
}

/// Activates the read-only compatible features of `traidz2` (see
/// `docs/luks/zfs-versions.rst`). Deduplicated blocks are cloned through
/// the dedup table, so block cloning needs a dataset without dedup.
fn parity_features(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    vm.must(
        "zfs create -o mountpoint=/data/dd -o dedup=on traidz2/dd && \
         head -c 4194304 /dev/urandom > /data/dd/a && cp /data/dd/a /data/dd/b && \
         zfs rewrite -P /data/dd/a && zfs create -o mountpoint=/data/bc traidz2/bc && \
         head -c 4194304 /dev/urandom > /data/bc/a && \
         cp --reflink=always /data/bc/a /data/bc/clone && zpool sync traidz2",
    )?;
    let (_, out) = vm.sh_show("features traidz2")?;
    let active: Vec<&str> = out.split_whitespace().collect();
    for feature in [
        "fast_dedup",
        "block_cloning",
        "block_cloning_endian",
        "physical_rewrite",
    ] {
        test.check(
            active.contains(&feature),
            &format!("traidz2: {feature} active"),
        );
    }
    Ok(())
}

/// Boots with automatic unlocking and checks every pool.
fn verify(test: &Test, name: &str, group: &Group) -> Result<()> {
    let settings = test.settings();
    let image = zbm_image(settings, &ImageSpec::new("auto"))?;
    let machine = Machine::boot(&image, "console=ttyS0 zbm.timeout=3").with_disks(disks(
        settings,
        name,
        0..group.disks,
        false,
    )?);
    let mut vm = Vm::start(test, "boot", &machine)?;
    let boot = follow_boot(&mut vm, Follow::default())?;
    test.check(
        boot.prompts == ["luks"],
        &format!(
            "one passphrase prompt unlocked all {} volumes (prompts: {})",
            group.disks,
            boot.prompts_text()
        ),
    );
    check_pools(test, &boot, group)?;
    check_teardown(test, &boot, &[]);
    let names: Vec<&str> = group.pools.iter().map(|pool| pool.name).collect();
    check_be(test, &boot, group.root, &names, &[]);
    check_features(test, &boot, name, group);
    Ok(())
}

/// The pools in ZFSBootMenu's status: imported, with their vdevs, on LUKS.
fn check_pools(test: &Test, boot: &Boot, group: &Group) -> Result<()> {
    let pools = boot.pools();
    let mapping = regex(r"/dev/mapper/luks-\S+\s+(ONLINE|AVAIL)")?;
    for pool in group.pools {
        let state = pools.show(pool.name);
        test.check(
            pools.get(pool.name) == Some("ONLINE"),
            &format!("ZFSBootMenu imported {}: {state}", pool.name),
        );
        let text = pool_block(&boot.zbm_status, pool.name);
        for vdev in pool.vdevs {
            test.check(
                text.contains(vdev),
                &format!("{} has vdev {}", pool.name, vdev.trim()),
            );
        }
        test.check(
            mapping.is_match(&text),
            &format!("{} vdevs are LUKS mappings", pool.name),
        );
    }
    Ok(())
}

/// The features the boot environment reports.
fn check_features(test: &Test, boot: &Boot, name: &str, group: &Group) {
    let features = boot.be_features();
    let feature = |pool: &str, name: &str| {
        features
            .get(&(pool.to_owned(), name.to_owned()))
            .map(String::as_str)
    };
    for pool in group.pools {
        // zpool create enables it, with or without the openzfs-2.4-linux
        // set; zpool upgrade does not (see procedures).
        let state = feature(pool.name, "dynamic_gang_header");
        test.check(
            matches!(state, Some("enabled" | "active")),
            &format!(
                "{}: dynamic_gang_header {} after zpool create",
                pool.name,
                crate::pyfmt::option(state)
            ),
        );
    }
    if name == "parity" {
        test.check(
            feature("tdraid", "draid") == Some("active"),
            "draid feature active",
        );
        test.check(
            feature("traidz3", "zstd_compress") == Some("active"),
            "zstd_compress active on raidz3",
        );
        test.check(
            feature("traidz3", "blake3") == Some("active"),
            "blake3 active on raidz3",
        );
    }
}

/// Boots the pools of a group with an image with an older ZFS.
fn oldzfs(test: &Test, name: &str) -> Result<()> {
    let settings = test.settings();
    let Some((kernel, initrd)) = &settings.old_image else {
        test.skip("ZBMTEST_OLD_KERNEL and ZBMTEST_OLD_INITRD are not set");
        return Ok(());
    };
    let group = group(name)?;
    let append = format!("console=ttyS0 zbm.timeout=3 {}", hostid_arg(settings)?);
    let machine = Machine::kernel(kernel, initrd, &append).with_disks(disks(
        settings,
        name,
        0..group.disks,
        false,
    )?);
    let mut vm = Vm::start(test, "oldzfs", &machine)?;
    let boot = follow_boot(&mut vm, Follow::default())?;
    if boot.has_prompt("emergency") {
        // Records why nothing could be booted.
        vm.shell_ready()?;
        for command in [
            "zfs version",
            "zpool list -H -o name,health,readonly",
            "zpool get -H -o name,value bootfs",
            "cat /zfsbootmenu/degraded",
            "zpool import",
            "zfs list -o name,mountpoint,canmount,mounted",
            "zpool get -H -o name,property,value all | grep -E 'unsupported@|feature@.*active'",
        ] {
            vm.sh_show(command)?;
        }
        vm.kill();
    }
    let log = std::fs::read(vm.serial_log()).unwrap_or_default();
    let log = String::from_utf8_lossy(&log);
    test.check(
        log.contains("nsupported features"),
        "the older ZFS warns about unsupported features",
    );
    let pools = boot.pools();
    for pool in group.pools {
        test.check(
            pools.get(pool.name) == Some("ONLINE"),
            &format!(
                "older ZFS imported {}: {}",
                pool.name,
                pools.show(pool.name)
            ),
        );
    }
    check_teardown(test, &boot, &[]);
    let names: Vec<&str> = group.pools.iter().map(|pool| pool.name).collect();
    check_be(test, &boot, group.root, &names, &[]);
    Ok(())
}
