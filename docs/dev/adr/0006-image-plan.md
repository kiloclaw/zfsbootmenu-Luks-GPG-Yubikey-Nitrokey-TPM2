# ADR 0006: The image plan and its backends

- **Status:** accepted, 29 October 2026 (proposed 28 October 2026)
- **Decides:** item SB8, and decision 16 of [ROADMAP.md](../../../ROADMAP.md#decisions) (a native image backend after
  the cutover)
- **Governs:** SB5, SB7, SB12, DM10, CD12, SE12, RL2

## Context

`generate-zbm` is Rust, but what goes into an image is decided in Bash: `generate-zbm` runs dracut or mkinitcpio, whose
ZFSBootMenu module (`packaging/dracut/module-setup.sh`, 281 lines) and install hook (`packaging/initcpio/install/
zfsbootmenu`, 289 lines) source `legacy/zfsbootmenu/install-helpers.sh` (1,465 lines). Those scripts choose the
runtime's files, the binaries and their libraries, kernel modules, firmware for the GPUs an image is for, udev rules,
fonts, keyboard layouts, the LUKS programs and tokens, and the hooks. So:

- the contents of an image are only known by building it, in a container with root, and cannot be checked by
  `cargo test`;
- the logic exists twice, once per backend, in the shells of two projects;
- Rust images still carry the legacy runtime (`legacy/zfsbootmenu/{lib,libexec,bin}`) and its programs (`awk`, `sed`,
  `od`, `tac`, `fzf` and others), because the helpers install them for both runtimes;
- the mkinitcpio hook needs Bash at boot to start `zbm` (`exec bash /libexec/zfsbootmenu-initcpio`).

Dracut stays needed after the port: remote unlocking relies on its modules (`dracut-crypt-ssh` with dropbear), and
distributions ship it.

## Decision

**`zbm-host` computes the image as a value, an `ImagePlan`; backends install it. A native backend comes after the
cutover, with dracut and mkinitcpio kept as backends.**

