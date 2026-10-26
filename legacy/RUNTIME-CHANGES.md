# Changes to the frozen legacy runtime

Rule 4 of [PORT-ROADMAP.md](../PORT-ROADMAP.md): from Phase 4 on, the legacy runtime gets bug fixes only, and every
change to it must be ported to the Rust runtime before the beta (Phase 7) ends. This file is the tracking list the
rule asks for. It is kept in the repository, rather than in a GitHub issue, because the repository has issues turned
off, and so that a change to the runtime adds its entry in the same commit.

- **Frozen files:** `legacy/zfsbootmenu/`, and the dracut and mkinitcpio files in `packaging/dracut/` and
  `packaging/initcpio/` that install it.
- **Freeze point:** commit `661e9f8`. `scripts/dev/legacy-runtime-changes.sh` lists every commit since then that
  changed a frozen file; each one must have an entry below.
- **Each entry** names the commit, what changed and what the Rust runtime must do. Tick it when the Rust runtime does
  it and a test covers it. Changes merged from upstream ZFSBootMenu belong here too.

## Changes

- [x] `dc82b80`: restricted (degraded) pools were matched as substrings and patterns (`grep -q`), so an unhealthy
  `data2` made `data` read-only too. `is_degraded` in `zfsbootmenu-core.sh` now matches whole names, and the start-up
  and re-import code use it.
  **Rust:** done; `zbm_state::State::is_degraded` matches whole names, with a parity test against the legacy
  `is_degraded`.
- [x] `712bf06`: the TPM2 authorization value derived from a PIN went to `tpm2_unseal` as an argument (`+hex:...`),
  readable by any process in `/proc`. It now goes on standard input (`+file:-`) as `hex:<value>`; raw bytes do not
  work, because tpm2-tools reads the input as a string. The token suite checks `tpm2_unseal`'s arguments.
  **Rust:** done (package B); `zbm_luks::keys::tpm2` passes `hex:<value>` with `Input::Secret`, its tests check with
  `FakeSystem::assert_no_leak` that the PIN appears in no argument, and the token suite passes against Rust.
- [x] `48f9d4d`: `zfsbootmenu-preinit.sh` wrote command line values to `/etc/zfsbootmenu.conf` between single quotes
  without escaping, so a value with a single quote in it (`zbm.prefer`, `zbm.wait_for`, `zbm.hookroot`, `console`)
  broke the file. `zbm_conf_quote` now writes each quote as `'\''`.
  **Rust:** done; `ZbmOptions::conf` quotes the same way, `zbm_cmdline::conf` reads the file back, and parity tests
  compare both writers and check the values against Bash sourcing the file.
- [x] `1346d04`: the image build accepts `zfsbootmenu_runtime=rust`, which installs the Rust start-up hooks
  (`packaging/dracut/zfsbootmenu-rust-*.sh`, the mkinitcpio entry point) and leaves out the legacy start-up program and
  `init.d`; legacy images are unchanged. The online help of `zfsbootmenu(7)` describes the setting. Also a fix: dracut
  images lost `embedded_kcl`, which `module-setup.sh` appended to `/etc/zfsbootmenu.conf` before `create_zbm_conf`
  wrote the file anew.
  **Rust:** done; `zbm_startup::preinit` writes `embedded_kcl` to `/etc/cmdline.d/zfsbootmenu.conf` as the legacy parser
  does, and `crates/zbm/tests/image.rs` checks both runtimes' images.
- [x] `bd1b300`: the online help of `zfsbootmenu(7)` (`help-files/`) describes what the Rust runtime handles after
  package A; the legacy runtime is unchanged.
  **Rust:** nothing to port; documentation only.
- [x] `935f6c6`: `luks_record_mapping` compared the new mapping name with the recorded ones as a pattern, so `a.b`
  was not recorded when `axb` was, and the teardown hook left that volume open, with its key in memory. Names are now
  compared as text; `edge.sh` checks it.
  **Rust:** done; `zbm_luks::mappings` compares names as text, with a parity test against the legacy function.
- [x] `5ac0150`: images with `zfsbootmenu_runtime=rust` link `/bin/zluks` to `zbm` instead of installing the legacy
  script, and their teardown hook `80-luks-close.sh` runs `zluks close`; legacy images are unchanged. The online help
  of `zfsbootmenu(7)` and `zfsbootmenu-luks(7)` describes what the Rust runtime does with LUKS.
  **Rust:** done; `crates/zbm/tests/image.rs` checks both runtimes' images.
