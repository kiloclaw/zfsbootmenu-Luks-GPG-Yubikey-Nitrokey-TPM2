//! `Kernel.GraphicsDrivers` (ROADMAP.md #17), which only the Rust program
//! reads: the firmware of the named families, from what the fake `modinfo`
//! lists, reaches `dracut` and `mkinitcpio` as build options. The GPUs of
//! the build host are covered by the unit tests of `gpu`, with a fake sysfs.

use crate::fixtures::COMPONENTS;
use crate::sandbox::{Case, Sandbox, run_rust};

const ARGS: &[&str] = &["-c", "{root}/config.yaml", "-b", "{root}/boot"];

/// The components configuration with a command line and
/// `Kernel.GraphicsDrivers: drivers`.
fn with_drivers(drivers: &str) -> String {
    format!(
        "{COMPONENTS}Kernel:\n  CommandLine: ro nomodeset quiet\n  GraphicsDrivers: {drivers}\n"
    )
}

/// What amdgpu and i915 of the fake kernel list.
fn setup_modinfo(sandbox: &Sandbox) {
    sandbox.dir.write(
        "modinfo/amdgpu",
        "amdgpu/navi10_me.bin\namdgpu/gc_12_0_0_me.bin\namdgpu/gc_12_0_1_me.bin\n\
         amdgpu/psp_14_0_2_sos.bin\namdgpu/smu_14_0_2.bin\n",
    );
    sandbox.dir.write(
        "modinfo/i915",
        "i915/kbl_dmc_ver1_04.bin\ni915/tgl_dmc.bin\ni915/tgl_guc_70.bin\n",
    );
}

fn case(config: &str) -> Case {
    let mut case = Case::new(config, ARGS);
    case.setup = setup_modinfo;
    case
}

#[test]
fn dracut_gets_the_drivers_and_the_firmware_of_the_families() {
    let run = run_rust(&case(&with_drivers("amdgpu:gfx12 i915:tgl")));

    assert_eq!(run.code, 0, "{}", run.stdout);
    assert!(
        run.stdout
            .contains("Graphics: amdgpu: navi4x (gfx12): 4 firmware files\n"),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains("Graphics: i915: tgl (gen12): 2 firmware files\n"),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains("Removing nomodeset from the command line, for the graphics drivers\n"),
        "{}",
        run.stdout
    );
    assert!(
        run.log
            .contains("modinfo [-k] [6.2.5-test] [-F] [firmware] [amdgpu]\n"),
        "{}",
        run.log
    );
    assert!(!run.log.contains("--add-drivers"), "{}", run.log);
    assert!(
        run.log
            .contains("zfsbootmenu_graphics_drivers=amdgpu i915\n"),
        "{}",
        run.log
    );
    let firmware = "zfsbootmenu_graphics_firmware=amdgpu/gc_12_0_0_me.bin \
                    amdgpu/gc_12_0_1_me.bin amdgpu/psp_14_0_2_sos.bin amdgpu/smu_14_0_2.bin \
                    i915/tgl_dmc.bin i915/tgl_guc_70.bin\n";
    assert!(run.log.contains(firmware), "{}", run.log);
}

#[test]
fn mkinitcpio_gets_the_build_options() {
    let config = with_drivers("i915:gen9").replace(
        "  ManageImages: true\n",
        "  ManageImages: true\n  InitCPIO: true\n  InitCPIOConfig: {root}/mkinitcpio.conf\n",
    );

    let run = run_rust(&case(&config));

    assert_eq!(run.code, 0, "{}", run.stdout);
    assert!(run.log.contains("mkinitcpio ["), "{}", run.log);
    let left_out = "Graphics: i915: skl (gen9): not known to the kernel, left out\n";
    assert!(run.stdout.contains(left_out), "{}", run.stdout);
    assert!(
        run.log
            .contains("zfsbootmenu_graphics_firmware=i915/kbl_dmc_ver1_04.bin\n"),
        "{}",
        run.log
    );
}

#[test]
fn an_unknown_family_stops_before_anything_is_built() {
    let run = run_rust(&case(&with_drivers("amdgpu:gfx13")));

    assert_eq!(run.code, 1);
    assert!(
        run.stdout
            .contains("Kernel.GraphicsDrivers: amdgpu has no chip or family 'gfx13'"),
        "{}",
        run.stdout
    );
    assert!(!run.log.contains("modinfo"), "{}", run.log);
    assert!(!run.log.contains("dracut"), "{}", run.log);
}

#[test]
fn a_driver_the_kernel_lacks_stops_the_build() {
    let run = run_rust(&case(&with_drivers("xe:xe2")));

    assert_eq!(run.code, 1);
    assert!(
        run.stdout.contains(
            "modinfo: ERROR: Module xe not found.\n\
             Unable to read the firmware list of xe for kernel 6.2.5-test\n"
        ),
        "{}",
        run.stdout
    );
    assert!(!run.log.contains("dracut"), "{}", run.log);
}

#[test]
fn a_gpu_the_kernel_does_not_know_stops_the_build() {
    let run = run_rust(&case(&with_drivers("amdgpu:navi3x")));

    assert_eq!(run.code, 1);
    assert!(
        run.stdout.contains(
            "Kernel.GraphicsDrivers: amdgpu lists no firmware amdgpu/gc_11_0_0* for navi3x \
             (gfx11): the kernel does not know this GPU (kernel 6.2.5-test)"
        ),
        "{}",
        run.stdout
    );
    assert!(!run.log.contains("dracut"), "{}", run.log);
}
