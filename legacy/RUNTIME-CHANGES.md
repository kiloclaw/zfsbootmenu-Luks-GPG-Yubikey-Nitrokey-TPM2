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