1. **`ImagePlan`.** A pure function of the configuration and of what it reads on the build host (kernel modules,
   firmware, keymaps, fonts, the binaries' paths) returns the plan: files with their source, destination and mode;
   links; directories; kernel modules with their soft dependencies; firmware; udev rules; the hooks of the image; and
   for each binary its path, not its libraries. A plan is per runtime: a Rust image's plan names its own binaries
   and none of the legacy payload (SB7).
2. **`ImageBackend`.** A trait with `install(&ImagePlan) -> Result<Image>`. `Dracut` and `Mkinitcpio` write the plan
   as a manifest (one line per entry: kind, source, destination, mode) and run the builder with a shim module that
   reads the manifest and calls `inst`, `inst_multiple`, `instmods` (dracut) or `add_file`, `add_binary`,
   `add_module` (mkinitcpio). The shims replace `module-setup.sh`, the install hook and `install-helpers.sh`; the
   builders keep resolving shared libraries, as they do now.
3. **The mkinitcpio hook runs `zbm` without Bash:** its run hook is a link to `zbm`, as dracut's `pre-mount` hook is
   a short script today.
4. **Golden plans.** `cargo test` checks the plans of a set of configurations against golden files, so a change to
   what goes into an image is a reviewable diff without building one.
5. **A native backend, after the cutover (decision 16, SB12).** Once the Rust runtime is the default (RL2), `Native`
   writes the initramfs itself: a `cpio` writer, the ELF closure of each binary (with the `object` crate, a
   decision 1 entry then), the module and firmware closure, and a minimal early init in `zbm-startup` that mounts the
   pseudo file systems and starts udev. It is checked against dracut's output for the same plan (`xtask
   compare-images`), and serves release and container images first. Dracut stays a backend for remote unlock and
   distribution packages; mkinitcpio stays for Arch.
6. **Signed and accounted for (SE12).** `zbm-host` signs the unified kernel image itself, builds reproducibly (#5), and
   writes the SBOM from the plan.

## Consequences

- What goes into an image is code with tests: a reviewer reads a golden diff, and a missing firmware file or font is a
  test failure, not a boot failure.
- One description of an image instead of two shell implementations; the Bash install helpers go, and with them the
  last Bash that decides anything at build time.
- Rust images shrink by the legacy payload, and the image's size (B1) shows it.
- The shims are new shell code, small and generic, that must keep up with dracut's and mkinitcpio's install APIs.
- Until the native backend, an image still needs dracut or mkinitcpio and a container with root to build.
- The native backend is a second way to build the same image, kept honest by comparing its output with dracut's.

## Alternatives considered

- **Keep the Bash helpers and only shrink them.** No new code, but the image stays untestable without a build.
- **Write the native backend now, and drop dracut and mkinitcpio.** Remote unlock and distribution packages depend on
  dracut's modules; the native backend would have to reimplement them before the Rust runtime is even the default.
- **Port the helpers line by line into Rust that still calls dracut's functions.** It moves the logic without making
  it a value, so it stays untestable.
- **Have the plan resolve shared libraries for dracut and mkinitcpio too.** One more thing to get right that the
  builders already do well; the native backend needs it, the others do not.

## How it is checked

- Golden plans under `cargo test`, for the legacy and the Rust runtime, with and without LUKS, graphics firmware and
  keyboard layouts.
- The VM suites build their images with dracut, through the backend. mkinitcpio has no VM test today; one image built
  with it boots in a new VM test once its shim exists.
- `xtask compare-images` between the native backend and dracut, once SB12 starts.
- B1 (image size) and B7 (process count) in the benchmarks.

## Where the work stands

On 29 October 2026, when the decision was accepted, nothing of it was built. On 1 November 2026 (SB8):

- **The plan** is a value, `zbm_image::Plan`, which `zbm_image::plan` computes from the builder's settings and the
  build host, through its files and its programs' answers (`zbm_tools::host`), for the legacy and the Rust runtime
  (answer 2). It lives in `crates/zbm-image`, in the orchestration layer beside `generate-zbm`, until DM10 makes
  `zbm-host`; the crate map puts the host tools among the front ends, which may not use adapters, so the plan cannot
  live there yet. The plan names each binary's path, not its libraries, and the builders resolve them (decision 2).
- **The manifest** is the plan's text (answer 1): `dir`, `file`, `link`, `hardlink`, `binary`, `library`, `module`,
  `rule`, `unomit`, `hook`, `hostonly`, `firmware`, `info` and `warn` lines, separated by tabs (`plan.rs` documents
  each). `zbm image plan --builder dracut|mkinitcpio --config FILE --stage DIR` prints it, the files the plan writes
  in `DIR`; the shims hand it the settings as `NAME<TAB>VALUE` lines.
- **The shims** (`packaging/dracut/module-setup.sh`, `packaging/initcpio/install/zfsbootmenu`) read the manifest with
  `while IFS=$'\t' read -r` and call `inst_*`, `instmods` and `mark_hostonly`, or `add_*`; dracut's `installkernel`
  takes the `module`, `unomit` and `firmware` lines, `install` the others. The graphics firmware is pruned on the
  built tree by `zbm image prune-firmware`. Every build needs `zbm` (the maintainer's choice when SB8 started), and
  `legacy/zfsbootmenu/install-helpers.sh` is gone; `crates/zbm/tests/image_shims.rs` runs both shims with stub
  builders.
- **The mkinitcpio run hook** of the Rust runtime starts `zbm` from busybox's shell; only the legacy runtime's entry
  point needs Bash (decision 3).
- **Golden plans** (decision 4): `crates/zbm-image/src/snapshots/` holds the plans of five configurations, dracut and
  mkinitcpio, legacy and Rust, with LUKS, GnuPG, graphics firmware, keyboard layouts, the lock and the screen readers,
  and a release; `cargo insta review` takes a change.
- **Still to come:** the `ImageBackend` trait, with the native backend (SB12, decision 5), which is when a second
  backend gives it a reason; SB7, the Rust plan without the legacy payload; signing and the SBOM (SE12); a VM test
  with mkinitcpio.

## Answers of the maintainer

Accepted as proposed, on 29 October 2026:

1. **The manifest format:** tab-separated lines (`file`, `link`, `dir`, `module`, `firmware`, `binary`), which the
   shims read with `while IFS=$'\t' read -r`.
2. **The legacy runtime's plan:** it goes with the legacy runtime (RL1, RL2); until then both plans are computed and
   tested.
3. **Container images:** the build container keeps dracut until the native backend passes the VM suites, then switches
   to it, which drops dracut from the container.
