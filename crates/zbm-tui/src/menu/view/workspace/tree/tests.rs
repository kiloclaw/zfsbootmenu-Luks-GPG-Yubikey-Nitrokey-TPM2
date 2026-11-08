//! Tests of the Pools tab as the workspace draws it: the inspector of a
//! vdev, a device and a volume, the states of volumes and devices, a volume
//! whose TPM2 key is out of date, the header of the volumes of no pool, the
//! matches of a filter, and an empty tab.

use zbm_types::{HostId, Untrusted};

use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};
use crate::menu::keymap::Map;
use crate::menu::model::{Model, Screen};
use crate::menu::theme::Theme;
use crate::menu::view::tests::screen;
use crate::menu::view::words;
use crate::menu::world::{PoolHost, VolumeState};

/// The demo in the workspace, on its Pools tab, the row `downs` below the
/// first selected, after `change` changed its machine.
fn tab(downs: usize, change: impl FnOnce(&mut crate::menu::world::Snapshot)) -> Model {
    let machine = FakeMachine::new(&DemoOptions::default());
    let mut snapshot = machine.snapshot().clone();
    change(&mut snapshot);
    let mut model = Model::new(snapshot, Theme::new(Capabilities::FULL));
    model.map = Map::Workspace;
    model.screen = Screen::Pools;
    model.refresh();
    model.pools.selected = downs;
    model
}

#[test]
fn the_inspector_of_a_vdev_says_its_kind_and_how_many_devices_it_holds() {
    let model = tab(8, |_| {});

    let shown = screen(&model, 120, 40);
    assert!(shown.contains("┌ raidz1-0 ─"), "{shown}");
    assert!(shown.contains("Kind  raidz1"), "{shown}");
    assert!(shown.contains("Devices  3"), "{shown}");
    assert!(shown.contains("Health  DEGRADED"), "{shown}");
    assert!(shown.contains(" tank › raidz1-0\n"), "{shown}");
}

#[test]
fn the_inspector_of_a_device_says_its_path_its_note_and_its_errors() {
    let model = tab(11, |_| {});

    let shown = screen(&model, 80, 24);
    assert!(shown.contains("Path  /dev/sdc2"), "{shown}");
    assert!(shown.contains("Note  too many errors"), "{shown}");
    assert!(
        shown.contains("Errors  0 read, 0 write, 14 checksum"),
        "{shown}"
    );
    assert!(shown.contains(" tank › raidz1-0 › sdc2\n"), "{shown}");
}

#[test]
fn volumes_and_devices_say_their_state_whatever_it_is() {
    let model = tab(5, |snapshot| {
        snapshot.volumes[0].state = VolumeState::Locked;
        snapshot.volumes[1].state = VolumeState::Skipped;
        snapshot.volumes[2].state = VolumeState::Failed("no \u{1b}key".to_owned());
        snapshot.pools[1].devices[0].children[0].state = None;
        snapshot.pools[0].host = Some(PoolHost {
            hostid: None,
            hostname: Some(Untrusted::new("elsewhere".to_owned())),
        });
        snapshot.pools[1].host = Some(PoolHost {
            hostid: "deadbeef".parse::<HostId>().ok(),
            hostname: None,
        });
    });

    let shown = screen(&model, 120, 40);
    assert!(shown.contains("Volume · locked"), "{shown}");
    assert!(shown.contains("Volume · skipped"), "{shown}");
    assert!(shown.contains("Volume · failed: no \\x1bkey"), "{shown}");
    assert!(shown.contains("sda2") && shown.contains("none"), "{shown}");
    assert!(shown.contains("host elsewhere · read-only"), "{shown}");
    assert!(shown.contains("host deadbeef · read-only"), "{shown}");
    // The inspector of the volume selected.
    assert!(shown.contains("State  locked"), "{shown}");
    assert!(shown.contains("Device  nvme0n1p2"), "{shown}");
    assert!(shown.contains("Pool  zroot"), "{shown}");
}

#[test]
fn a_volume_whose_tpm2_key_is_out_of_date_says_so_in_its_row() {
    let changed = FakeMachine::new(&DemoOptions {
        tpm_changed: true,
        ..DemoOptions::default()
    });
    let seal = changed.snapshot().volumes[0].tpm2.clone();
    let model = tab(5, |snapshot| snapshot.volumes[0].tpm2 = seal);

    let shown = screen(&model, 120, 40);
    assert!(shown.contains("Volume · TPM2 changed · open"), "{shown}");
    assert!(shown.contains(" zroot › luks-zroot-a\n"), "{shown}");
    assert_eq!(shown.matches("TPM2 changed").count(), 1, "{shown}");
}

