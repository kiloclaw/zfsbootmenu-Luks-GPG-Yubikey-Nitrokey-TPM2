use pretty_assertions::assert_eq;

use super::*;

/// Settings with a fresh work directory and a checkout holding one hook.
fn settings(name: &str) -> Settings {
    let root =
        std::env::temp_dir().join(format!("zbm-vmtest-images-{name}-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    let hooks = root.join("repo/tests/luks/vm/hooks/teardown.d");
    fs::create_dir_all(&hooks).unwrap();
    fs::write(hooks.join("99-report.sh"), "#!/bin/sh\n").unwrap();
    let mut settings = Settings::from_vars(|_| None, false, false);
    settings.work = root.join("work");
    settings.repo = root.join("repo");
    // A container runtime that fails at once.
    settings.container = "false".into();
    settings
}

fn cleanup(settings: &Settings) {
    if let Some(root) = settings.work.parent() {
        fs::remove_dir_all(root).ok();
    }
}

fn spec() -> ImageSpec {
    ImageSpec {
        crypttab: Some("tank-a PARTLABEL=zfs2 none -\n".into()),
        files: vec![
            ("hooks/early-setup.d/95-x".into(), b"#!/bin/sh\n".to_vec()),
            ("keys/tank.key.gpg".into(), vec![0, 1, 2]),
        ],
        conf: "zfsbootmenu_luks_token_plugins=\"no\"".into(),
        cmdline: Some("console=ttyS0".into()),
        efi: true,
        rc: Some("cp /build/keys/tank.key.gpg /etc/".into()),
        ..ImageSpec::new("keys-native")
    }
}

#[test]
fn the_build_directory_holds_what_the_image_is_built_from() {
    let settings = settings("prepare");
    let dir = settings.work.join("zbm/keys-native");

    prepare_build(&settings, &spec(), &dir, "extra=\"1\"").unwrap();

    let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap();
    assert_eq!(
        read("dracut.conf.d/zz-test.conf"),
        "zfsbootmenu_hook_root=\"/build/hooks\"\ncompress=\"zstd\"\n\
         zfsbootmenu_luks_crypttab=\"/build/crypttab\"\nextra=\"1\"\n"
    );
    assert_eq!(read("crypttab"), "tank-a PARTLABEL=zfs2 none -\n");
    assert_eq!(
        read("rc.d/10-test"),
        "#!/bin/sh\nset -e\ncp /build/keys/tank.key.gpg /etc/\n"
    );
    assert_eq!(
        read("config.yaml"),
        "Components:\n  Enabled: true\n  Versions: false\nEFI:\n  Enabled: true\n  \
         Versions: false\nKernel:\n  Version: 7.3.*\n  CommandLine: \"console=ttyS0\"\n"
    );
    assert_eq!(fs::read(dir.join("hostid")).unwrap(), HOSTID);
    assert!(dir.join("hooks/teardown.d/99-report.sh").exists());
    let mode = |name: &str| fs::metadata(dir.join(name)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode("hooks/early-setup.d/95-x"), 0o755);
    assert_eq!(mode("rc.d/10-test"), 0o755);
    assert_eq!(
        fs::read(dir.join("keys/tank.key.gpg")).unwrap(),
        vec![0, 1, 2]
    );
    cleanup(&settings);
}

#[test]
fn a_console_font_is_a_global_key_of_the_generate_zbm_configuration() {
    let spec = ImageSpec {
        console_font: Some("spleen".into()),
        ..ImageSpec::new("auto-spleen")
    };

    assert!(
        generate_config(&spec).starts_with("Global:\n  ConsoleFont: spleen\nComponents:\n"),
        "{}",
        generate_config(&spec)
    );
    assert_ne!(spec.stamp(), ImageSpec::new("auto-spleen").stamp());
}

#[test]
fn an_image_without_a_crypttab_names_none() {
    let conf = dracut_conf(&ImageSpec::new("auto"), "");

    assert!(conf.contains("zfsbootmenu_luks_crypttab=\"/build/no-crypttab\"\n"));
}

#[test]
fn a_failed_build_names_its_log() {
    let settings = settings("failed");

    let error = zbm_image(&settings, &ImageSpec::new("auto")).unwrap_err();

    assert!(matches!(&error, Error::Build { log, .. } if log.ends_with("zbm/auto/build.log")));
    let conf = fs::read_to_string(settings.work.join("zbm/auto/dracut.conf.d/zz-test.conf"));
    assert!(conf.unwrap().ends_with("\n\nzfsbootmenu_shadow=\"yes\"\n"));
    cleanup(&settings);
}

#[test]
fn an_image_built_from_the_same_inputs_is_reused() {
    let mut settings = settings("reuse");
    settings.reuse = true;
    settings.runtime = ZbmRuntime::Rust;
    let spec = spec();
    let build = settings.work.join("zbm/keys-native-rust/build");
    fs::create_dir_all(&build).unwrap();
    fs::write(build.join("initramfs-bootmenu.img"), "").unwrap();
    fs::write(build.join("vmlinuz.EFI"), "").unwrap();
    let conf = "zfsbootmenu_luks_token_plugins=\"no\"\nzfsbootmenu_runtime=\"rust\"";
    let stamp_path = settings.work.join("zbm/keys-native-rust/inputs.txt");
    // Recorded without the sources, as before they were inputs: built anew.
    fs::write(&stamp_path, format!("{}conf+ {conf}\n", spec.stamp())).unwrap();
    assert!(zbm_image(&settings, &spec).is_err());
    fs::create_dir_all(&build).unwrap();
    fs::write(build.join("initramfs-bootmenu.img"), "").unwrap();
    fs::write(build.join("vmlinuz.EFI"), "").unwrap();
    fs::write(&stamp_path, inputs(&settings, &spec, conf)).unwrap();

    let image = zbm_image(&settings, &spec).unwrap();

    assert_eq!(image.kernel, build.join("vmlinuz-bootmenu"));
    let mut changed = spec.clone();
    changed.files[1].1.push(3);
    assert!(zbm_image(&settings, &changed).is_err());
    cleanup(&settings);
}

#[test]
fn the_stamp_does_not_depend_on_the_order_of_the_files() {
    let mut reversed = spec();
    reversed.files.reverse();

    assert_eq!(reversed.stamp(), spec().stamp());
    assert_ne!(ImageSpec::new("a").stamp(), spec().stamp());
}

#[test]
fn the_host_id_argument_is_that_of_the_test_images() {
    let settings = settings("hostid");

    assert_eq!(hostid_arg(&settings).unwrap(), "spl_hostid=0x00bab10c");
    assert!(matches!(
        build_be(&settings, false),
        Err(Error::Build { .. })
    ));
    cleanup(&settings);
}
