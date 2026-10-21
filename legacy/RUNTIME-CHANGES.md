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
- [ ] `712bf06`: the TPM2 authorization value derived from a PIN went to `tpm2_unseal` as an argument (`+hex:...`),
  readable by any process in `/proc`. It now goes on standard input (`+file:-`) as `hex:<value>`; raw bytes do not
  work, because tpm2-tools reads the input as a string. The token suite checks `tpm2_unseal`'s arguments.
  **Rust (package B):** pass the value as `hex:<value>` with `Input::Secret`, and check with
  `FakeSystem::assert_no_leak` that it appears in no argument.
- [x] `48f9d4d`: `zfsbootmenu-preinit.sh` wrote command line values to `/etc/zfsbootmenu.conf` between single quotes
  without escaping, so a value with a single quote in it (`zbm.prefer`, `zbm.wait_for`, `zbm.hookroot`, `console`)
  broke the file. `zbm_conf_quote` now writes each quote as `'\''`.
  **Rust:** done; `ZbmOptions::conf` quotes the same way, `zbm_cmdline::conf` reads the file back, and parity tests
  compare both writers and check the values against Bash sourcing the file.

## Legacy limitations the Rust runtime must not copy

- [x] Kernels are ordered by the image's `sort -V`, which is GNU coreutils, uutils (images built in the build
  container, Ubuntu 26.10) or BusyBox (mkinitcpio). uutils 0.12.0 and GNU 9.4 differ only for names where a version
  part is `0` and another name has a dot followed by a letter at that place (`kernel-0`, `kernel-.X`).
  **Rust:** done; `zbm_boot::kernels` orders like GNU coreutils, tested against GNU `sort -V`. BusyBox is still to be
  compared (package C).

- [ ] A salted TPM2 PIN (systemd 252 and later) reaches `openssl kdf` as an argument (`-kdfopt pass:<pin>`), the only
  way `openssl kdf` accepts it, so the legacy code keeps this until it is removed.
  **Rust (package B):** compute PBKDF2-HMAC-SHA256 and SHA-256 in the process (see "Secrets" in
  [crates/DESIGN.md](../crates/DESIGN.md)).
- [ ] Decrypted GnuPG key files and unsealed TPM2 and FIDO2 secrets are kept in temporary files below `/run` (RAM), as
  `zfsbootmenu-luks(7)` documents.
  **Rust (package B):** capture them with `Stream::CaptureSecret` and pass them with `--key-file=-`, creating no
  files.
