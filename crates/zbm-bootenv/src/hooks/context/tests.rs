//! The context files of the hook API version 2, on a fake machine.

use serde_json::json;
use zbm_platform::{Installed, KernelLog};
use zbm_testkit::fakes::FakeSystem;

use super::super::{BootSteps, HOOKS, Variables, run_stage};
use super::*;

fn log(fake: &FakeSystem) -> Installed {
    fake.files.dir("/zfsbootmenu");
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
}

/// The variables of an image's configuration, with `BOOTFS`.
fn variables() -> Variables {
    Variables::from(
        [
            ("ZBM_RUNTIME", "rust"),
            ("ZBM_BUILDSTYLE", "dracut"),
            ("ZBM_RELEASE_BUILD", ""),
            ("BOOTFS", "tank/ROOT/void"),
        ]
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .to_vec(),
    )
}

/// A machine with two imported pools, under lockdown, without Secure Boot.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .dir("/proc/spl/kstat/zfs/tank")
        .dir("/proc/spl/kstat/zfs/boot")
        .file("/proc/spl/kstat/zfs/arcstats", "")
        .file(
            "/sys/kernel/security/lockdown",
            "none [integrity] confidentiality\n",
        );
    fake
}

#[test]
fn each_stage_says_what_it_knows() {
    let fake = machine();
    let context = fake.context();
    let variables = variables();
    let kernel = Kernel::new("tank/ROOT/void", "/boot/vmlinuz", "/boot/initramfs.img");
    let env = |stage| HookEnv {
        variables: &variables,
        stage,
    };
    let common = json!({
        "api": 2,
        "runtime": "rust",
        "build_style": "dracut",
        "release": false,
        "locked": false,
        "lockdown": "integrity",
        "secure_boot": false,
    });
    let with = |stage: &str, known: serde_json::Value| {
        let mut expected = common.clone();
        let object = expected.as_object_mut().unwrap();
        object.insert("stage".to_owned(), json!(stage));
        object.extend(known.as_object().unwrap().clone());
        expected
    };
    let found = json!({
        "pools": ["boot", "tank"],
        "boot_environment": "tank/ROOT/void",
    });
    let selected = json!({
        "boot_environment": "tank/ROOT/void",
        "kernel": "/boot/vmlinuz",
        "initramfs": "/boot/initramfs.img",
    });

    // Early setup: nothing is imported yet.
    assert_eq!(
        document(&context, &env(StageEnv::EarlySetup)),
        with("early-setup.d", json!({}))
    );
    assert_eq!(
        document(&context, &env(StageEnv::Setup)),
        with("setup.d", found.clone())
    );
    let mut load_key = found.as_object().unwrap().clone();
    load_key.extend([
        ("locked_fs".to_owned(), json!("tank/ROOT/void")),
        ("encryption_root".to_owned(), json!("tank")),
    ]);
    assert_eq!(
        document(
            &context,
            &env(StageEnv::LoadKey {
                fs: "tank/ROOT/void",
                root: "tank"
            })
        ),
        with("load-key.d", json!(load_key))
    );
    let mut boot_sel = found.as_object().unwrap().clone();
    boot_sel.extend([
        ("selected".to_owned(), selected.clone()),
        ("mountpoint".to_owned(), json!("/mnt")),
    ]);
    assert_eq!(
        document(
            &context,
            &env(StageEnv::BootSel {
                selected: &kernel,
                mountpoint: "/mnt"
            })
        ),
        with("boot-sel.d", json!(boot_sel))
    );
    let tree = Path::new("/mnt/boot/board.dtb");
    let loaded = Loaded {
        command_line: "root=zfs:tank/ROOT/void quiet",
        devicetree: Some(tree),
        by: "kexec",
    };
    let mut teardown = found.as_object().unwrap().clone();
    teardown.extend([
        ("selected".to_owned(), selected),
        (
            "command_line".to_owned(),
            json!("root=zfs:tank/ROOT/void quiet"),
        ),
        ("devicetree".to_owned(), json!("/mnt/boot/board.dtb")),
        ("loaded_by".to_owned(), json!("kexec")),
    ]);
    assert_eq!(
        document(
            &context,
            &env(StageEnv::Teardown {
                selected: &kernel,
                loaded: Some(loaded)
            })
        ),
        with("teardown.d", json!(teardown))
    );
}

