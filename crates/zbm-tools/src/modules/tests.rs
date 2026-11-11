use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeSystem;

use super::*;

const ROOT: &str = "/lib/modules/6.12.8";

/// A system whose kernel has an index of ZFS's modules and a GPU driver.
fn system() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.file("/proc/sys/kernel/osrelease", "6.12.8\n");
    fake.files
        .file("/proc/cmdline", "root=zfs zfs.zfs_arc_max=2\n");
    fake.files.file(
        format!("{ROOT}/modules.dep"),
        "kernel/zfs/zfs.ko.zst: kernel/zfs/spl.ko.zst\nkernel/zfs/spl.ko.zst:\n\
         kernel/drm/i915.ko:\nkernel/tpm/tpm_crb.ko:\n",
    );
    fake.files.file(format!("{ROOT}/modules.alias"), "");
    fake.files.file(format!("{ROOT}/modules.softdep"), "");
    fake.files
        .file(format!("{ROOT}/modules.builtin"), "kernel/tty/vt.ko\n");
    fake.files
        .file("/etc/modprobe.d/zfs.conf", "options spl spl_hostid=0x7\n");
    fake.files
        .file("/lib/modprobe.d/zfs.conf", "options spl spl_hostid=0xbad\n");
    fake.files
        .file("/lib/modprobe.d/other.conf", "install i915 /bin/true\n");
    fake
}

/// The modules `fake` inserted: their files' names and parameters.
fn inserted(fake: &FakeSystem) -> Vec<(String, String)> {
    fake.modules
        .inserted()
        .into_iter()
        .map(|(path, parameters)| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                parameters,
            )
        })
        .collect()
}

fn pair(file: &str, parameters: &str) -> (String, String) {
    (file.to_owned(), parameters.to_owned())
}

#[test]
fn a_module_loads_itself_with_what_it_needs_and_its_options() {
    let fake = system();
    let modules = Modules::of(&fake.system());

    modules.load("zfs").unwrap();
    modules.load("vt").unwrap();

    assert_eq!(
        inserted(&fake),
        [
            pair("spl.ko.zst", "spl_hostid=0x7"),
            pair("zfs.ko.zst", "zfs_arc_max=2")
        ]
    );
    assert!(fake.runner.lines().is_empty());
}

#[test]
fn what_is_loaded_already_is_left_and_failures_are_said() {
    let fake = system();
    fake.files.dir("/sys/module/spl");
    fake.modules
        .fail(format!("{ROOT}/kernel/tpm/tpm_crb.ko"), 19);
    fake.modules
        .fail(format!("{ROOT}/kernel/zfs/zfs.ko.zst"), 17);
    let modules = Modules::of(&fake.system());

    modules.load("zfs").unwrap();
    let failed = modules.load("tpm_crb").unwrap_err();
    let unknown = modules.load("nouveau").unwrap_err();
    modules.probe_quietly("tpm_crb");

    assert!(inserted(&fake).is_empty());
    assert!(failed.starts_with("tpm_crb: "), "{failed}");
    assert_eq!(unknown, "module nouveau not found");
    // `modprobe -q` is asked when the module did not load.
    assert_eq!(fake.runner.lines(), ["modprobe -q tpm_crb"]);
}

#[test]
fn modprobe_loads_what_the_runtime_is_beyond_kernel() {
    let fake = system();
    // A kernel that cannot decompress the module's file.
    fake.modules
        .fail(format!("{ROOT}/kernel/zfs/spl.ko.zst"), 22);
    fake.runner
        .respond("modprobe i915", 1, "modprobe: ERROR: i915 failed\n");
    let modules = Modules::of(&fake.system());

    modules.load("zfs").unwrap();
    let configured = modules.load("i915").unwrap_err();

    assert_eq!(configured, "modprobe: ERROR: i915 failed");
    assert_eq!(fake.runner.lines(), ["modprobe zfs", "modprobe i915"]);
    assert!(fake.runner.calls()[0].merges_stderr());
}

