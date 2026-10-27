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

    // The images are the Rust runtime's unless ZBMTEST_ZBM_RUNTIME=legacy.
    assert!(matches!(&error, Error::Build { log, .. } if log.ends_with("zbm/auto-rust/build.log")));
    let conf = fs::read_to_string(
        settings
            .work
            .join("zbm/auto-rust/dracut.conf.d/zz-test.conf"),
    );
    assert!(conf.is_ok_and(|conf| conf.ends_with("\nzfsbootmenu_runtime=\"rust\"\n")));
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
    assert!(matches!(build_be(&settings), Err(Error::Build { .. })));
    cleanup(&settings);
}

#[test]
fn the_boot_environment_is_built_again_when_its_inputs_change() {
    let settings = settings("be");
    let payload = settings.work.join("payload");
    fs::create_dir_all(&payload).unwrap();
    fs::write(payload.join("kver"), "7.3.1-zbm\n").unwrap();
    // Built before its inputs were recorded: built again.
    assert!(matches!(build_be(&settings), Err(Error::Build { .. })));

    let be = settings.repo.join("tests/luks/vm/be");
    fs::create_dir_all(&be).unwrap();
    fs::write(be.join("init.sh"), "echo BE-INIT-START\n").unwrap();
    let stamp = format!("be {}\ncontainer \n", source::fingerprint_of(&be));
    fs::write(settings.work.join("payload-inputs.txt"), stamp).unwrap();
    build_be(&settings).unwrap();
    assert_eq!(be_kver(&settings).unwrap(), "7.3.1-zbm");

    fs::write(be.join("init.sh"), "echo BE-INIT-START; echo BE-NEW\n").unwrap();
    assert!(matches!(build_be(&settings), Err(Error::Build { .. })));
    cleanup(&settings);
}

#[test]
fn a_second_process_waits_for_the_image_being_built() {
    let settings = settings("lock");
    let images = settings.work.join("zbm");
    let held = lock(&images, "auto").unwrap();
    let waiting = {
        let images = images.clone();
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            drop(lock(&images, "auto").unwrap());
            start.elapsed()
        })
    };
    // Another image is not held up.
    drop(lock(&images, "auto-rust").unwrap());
    std::thread::sleep(std::time::Duration::from_millis(200));
    drop(held);
    let waited = waiting.join().unwrap();
    assert!(
        waited >= std::time::Duration::from_millis(200),
        "{waited:?}"
    );
    cleanup(&settings);
}

#[test]
fn each_process_boots_its_own_links_which_a_rebuild_leaves_alone() {
    let settings = settings("links");
    let images = settings.work.join("zbm");
    let build = images.join("auto-rust/build");
    fs::create_dir_all(&build).unwrap();
    fs::write(build.join("vmlinuz-bootmenu"), "kernel 1").unwrap();
    fs::write(build.join("initramfs-bootmenu.img"), "initrd 1").unwrap();
    let built = Image {
        kernel: build.join("vmlinuz-bootmenu"),
        initrd: build.join("initramfs-bootmenu.img"),
        efi: build.join("vmlinuz.EFI"),
        debug: build.join("debug"),
    };
    // The directory of a process that has ended (no PID is this large).
    let ended = images.join("in-use/auto.4294967295");
    fs::create_dir_all(&ended).unwrap();

    let own = own_links(&built, &images, "auto-rust").unwrap();

    let dir = images.join(format!("in-use/auto-rust.{}", std::process::id()));
    assert_eq!(own.kernel, dir.join("vmlinuz-bootmenu"));
    assert_eq!(own.initrd, dir.join("initramfs-bootmenu.img"));
    // No EFI image was built: the path stays as it was.
    assert_eq!(own.efi, built.efi);
    assert!(!ended.exists());

    // Another process rebuilds the variant: the links keep the old files.
    fs::remove_dir_all(&build).unwrap();
    fs::create_dir_all(&build).unwrap();
    fs::write(build.join("vmlinuz-bootmenu"), "kernel 2").unwrap();
    assert_eq!(fs::read_to_string(&own.kernel).unwrap(), "kernel 1");
    assert_eq!(fs::read_to_string(&own.initrd).unwrap(), "initrd 1");

    // This process's next build replaces its links.
    let again = own_links(&built, &images, "auto-rust").unwrap();
    assert_eq!(fs::read_to_string(&again.kernel).unwrap(), "kernel 2");
    assert!(!again.initrd.exists());
    cleanup(&settings);
}