#[test]
fn what_is_not_known_is_null() {
    let fake = FakeSystem::new();
    fake.files
        .file(zbm_config::conf::LOCK, "$argon2id$...\n")
        .file(
            "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c",
            b"\x06\x00\x00\x00\x01".as_slice(),
        );
    let none = Variables::default();
    let kernel = Kernel::new("tank/ROOT/void", "/boot/vmlinuz", "/boot/initramfs.img");
    let document = document(
        &fake.context(),
        &HookEnv {
            variables: &none,
            stage: StageEnv::Teardown {
                selected: &kernel,
                loaded: None,
            },
        },
    );
    for name in [
        "runtime",
        "build_style",
        "lockdown",
        "boot_environment",
        "command_line",
        "devicetree",
        "loaded_by",
    ] {
        assert_eq!(document[name], json!(null), "{name}");
    }
    assert_eq!(document["pools"], json!([]));
    assert_eq!(document["locked"], json!(true));
    assert_eq!(document["secure_boot"], json!(true));
}

/// ADR 0017, point 8: the file is written before the stage's hooks, read
/// only, named by `ZBM_HOOK_CONTEXT`, and removed after them.
#[test]
fn the_hooks_get_the_file_while_the_stage_runs() {
    let fake = machine();
    let _log = log(&fake);
    fake.files
        .file_with_mode("/libexec/hooks/setup.d/10-a", "#!/bin/sh\n", 0o755);
    let path = "/run/zbm/hooks/setup.d.json";
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let files = fake.files.clone();
    let record = std::sync::Arc::clone(&seen);
    fake.runner.on(move |_| {
        let metadata = zbm_platform::Files::metadata(&files, Path::new(path)).ok();
        *record.lock().unwrap() = Some((files.text(path), metadata.map(|m| m.mode & 0o777)));
        None
    });
    let variables = variables();
    let env = HookEnv {
        variables: &variables,
        stage: StageEnv::Setup,
    };
    run_stage(
        &fake.context(),
        Path::new(HOOKS),
        &env,
        &BootSteps::default(),
        false,
    );

    let (text, mode) = seen.lock().unwrap().clone().expect("the hook ran");
    let document: serde_json::Value = serde_json::from_str(&text.unwrap()).unwrap();
    assert_eq!(document["stage"], json!("setup.d"));
    assert_eq!(mode, Some(0o444));
    let call = fake.runner.calls().pop().unwrap();
    let env: Vec<(String, String)> = call
        .get_env()
        .iter()
        .map(|(name, value)| {
            let value = value.clone().unwrap_or_default();
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert!(env.contains(&("ZBM_HOOK_API".to_owned(), "2".to_owned())));
    assert!(env.contains(&("ZBM_HOOK_CONTEXT".to_owned(), path.to_owned())));
    assert!(
        !fake.files.exists(Path::new(path)),
        "removed after the stage"
    );
}

#[test]
fn a_stage_of_steps_alone_writes_no_file_and_a_file_not_written_is_logged() {
    let fake = machine();
    let _log = log(&fake);
    let variables = variables();
    let env = HookEnv {
        variables: &variables,
        stage: StageEnv::Setup,
    };
    run_stage(
        &fake.context(),
        Path::new(HOOKS),
        &env,
        &BootSteps::default(),
        false,
    );
    assert!(!fake.files.exists(Path::new(CONTEXTS)));

    // /run cannot be written: the hooks get the variable empty, not one
    // a stage they run in passed on.
    fake.environment
        .set("ZBM_HOOK_CONTEXT", "/run/zbm/hooks/boot-sel.d.json");
    fake.files
        .file_with_mode("/libexec/hooks/setup.d/10-a", "#!/bin/sh\n", 0o755)
        .fail(CONTEXTS, std::io::ErrorKind::PermissionDenied);
    run_stage(
        &fake.context(),
        Path::new(HOOKS),
        &env,
        &BootSteps::default(),
        false,
    );
    let call = fake.runner.calls().pop().unwrap();
    assert!(
        call.get_env().iter().any(
            |(name, value)| name == "ZBM_HOOK_CONTEXT" && value.as_deref() == Some("".as_ref())
        ),
        "{:?}",
        call.get_env()
    );
    assert!(
        fake.kmsg()
            .contains("unable to write the hook context /run/zbm/hooks/setup.d.json"),
        "{}",
        fake.kmsg()
    );
}
