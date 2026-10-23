//! `complex-setup`: the disks, pools, datasets and boot environment, made
//! in the recovery shell of a first boot (`zbm.luks=off`).

use std::fs;

use crate::disks::partition;
use crate::error::{Error, Result};
use crate::images::{ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::{FAST_PBKDF, PW, PW_OTHER, ZFSKEY};
use crate::vm::{Sh, Vm};

use super::super::{disks, payload};
use super::{GROUP, N, NEW_FEATURES, OPTS, crypttab, disk};

/// Creates everything in the recovery shell.
pub(super) fn setup(test: &Test) -> Result<()> {
    let settings = test.settings();
    let mut machine_disks = disks(settings, GROUP, 0..N, true)?;
    partition(
        &disk(settings, 15)?,
        &[("zfsA", Some("180M")), ("plainB", None)],
    )?;
    let image = zbm_image(settings, &ImageSpec::new("auto"))?;
    machine_disks.push(payload(settings)?);
    let machine = Machine::boot(&image, "console=ttyS0 zbm.luks=off zbm.retry_delay=3")
        .with_disks(machine_disks);
    let mut vm = Vm::start(test, "setup", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    create_pools(test, &mut vm)?;
    if !new_features(test, &mut vm)? {
        return Ok(());
    }
    boot_environment(test, &mut vm)?;
    vm.must("finish")?;
    vm.poweroff()
}

/// `/dev/disk/by-id/virtio-d<n>` for each disk.
fn by_id(indexes: impl IntoIterator<Item = usize>) -> String {
    let devices: Vec<String> = indexes
        .into_iter()
        .map(|index| format!("/dev/disk/by-id/virtio-d{index}"))
        .collect();
    devices.join(" ")
}

/// Formats the volumes and creates tank, mixed and parts.
fn create_pools(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let volumes = by_id((0..12).chain([14]));
    vm.must_with(
        &format!("V=(); U=(); mkluks {PW} '{FAST_PBKDF}' {volumes}"),
        Sh::default().timeout(3600),
    )?;
    vm.must(&format!("mkluks {PW_OTHER} '{FAST_PBKDF}' {}", by_id([12])))?;
    vm.must(&format!(
        "mkluks {PW} '{FAST_PBKDF}' /dev/disk/by-partlabel/zfsA"
    ))?;
    // V[0..11] = d0..d11, V[12] = d14, V[13] = d12, V[14] = zfsA
    let (rc, _) = vm.sh(&format!(
        "zpool create -f {OPTS} tank raidz2 ${{V[@]:0:4}} special mirror ${{V[4]}} ${{V[5]}} \
         dedup mirror ${{V[6]}} ${{V[7]}} log mirror ${{V[8]}} ${{V[9]}} cache ${{V[10]}} \
         spare ${{V[11]}}"
    ))?;
    test.check(
        rc == 0,
        "tank: RAIDZ2 + special, dedup and log mirrors + cache + spare",
    );
    let (rc, _) = vm.sh_with(
        "zpool attach tank raidz2-0 ${V[12]} && zpool wait -t raidz_expand tank",
        Sh::default().timeout(3600),
    )?;
    test.check(
        rc == 0,
        "RAIDZ expansion: zpool attach tank raidz2-0 <LUKS volume>",
    );
    let (rc, _) = vm.sh(&format!(
        "zpool create -f {OPTS} mixed mirror ${{V[13]}} /dev/disk/by-id/virtio-d13"
    ))?;
    test.check(rc == 0, "mixed: mirror of a LUKS volume and a plain disk");
    let (rc, _) = vm.sh(&format!(
        "zpool create -f {OPTS} parts mirror ${{V[14]}} /dev/disk/by-partlabel/plainB"
    ))?;
    test.check(
        rc == 0,
        "parts: mirror of a LUKS partition and a plain partition",
    );
    Ok(())
}

/// Activates the newer features; returns `false` when the test ended
/// because the gang blocks could not be written in time.
fn new_features(test: &Test, vm: &mut Vm<'_>) -> Result<bool> {
    vm.must(
        "zfs create -o mountpoint=/data/t -o dedup=on -o compression=zstd -o checksum=blake3 \
         -o dnodesize=auto -o longname=on tank/data",
    )?;
    vm.must(
        "head -c 8388608 /dev/urandom > /data/t/a && cp /data/t/a /data/t/dup && zpool sync tank",
    )?;
    // Deduplicated blocks are cloned through the dedup table: clone in a
    // dataset without dedup.
    let (rc, _) = vm.sh_show(
        "zfs create -o mountpoint=/data/bc tank/bc && \
         head -c 4194304 /dev/urandom > /data/bc/a && \
         cp --reflink=always /data/bc/a /data/bc/clone && zpool sync tank && \
         zpool get -H -o value bcloneused tank",
    )?;
    test.check(rc == 0, "cp --reflink=always clones blocks (block cloning)");
    vm.must(r#"n=; for i in {1..300}; do n=${n}n; done; : > "/data/t/${n}" && zpool sync tank"#)?;
    let (rc, _) = vm.sh("zfs rewrite -P /data/t/a && zpool sync tank")?;
    test.check(
        rc == 0,
        "zfs rewrite -P (physical rewrite, new in OpenZFS 2.4)",
    );
    if !gang_blocks(test, vm)? {
        return Ok(false);
    }
    vm.must("zfs snapshot tank/data@s1 && zfs bookmark tank/data@s1 tank/data#b1")?;
    vm.must(
        "zfs create -o mountpoint=/data/m mixed/data && \
         zfs create -o mountpoint=/data/p parts/data && \
         head -c 4194304 /dev/urandom > /data/m/f && head -c 4194304 /dev/urandom > /data/p/f && \
         zpool sync mixed parts",
    )?;
    Ok(true)
}

/// Writes forced gang blocks; returns `false` when they could not be
/// written in time, which ends the test.
///
/// ZFS 2.4 activates `dynamic_gang_header` when the children of a gang
/// block do not fit (`zio_write_gang_block`). Forcing every block of 32 KiB
/// or more to gang makes 128 KiB records gang and their ~43 KiB children
/// gang again; their ~15 KiB grandchildren are allocated normally. The
/// dataset has neither dedup nor compression.
fn gang_blocks(test: &Test, vm: &mut Vm<'_>) -> Result<bool> {
    let written = vm.sh_with(
        "zfs create -o mountpoint=/data/g -o dedup=off -o compression=off -o recordsize=128k \
         tank/gang && P=/sys/module/zfs/parameters; echo 32769 > $P/metaslab_force_ganging; \
         echo 100 > $P/metaslab_force_ganging_pct; \
         head -c 1048576 /dev/urandom > /data/g/gang; zpool sync tank; \
         echo 16777217 > $P/metaslab_force_ganging; zpool sync tank",
        Sh::default().timeout(900),
    );
    match written {
        Ok((rc, _)) => {
            test.check(rc == 0, "forced gang blocks written");
            Ok(true)
        }
        Err(Error::Timeout { .. }) => {
            test.check(
                false,
                "writing forced gang blocks did not finish within 15 minutes",
            );
            vm.kill();
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// The boot environment, an encrypted root on tank, its command line, and
/// the crypttab of the crypttab scenario.
fn boot_environment(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    vm.must(&format!(
        "printf '{ZFSKEY}\\n{ZFSKEY}\\n' | mkbe tank -o encryption=on -o keyformat=passphrase \
         -o keylocation=prompt"
    ))?;
    let (_, out) = vm.sh_show("zpool sync tank && features tank")?;
    let active: Vec<&str> = out.split_whitespace().collect();
    for feature in NEW_FEATURES {
        test.check(
            active.contains(&feature),
            &format!("tank: feature {feature} active"),
        );
    }
    vm.must(
        r#"becmdline tank/ROOT/be "${U[@]:0:13}" "${U[13]}=/etc/zbmtest/be-other.key" "${U[14]}""#,
    )?;
    vm.sh_show("zpool status -P")?;
    // The crypttab of the crypttab scenario: the names the pool was created
    // with and a mix of device specifications.
    let (_, out) = vm.sh(concat!(
        r#"for i in {0..11} 14; do j=$i; [ $i -eq 14 ] && j=12; "#,
        r#"case $i in 0|4) s="UUID=${U[$j]}";; *) s="ID=virtio-d$i";; esac; "#,
        r#"echo "luks-${U[$j]} $s none discard"; done; "#,
        r#"echo "luks-${U[13]} UUID=${U[13]^^} none"; echo "luks-${U[14]} PARTLABEL=zfsA none""#,
    ))?;
    let lines = crypttab_lines(&out);
    test.check(
        lines.len() == 15,
        &format!(
            "crypttab with {} entries for the crypttab scenario",
            lines.len()
        ),
    );
    let path = crypttab(test.settings());
    fs::write(&path, crypttab_text(&lines))
        .map_err(Error::io(format!("writing {}", path.display())))
}

/// The crypttab entries in the output of the command that prints them.
pub(super) fn crypttab_lines(out: &str) -> Vec<&str> {
    out.lines()
        .filter(|line| line.starts_with("luks-"))
        .collect()
}

/// The crypttab of `lines`.
pub(super) fn crypttab_text(lines: &[&str]) -> String {
    lines.join("\n") + "\n"
}