#[test]
fn the_volumes_of_no_pool_show_under_their_header() {
    let model = tab(10, |snapshot| {
        for volume in &mut snapshot.volumes {
            volume.pool = None;
        }
    });

    let shown = screen(&model, 80, 25);
    assert!(shown.contains("│   No pool"), "{shown}");
    assert!(shown.contains(" No pool › luks-zroot-a\n"), "{shown}");
    assert!(!shown.contains("Pool  "), "{shown}");
}

#[test]
fn a_filter_shows_its_matches_alone_with_their_depth() {
    let mut model = tab(0, |_| {});
    model.pools.filter = "sd".to_owned();
    model.refresh();

    let shown = screen(&model, 80, 25);
    assert!(shown.contains("3 of 13"), "{shown}");
    assert!(shown.contains("▶     sda2"), "{shown}");
    assert!(!shown.contains("zroot  "), "{shown}");
    let (rows, _) = words::rows(&model);
    assert_eq!(rows.len(), 3);
    assert!(rows[0].starts_with("    sda2"), "{rows:?}");
}

#[test]
fn a_tab_without_pools_or_volumes_says_so_and_inspects_nothing() {
    let model = tab(0, |snapshot| {
        snapshot.pools.clear();
        snapshot.volumes.clear();
    });

    for (width, height) in [(120, 40), (52, 20)] {
        let shown = screen(&model, width, height);
        assert!(shown.contains("No Pools"), "{shown}");
        assert!(shown.contains("No Selection"), "{shown}");
    }
}

#[test]
fn the_tree_draws_in_ascii() {
    let mut model = tab(10, |_| {});
    model.theme = Theme::new(Capabilities::ASCII);

    let shown = screen(&model, 80, 24);
    assert!(shown.is_ascii(), "{shown}");
    assert!(shown.contains("> tank > raidz1-0 > sdb2") || shown.contains("tank > raidz1-0"));
    assert!(shown.contains("cannot open | UNAVAIL"), "{shown}");
}

#[test]
fn a_long_note_or_host_is_cut_before_the_name_or_the_state_is() {
    let long = "WDC_WD40EFRX-68N32N0_WD-WCC7K1234567-part1-and-more-of-it";
    let model = tab(10, |snapshot| {
        let sdb2 = &mut snapshot.pools[1].devices[0].children[1];
        sdb2.name = Untrusted::new("1234567890123456789".to_owned());
        sdb2.note = Some(Untrusted::new(format!("was /dev/disk/by-id/ata-{long}")));
        snapshot.pools[1].host = Some(PoolHost {
            hostid: None,
            hostname: Some(Untrusted::new(long.repeat(2))),
        });
    });

    for (width, height) in [(120, 40), (80, 24), (52, 20)] {
        let shown = screen(&model, width, height);
        let row = |state: &str| {
            let line = shown.lines().find(|line| line.contains(state));
            line.unwrap_or_else(|| panic!("no {state} row: {shown}"))
        };
        let device = row("· UNAVAIL │");
        assert!(device.contains("▶     ata-WDC"), "{device}");
        assert!(device.contains("was /dev"), "{device}");
        let pool = row("· DEGRADED │");
        assert!(pool.contains("│   tank "), "{pool}");
        assert!(pool.contains("host WDC"), "{pool}");
    }
}

#[test]
fn the_inspector_names_the_last_host_once() {
    let host = |hostname: Option<&str>, hostid: Option<&str>| PoolHost {
        hostid: hostid.and_then(|hostid| hostid.parse::<HostId>().ok()),
        hostname: hostname.map(|name| Untrusted::new(name.to_owned())),
    };
    let cases = [
        (
            host(Some("backup"), Some("deadbeef")),
            "Last Host  backup (deadbeef)",
        ),
        (host(Some("backup"), None), "Last Host  backup\n"),
        (host(None, Some("deadbeef")), "Last Host  deadbeef"),
        (host(None, None), "Last Host  unknown"),
    ];
    for (last, said) in cases {
        let model = tab(7, |snapshot| snapshot.pools[1].host = Some(last));

        let shown = screen(&model, 80, 25);
        let said = said.trim_end();
        let line = shown.lines().find(|line| line.contains("Last Host"));
        let line = line.unwrap_or_else(|| panic!("no host: {shown}"));
        assert!(line.contains(said), "{line}");
        assert!(!line.contains("host "), "{line}");
    }
}
