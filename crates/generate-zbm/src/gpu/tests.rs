use std::collections::BTreeMap;

use zbm_testkit::TempDir;

use super::host::{Device, scan};
use super::tables::{CHIPS, names};
use super::*;

/// The files of linux-firmware in amdgpu/, i915/ and xe/, which stand for
/// what the drivers list.
fn linux_firmware() -> Vec<String> {
    include_str!("linux-firmware.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// What each driver lists, from linux-firmware: amdgpu its directory, i915
/// its own, and xe its own and the display firmware in i915/.
fn listed() -> BTreeMap<Driver, Vec<String>> {
    let files = linux_firmware();
    Driver::ALL
        .into_iter()
        .map(|driver| {
            let dirs = driver.firmware_dirs();
            let own = files
                .iter()
                .filter(|file| dirs.iter().any(|dir| file.starts_with(&format!("{dir}/"))))
                .cloned()
                .collect();
            (driver, own)
        })
        .collect()
}

fn words(text: &str) -> Vec<String> {
    vec![text.to_owned()]
}

#[test]
fn every_stem_of_the_table_names_files_of_linux_firmware() {
    let files = linux_firmware();
    for chip in CHIPS {
        for stem in chip.stems {
            assert!(
                files.iter().any(|file| has_stem(file, stem)),
                "{} {}: no file {stem}*",
                chip.driver.name(),
                chip.name
            );
        }
    }
}

#[test]
fn names_and_device_ids_are_not_shared() {
    for driver in Driver::ALL {
        let (families, chips) = names(driver);
        for chip in &chips {
            assert!(!families.contains(chip), "{chip} is also a family");
            assert_eq!(
                chips.iter().filter(|name| *name == chip).count(),
                1,
                "{chip}"
            );
        }
    }
    for chip in CHIPS {
        for device in chip.devices {
            let owners = CHIPS
                .iter()
                .filter(|other| other.driver.vendor() == chip.driver.vendor())
                .filter(|other| other.devices.contains(device))
                .count();
            assert_eq!(owners, 1, "{:04x} of {}", device, chip.name);
        }
    }
}

#[test]
fn stems_end_at_an_underscore_or_a_dot() {
    assert!(has_stem("amdgpu/navi10_me.bin", "amdgpu/navi10"));
    assert!(has_stem("amdgpu/smu_14_0_2.bin", "amdgpu/smu_14_0_2"));
    assert!(!has_stem("amdgpu/navi12_me.bin", "amdgpu/navi1"));
    assert!(!has_stem("amdgpu/psp_13_0_10_sos.bin", "amdgpu/psp_13_0_1"));
    assert!(!has_stem("amdgpu/navi10", "amdgpu/navi10"));
    assert!(has_version_stem(
        "amdgpu/dcn_3_5_dmcub.bin",
        "amdgpu/dcn_3_5"
    ));
    assert!(has_version_stem(
        "amdgpu/smu_14_0_2.bin",
        "amdgpu/smu_14_0_2"
    ));
    assert!(!has_version_stem(
        "amdgpu/dcn_3_5_1_dmcub.bin",
        "amdgpu/dcn_3_5"
    ));
    assert!(!has_version_stem(
        "amdgpu/smu_14_0_2.bin",
        "amdgpu/smu_14_0"
    ));
}

#[test]
fn the_option_names_drivers_alone_or_with_chips_and_families() {
    let requests = parse(&words("AMDGPU:gfx12,Navi10 i915 amdgpu:gfx12  amdgpu")).unwrap();

    assert_eq!(
        requests,
        [
            Request {
                driver: Driver::Amdgpu,
                host: true,
                names: vec!["gfx12".to_owned(), "navi10".to_owned()],
            },
            Request {
                driver: Driver::I915,
                host: true,
                names: Vec::new(),
            },
        ]
    );
    assert_eq!(
        parse(&["xe:xe2".to_owned(), "i915:tgl".to_owned()])
            .unwrap()
            .len(),
        2
    );
    assert_eq!(parse(&words("  ")).unwrap(), []);
}

#[test]
fn unknown_drivers_chips_and_families_are_refused() {
    let error = parse(&words("nouveau")).unwrap_err();
    assert!(
        error.contains("unknown graphics driver 'nouveau'"),
        "{error}"
    );

    let error = parse(&words("amdgpu:gfx13")).unwrap_err();
    assert!(
        error.contains("amdgpu has no chip or family 'gfx13'"),
        "{error}"
    );
    assert!(
        error.contains("gfx8, gfx9, gfx10, gfx10.3, gfx11, gfx11.5, gfx12"),
        "{error}"
    );

    let error = parse(&words("i915:lnl")).unwrap_err();
    assert!(
        error.contains("i915 has no chip or family 'lnl'"),
        "{error}"
    );
    assert!(parse(&words("amdgpu:")).is_err());
}

#[test]
fn a_family_takes_the_firmware_of_its_chips() {
    let requests = parse(&words("amdgpu:gfx12 i915:gen9")).unwrap();
    let choices = choose(&requests, &[]).unwrap();
    let selection = select(&choices, &listed()).unwrap();

    assert_eq!(selection.drivers, [Driver::Amdgpu, Driver::I915]);
    let firmware = &selection.firmware;
    assert!(
        firmware.contains(&"amdgpu/gc_12_0_0_mes.bin".to_owned()),
        "{firmware:?}"
    );
    assert!(
        firmware.contains(&"amdgpu/gc_12_0_1_mes.bin".to_owned()),
        "{firmware:?}"
    );
    assert!(
        firmware.contains(&"i915/kbl_dmc_ver1_04.bin".to_owned()),
        "{firmware:?}"
    );
    assert!(
        !firmware.iter().any(|file| file.contains("navi10")),
        "{firmware:?}"
    );
    assert!(
        !firmware.iter().any(|file| file.starts_with("i915/tgl")),
        "{firmware:?}"
    );
    assert_eq!(selection.report[0].split(':').next(), Some("amdgpu"));
    assert!(
        selection.report[0].contains("navi4x (gfx12): "),
        "{:?}",
        selection.report
    );
}

/// A sysfs PCI device directory.
fn write_device(sysfs: &TempDir, address: &str, class: &str, vendor: &str, device: &str) {
    sysfs.write(format!("{address}/class"), format!("{class}\n"));
    sysfs.write(format!("{address}/vendor"), format!("{vendor}\n"));
    sysfs.write(format!("{address}/device"), format!("{device}\n"));
}

/// The IP versions of a running Navi 44.
fn write_navi44_blocks(sysfs: &TempDir, address: &str) {
    let blocks = [
        ("GC", "12 0 0"),
        ("MP0", "14 0 2"),
        ("MP1", "14 0 2"),
        ("SDMA0", "7 0 0"),
        ("DMU", "4 0 1"),
        ("UVD", "5 0 0"),
    ];
    for (block, version) in blocks {
        let dir = format!("{address}/ip_discovery/die/0/{block}/0");
        for (name, number) in ["major", "minor", "revision"]
            .iter()
            .zip(version.split(' '))
        {
            sysfs.write(format!("{dir}/{name}"), format!("{number}\n"));
        }
    }
}

#[test]
fn the_host_scan_finds_display_controllers() {
    let sysfs = TempDir::new("gpu-scan");
    write_device(&sysfs, "0000:03:00.0", "0x030000", "0x1002", "0x7590");
    write_navi44_blocks(&sysfs, "0000:03:00.0");
    write_device(&sysfs, "0000:00:02.0", "0x030000", "0x8086", "0x4680");
    write_device(&sysfs, "0000:04:00.0", "0x020000", "0x8086", "0x15f3");

    let devices = scan(sysfs.path());

    assert_eq!(devices.len(), 2, "{devices:?}");
    assert_eq!(devices[0].address, "0000:00:02.0");
    assert_eq!((devices[0].vendor, devices[0].device), (0x8086, 0x4680));
    assert!(devices[0].blocks.is_empty());
    assert_eq!(devices[1].blocks[0], ("GC".to_owned(), (12, 0, 0)));
    assert_eq!(devices[1].blocks.len(), 6);
    assert!(scan(&sysfs.join("missing")).is_empty());
}

#[test]
fn a_running_amdgpu_gives_the_exact_chip() {
    let sysfs = TempDir::new("gpu-navi44");
    write_device(&sysfs, "0000:03:00.0", "0x030000", "0x1002", "0x7590");
    write_navi44_blocks(&sysfs, "0000:03:00.0");
    let devices = scan(sysfs.path());

    let choices = choose(&parse(&words("amdgpu")).unwrap(), &devices).unwrap();
    let selection = select(&choices, &listed()).unwrap();

    assert_eq!(
        choices[0].label,
        "navi4x, GC 12.0.0, at 0000:03:00.0 (1002:7590)"
    );
    for file in &selection.firmware {
        let known = [
            "amdgpu/gc_12_0_0_",
            "amdgpu/psp_14_0_2_",
            "amdgpu/smu_14_0_2.",
            "amdgpu/sdma_7_0_0.",
            "amdgpu/dcn_4_0_1_",
            "amdgpu/vcn_5_0_0.",
        ];
        assert!(known.iter().any(|stem| file.starts_with(stem)), "{file}");
    }
    assert!(
        selection
            .firmware
            .contains(&"amdgpu/smu_14_0_2.bin".to_owned())
    );
    assert!(selection.firmware.len() > 10, "{:?}", selection.firmware);
}

#[test]
fn without_a_running_driver_a_gpu_takes_its_group() {
    let devices = [Device {
        address: "0000:03:00.0".to_owned(),
        vendor: 0x1002,
        device: 0x7590,
        blocks: Vec::new(),
    }];

    let choices = choose(&parse(&words("amdgpu")).unwrap(), &devices).unwrap();
    let selection = select(&choices, &listed()).unwrap();

    assert_eq!(choices[0].label, "navi4x at 0000:03:00.0 (1002:7590)");
    assert!(
        selection
            .firmware
            .contains(&"amdgpu/gc_12_0_1_mes.bin".to_owned())
    );
}

#[test]
fn a_chip_named_after_itself_ignores_ip_versions() {
    let mut device = Device {
        address: "0000:03:00.0".to_owned(),
        vendor: 0x1002,
        device: 0x731f,
        blocks: vec![("GC".to_owned(), (10, 1, 0))],
    };

    let choices = choose(&parse(&words("amdgpu")).unwrap(), &[device.clone()]).unwrap();
    assert_eq!(choices[0].stems, [vec!["amdgpu/navi10".to_owned()]]);

    device.device = 0x7700;
    let choices = choose(&parse(&words("amdgpu")).unwrap(), &[device.clone()]).unwrap();
    assert_eq!(choices[0].label, "GC 10.1.0 at 0000:03:00.0 (1002:7700)");

    device.blocks.clear();
    let error = choose(&parse(&words("amdgpu")).unwrap(), &[device]).unwrap_err();
    assert!(
        error.contains("the AMD GPU at 0000:03:00.0 (1002:7700) is not one"),
        "{error}"
    );
    assert!(error.contains("such as amdgpu:gfx12"), "{error}");
}

#[test]
fn a_revision_of_0_may_be_left_out_of_the_name() {
    let devices = [Device {
        address: "0000:c4:00.0".to_owned(),
        vendor: 0x1002,
        device: 0x150e,
        blocks: vec![
            ("GC".to_owned(), (11, 5, 0)),
            ("DMU".to_owned(), (3, 5, 0)),
            ("MP1".to_owned(), (14, 0, 0)),
        ],
    }];

    let choices = choose(&parse(&words("amdgpu")).unwrap(), &devices).unwrap();
    let selection = select(&choices, &listed()).unwrap();

    assert!(
        selection
            .firmware
            .contains(&"amdgpu/dcn_3_5_dmcub.bin".to_owned())
    );
    assert_eq!(selection.unlisted, ["amdgpu/smu_14_0_0"]);
}

#[test]
fn intel_gpus_go_to_their_own_driver() {
    let devices = [Device {
        address: "0000:00:02.0".to_owned(),
        vendor: 0x8086,
        device: 0x64a0,
        blocks: Vec::new(),
    }];

    let error = choose(&parse(&words("i915")).unwrap(), &devices).unwrap_err();
    assert!(error.contains("it has no Intel GPU for it"), "{error}");
    assert!(
        error.contains("0000:00:02.0 (8086:64a0) is lnl, which xe drives"),
        "{error}"
    );

    let choices = choose(&parse(&words("xe")).unwrap(), &devices).unwrap();
    let selection = select(&choices, &listed()).unwrap();
    assert!(selection.firmware.contains(&"xe/lnl_guc_70.bin".to_owned()));
    assert!(
        selection
            .firmware
            .contains(&"i915/xe2lpd_dmc.bin".to_owned())
    );
}

#[test]
fn a_machine_without_the_gpu_is_refused() {
    let error = choose(&parse(&words("amdgpu")).unwrap(), &[]).unwrap_err();

    assert!(error.contains("no AMD GPU for it"), "{error}");
}

#[test]
fn a_kernel_that_lists_no_firmware_of_a_chip_does_not_know_it() {
    let choices = choose(&parse(&words("amdgpu:navi4x")).unwrap(), &[]).unwrap();
    let mut old = listed();
    old.insert(
        Driver::Amdgpu,
        vec![
            "amdgpu/navi10_me.bin".to_owned(),
            "amdgpu/psp_14_0_2_sos.bin".to_owned(),
        ],
    );

    let error = select(&choices, &old).unwrap_err();

    assert!(
        error.contains("amdgpu lists no firmware amdgpu/gc_12_0_0* for navi4x (gfx12)"),
        "{error}"
    );
}

#[test]
fn chips_of_a_family_the_kernel_does_not_know_are_left_out() {
    let choices = choose(&parse(&words("i915:gen9")).unwrap(), &[]).unwrap();
    let mut old = BTreeMap::new();
    old.insert(Driver::I915, vec!["i915/kbl_dmc_ver1_04.bin".to_owned()]);

    let selection = select(&choices, &old).unwrap();

    assert_eq!(selection.firmware, ["i915/kbl_dmc_ver1_04.bin"]);
    assert!(
        selection
            .report
            .contains(&"i915: skl (gen9): not known to the kernel, left out".to_owned()),
        "{:?}",
        selection.report
    );

    old.insert(Driver::I915, Vec::new());
    let error = select(&choices, &old).unwrap_err();
    assert_eq!(
        error,
        "i915 lists no firmware for any GPU of gen9: the kernel does not know them"
    );
}