#[test]
fn without_an_index_the_programs_do_it_all() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("modinfo -F filename spl", 0, "/lib/modules/spl.ko\n");
    let modules = Modules::of(&fake.system());

    modules.probe_quietly("tpm_crb");
    modules.load("zfs").unwrap();
    modules.insert_alone("spl", &["spl_hostid=0"]).unwrap();
    fake.runner.respond("modinfo -F filename zfs", 0, "");
    let none = modules.insert_alone("zfs", &[]).unwrap_err();

    assert_eq!(
        fake.runner.lines(),
        [
            "modprobe -q tpm_crb",
            "modprobe zfs",
            "modinfo -F filename spl",
            "insmod /lib/modules/spl.ko spl_hostid=0",
            "modinfo -F filename zfs",
        ]
    );
    assert_eq!(none, "module zfs not found");
    let calls = fake.runner.calls();
    assert_eq!(calls[0].get_stdout(), Stream::Null);
    assert!(calls[1].merges_stderr() && calls[3].merges_stderr());
}

#[test]
fn a_module_inserted_alone_has_only_the_parameters_given() {
    let fake = system();
    let modules = Modules::of(&fake.system());

    modules.insert_alone("spl", &["spl_hostid=0"]).unwrap();
    let unknown = modules.insert_alone("nouveau", &[]).unwrap_err();
    fake.modules
        .fail(format!("{ROOT}/kernel/zfs/zfs.ko.zst"), 8);
    let refused = modules.insert_alone("zfs", &[]).unwrap_err();
    fake.modules.fail(format!("{ROOT}/kernel/drm/i915.ko"), 17);
    modules.insert_alone("i915", &[]).unwrap();

    assert_eq!(inserted(&fake), [pair("spl.ko.zst", "spl_hostid=0")]);
    assert_eq!(unknown, "module nouveau not found");
    assert!(refused.starts_with("zfs: "), "{refused}");
}

#[test]
fn the_kernel_cannot_when_it_lacks_the_call_or_a_compressed_file_is_refused() {
    let compressed = Path::new("zfs.ko.zst");
    let plain = Path::new("zfs.ko");
    let errno = io::Error::from_raw_os_error;
    assert!(is_beyond_kernel(&io::ErrorKind::Unsupported.into(), plain));
    assert!(is_beyond_kernel(&errno(22), compressed));
    assert!(!is_beyond_kernel(&errno(22), plain));
    assert!(is_beyond_kernel(&errno(38), plain));
    assert!(!is_beyond_kernel(&errno(8), compressed));
}

#[test]
fn a_softdep_that_fails_is_passed_over_and_probing_loads_itself() {
    let fake = system();
    fake.files.file(
        "/etc/modprobe.d/tpm.conf",
        "softdep tpm_crb pre: i915_extra\n",
    );
    fake.files.file(
        format!("{ROOT}/modules.dep"),
        "kernel/tpm/tpm_crb.ko:\nkernel/drm/i915_extra.ko:\n",
    );
    fake.modules
        .fail(format!("{ROOT}/kernel/drm/i915_extra.ko"), 19);
    let modules = Modules::of(&fake.system());

    modules.probe_quietly("tpm_crb");

    assert_eq!(inserted(&fake), [pair("tpm_crb.ko", "")]);
    assert!(fake.runner.lines().is_empty());
}

#[test]
fn a_program_that_cannot_start_says_why() {
    let fake = FakeSystem::new();
    fake.runner
        .on(|_| Some(Err(io::Error::from(io::ErrorKind::NotFound))));
    let modules = Modules::of(&fake.system());

    let failed = modules.load("zfs").unwrap_err();

    assert!(failed.contains("not found"), "{failed}");
}

#[test]
fn modinfo_names_a_module_s_file_and_version() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("modinfo -F filename zfs", 0, "/lib/modules/zfs.ko\n\n");
    fake.runner
        .respond("modinfo -F version zfs", 0, "2.2.6-1\n");
    fake.runner.respond("modinfo -F version spl", 1, "");
    let modules = Modules::of(&fake.system());

    assert_eq!(modules.file("zfs").as_deref(), Some("/lib/modules/zfs.ko"));
    assert_eq!(modules.version("zfs").as_deref(), Some("2.2.6-1"));
    assert_eq!(modules.version("spl"), None);
    fake.runner
        .on(|_| Some(Err(io::Error::from(io::ErrorKind::NotFound))));
    assert_eq!(modules.file("spl"), None);
    assert_eq!(fake.runner.calls()[0].get_stdin(), &Input::Null);
}