- [x] `c364e98`: images with `zfsbootmenu_runtime=rust` install their console fonts as 256-glyph subsets that
  `install_zbm_fonts` makes with `zbm console-fonts` (ROADMAP.md #20), so the console keeps its 16 colours; the whole
  fonts when `zbm` cannot make them. Legacy images are unchanged.
  **Rust:** done; `zbm_types::psf` makes the subsets, and `crates/zbm/tests/image.rs` checks the fonts of both
  runtimes' images.
- [x] `79624e4`: the build option `zfsbootmenu_console_font=terminus|spleen` (ROADMAP.md #20), which `generate-zbm`
  sets from `Global.ConsoleFont`. With `spleen`, `install_zbm_fonts` installs the Spleen fonts of
  `fonts/spleen/` instead of Terminus, as 256-glyph subsets, with their licence as
  `/usr/share/zfsbootmenu/fonts/LICENSE.spleen`. Legacy images, whose autosize hook loads the Terminus fonts by name,
  warn and keep Terminus, as does any other value; without the option, images are unchanged.
  **Rust:** done; `crates/zbm/tests/image.rs` checks the fonts of a Spleen image and the warnings.
- [x] `37dcb86`: the online help of `zfsbootmenu(7)` (`help-files/`) describes `zbm.console=drm`, the console on
  the display of the Rust runtime (ROADMAP.md #19); the legacy runtime is unchanged.
  **Rust:** nothing to port; documentation only.
- [x] `adc48b8`: images with `zfsbootmenu_runtime=rust` install the TrueType font of the console on the display,
  `fonts/dejavu/DejaVuSansMono.ttf`, with its licence as `/usr/share/zfsbootmenu/fonts/LICENSE.dejavu`
  (ROADMAP.md #19, step 3); the online help of `zfsbootmenu(7)` describes `zbm.console_font`. Legacy images are
  unchanged.
  **Rust:** done; `crates/zbm/tests/image.rs` checks the fonts of both runtimes' images.
- [x] `4408efb`: images with `zfsbootmenu_runtime=rust` install the keyboard layouts of the console on the display
  (ROADMAP.md #19, step 4): `install_zbm_keyboard_layouts`, which the dracut module and the mkinitcpio hook call, runs
  `zbm keyboard-layouts` on the layouts of `zfsbootmenu_keyboard_layouts` and installs them in
  `/usr/share/zfsbootmenu/layouts`; the online help of `zfsbootmenu(7)` describes `zbm.keyboard_layout` and the build
  option. Legacy images are unchanged.
  **Rust:** done; `crates/zbm/tests/image.rs` checks the layouts of Rust images and their absence from legacy ones.
- [x] `459c402`: images with `zfsbootmenu_runtime=rust` let the recovery shell's line editor take characters beyond
  ASCII: `install_zbm_shell_input`, which the dracut module and the mkinitcpio hook call, writes `/etc/inputrc` (8-bit
  input and output) and installs the build host's `C.UTF-8` character set, when it has one, with
  `LC_CTYPE=C.UTF-8` in `/etc/profile`. Legacy images are unchanged.
  **Rust:** done; `crates/zbm/tests/image.rs` checks both runtimes' images, and `runtime-rust-console` types `ê` at
  the prompt.
- [x] `6c543fe`: with `SOURCE_DATE_EPOCH` set, `install_zbm_osver` writes that day as the image's `BUILD_ID`
  instead of the build's, so that reproducible builds give the same `/etc/zbm-release` (ROADMAP.md #5). Without it,
  nothing changes.
  **Rust:** nothing to port; the image build stays in the dracut module until R6, which carries it over. The CI job
  `reproducible` checks it.
- [x] `89657e6`: the online help of `zfsbootmenu(7)` (`help-files/`) describes `zbm.save_logs`, the saved boot logs
  and `zbm report` of the Rust runtime (ROADMAP.md #9); the legacy runtime is unchanged.
  **Rust:** nothing to port; documentation only.
- [x] `1fd2a01`: the online help of `zfsbootmenu(7)` (`help-files/`) says how the Rust runtime keeps secrets in memory
  and recommends `init_on_free=1` for its images (ROADMAP.md #1); the legacy runtime is unchanged.
  **Rust:** nothing to port; documentation only.
- [x] `99652d4`: the online help of `zfsbootmenu(7)` (`help-files/`) says that the Rust runtime loads the keys of
  encrypted boot environments and runs `load-key.d` hooks (R5 step 1 of `REFACTOR-ROADMAP.md`); the legacy runtime is
  unchanged.
  **Rust:** nothing to port; documentation only.
- [x] `fcaeb71`: `install_zbm_core` (`install-helpers.sh`) installs neither `bin/zfsbootmenu` nor `bin/zbm` in images
  with the Rust runtime: `/bin/zfsbootmenu` is a link to `/libexec/zbm`, the menu of R5 step 3, and `/bin/zbm` a
  script that runs `/libexec/zbm zfsbootmenu`. Legacy images are unchanged; the image tests' recordings changed only
  for the Rust images.
  **Rust:** nothing to port; the image build stays in the dracut module until R6, which carries it over.
- [x] `12e4071`: `module-setup.sh` wrote `embedded_kcl` to `/etc/zfsbootmenu.conf` between double quotes, so `$`,
  backticks and backslashes in it were expanded when the file was sourced, and a double quote broke the file. It is now
  written between single quotes, each quote as `'\''`, and keeps every character.
  **Rust:** done; `Conf::parse` reads the quoted value (a unit test with every special character), and
  `crates/zbm/tests/image.rs` checks the line in both runtimes' images.
- [x] `12e4071`: in images with `zfsbootmenu_runtime=rust`, `/bin/zbm` runs the command it is given (`zbm debug
  crash`, `zbm report`) and the menu otherwise; legacy images are unchanged.
  **Rust:** done; `crates/zbm/tests/image.rs` runs the wrapper with and without a command.
- [x] `51e7d4a`: `install_zbm_graphics` in `install-helpers.sh`, run by the dracut module's `installkernel` and the
  mkinitcpio hook, installs the graphics drivers of `zfsbootmenu_graphics_drivers` past dracut's `omit_drivers` and
  removes the firmware `zfsbootmenu_graphics_firmware` does not name (ROADMAP.md #17). Images with the legacy runtime
  fail to build with the option set; without it, every image is unchanged.
  **Rust:** nothing to port; the image build stays in the dracut module until R6, which carries it over.
  `crates/zbm/tests/image_graphics.rs` covers the helper.
- [x] `d620216`: `install_zbm_graphics` also writes `/etc/zfsbootmenu/graphics-drivers` and
  `/etc/modprobe.d/zfsbootmenu-graphics.conf`, which keeps udev from loading the graphics drivers, since the Rust
  runtime loads them for the GPUs `zbm.gpu` chooses; it takes `virtio_gpu` and `bochs` too. Images without
  `zfsbootmenu_graphics_drivers` are unchanged.
  **Rust:** done; `zbm_startup::graphics` loads them, and `crates/zbm/tests/image_graphics.rs` and the VM test
  `runtime-rust-gpu` cover it.

## Legacy limitations the Rust runtime must not copy

- [x] Kernels are ordered by the image's `sort -V`, which is GNU coreutils, uutils (images built in the build
  container, Ubuntu 26.10) or BusyBox (mkinitcpio). uutils 0.12.0 and GNU 9.4 differ only for names where a version
  part is `0` and another name has a dot followed by a letter at that place (`kernel-0`, `kernel-.X`).
  **Rust:** done; `zbm_boot::kernels` orders like uutils, the coreutils of the images (`uucore`'s `version_cmp`),
  tested against the build container's `sort -V`; until R2 of the refactoring it ordered like GNU. BusyBox is still
  to be compared (package C).

- [x] A salted TPM2 PIN (systemd 252 and later) reaches `openssl kdf` as an argument (`-kdfopt pass:<pin>`), the only
  way `openssl kdf` accepts it, so the legacy code keeps this until it is removed.
  **Rust:** done (package B); `zbm_luks::crypto` computes PBKDF2-HMAC-SHA256 and SHA-256 in the process, checked
  against the standard test vectors and the output of `openssl kdf` and `sha256sum`.
- [x] Decrypted GnuPG key files and unsealed TPM2 and FIDO2 secrets are kept in temporary files below `/run` (RAM), as
  `zfsbootmenu-luks(7)` documents.
  **Rust:** done (package B); they are captured with `Stream::CaptureSecret` and passed with `--key-file=-`, and the
  unit suites check that nothing is left below `/run` or `/tmp`.
