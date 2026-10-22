# Threat model

ZFSBootMenu runs before the operating system, holds the keys of encrypted volumes, and reads data that someone other
than its user may control. The rules about secrets in `crates/DESIGN.md` say how keys are handled; this page says
against whom, which inputs are hostile, what must hold, and what shows that it does. Every phase of
REFACTOR-ROADMAP.md keeps it current: a change that adds an input, a secret or a privileged action adds a row here.

The page describes the Rust runtime and the host tools (`generate-zbm`, `zbm-kcl`). Where the legacy runtime differs,
it says so; the legacy runtime is frozen and is removed in R9.

- [Attackers](#attackers)
- [Assets](#assets)
- [Untrusted input](#untrusted-input)
- [What must hold](#what-must-hold)
- [Risks that stay](#risks-that-stay)
- [Out of scope](#out-of-scope)
- [Fuzz targets](#fuzz-targets)

## Attackers

| Id | Attacker | Can |
| --- | --- | --- |
| A1 | Someone with physical access to the powered-off machine ("evil maid") | Change the ESP and the ZFSBootMenu image on it, the firmware's boot entries and EFI variables, and the unencrypted parts of every disk |
| A2 | A malicious USB device | Claim to be a key device (a file system with a key file or a crypttab), a keyboard, a FIDO2 or smart card, or a disk with a ZFS pool |
| A3 | Whoever controls the contents of a pool the user imports | Choose pool, dataset and snapshot names, `org.zfsbootmenu:*` properties, `keylocation`, `bootfs`, the files in a boot environment's `/boot`, and the hooks below a hook root |
| A4 | A process in the booted system | Read what ZFSBootMenu left behind: files, memory it did not wipe, mappings and keys it left loaded, logs |
| A5 | A remote user of the booted system without root | Everything A4 can, without root |

## Assets

| Id | Asset |
| --- | --- |
| S1 | Passphrases and PINs the user types |
| S2 | Key files: in the image, on key devices, cached from `org.zfsbootmenu:keysource` |
| S3 | Secrets derived or unsealed at boot: TPM2 and FIDO2 secrets, YubiKey responses, GnuPG-decrypted keys |
| S4 | The keys of open LUKS mappings and loaded ZFS encryption keys, in the kernel |
| S5 | The integrity of what `kexec` starts: kernel, initramfs, command line, device tree |
| S6 | The machine staying usable: every failure ends in a message and a recovery shell, never in a hang |

## Untrusted input

Each input names the attackers who control it and the code that parses it. "Parser" means code that turns text or
bytes into values; each one in this table must fail with an error on hostile input, never panic or hang (P3).

| Id | Input | Controlled by | Parsed by |
| --- | --- | --- | --- |
| I1 | The kernel command line of ZFSBootMenu itself (`/proc/cmdline`, `zbm.*` options, `cmdline.d`) | A1 | `zbm_cmdline` (`tokenize`, `ZbmArgs`, `ZbmOptions::parse`), `zbm_types` |
| I2 | `/etc/zfsbootmenu.conf` in the image | A1 | `zbm_cmdline::conf::Conf::parse` |
| I3 | Pool, dataset and snapshot names | A3 | `zbm_types` (`PoolName`, `DatasetName`), `zbm_zfs::parse_table` |
| I4 | `org.zfsbootmenu:commandline`, `:rootprefix`, `:kernel`, `:devicetree`, `:keysource`, `:active`, `:readonly`, `bootfs`, `keylocation` | A3 | `zbm_zfs`, `zbm_boot::commandline`, `zbm_boot::kernels` |
| I5 | Files in a boot environment's `/boot` and its `os-release` | A3 | `zbm_boot::kernels` (names, version order), `zbm_boot::commandline` (`os-release`) |
| I6 | Hooks below `zbm.hookroot` | A1 (names the root), A3 (fills it) | `zbm_boot::hooks` (they are run, not parsed) |
| I7 | crypttab and key files on removable devices | A2 | `zbm_luks::config::load_config`, `zbm_luks::keyfile` |
| I8 | LUKS headers: `cryptsetup luksDump` text and JSON metadata, token JSON | A1, A2 | `zbm_luks::tokens` (`parse_json_metadata`, `parse_dump_text`, `json_str`, `json_arr`, `json_bool`) |
| I9 | Output of the programs the runtime runs (`zpool`, `zfs`, `blkid`, `cryptsetup`, `tpm2_*`, `fido2-*`, `gpg`, `ykchalresp`) | A2, A3 through the data they print | the adapters of each program; `zbm_boot::import::scrape_hostid` |
| I10 | Key presses on the console | A2 (a fake keyboard) | the menu (`zbm-tui`), prompts |
| I11 | The state files of `/zfsbootmenu` (`luks-mappings`, `degraded`, command line caches) | Only ZFSBootMenu itself writes them; listed because hooks (I6) can | `zbm_luks::mappings::parse_mappings`, `zbm_state` |
| I12 | EFI images that `zbm-kcl` edits, and `generate-zbm`'s configuration | The host's root user | `zbm-kcl`, `generate-zbm` (host tools: trusted input, but they must not crash on a broken file) |

## What must hold

Each property names the attackers and assets it is about and what shows that it holds. "Holds" means a test, fuzz
target or review shows it; "to verify" means it is believed to hold and the evidence is still owed, by the phase named.

| Id | Property | Against | Evidence | Status |
| --- | --- | --- | --- | --- |
| P1 | No secret reaches a command line, an environment variable, a file or a log: secrets go to programs on standard input and come back as `Secret`, which is wiped when dropped and prints as `Secret(..)` | A4, A5 on S1 to S3 | `crates/DESIGN.md` (Secrets); the `Secret` type; `FakeSystem::assert_no_leak`, which the key method tests end with | Holds for the Rust runtime. The legacy runtime writes GnuPG-decrypted keys and unsealed TPM2 and FIDO2 secrets to files below `/run` and passes a salted TPM2 PIN to `openssl kdf` as an argument (`legacy/RUNTIME-CHANGES.md`) |
| P2 | No secret outlives its use in ZFSBootMenu's memory: an unlocking run is a process of its own and drops its secrets when it ends; nothing secret is left in `/zfsbootmenu` | A4 on S1 to S3 | The process design (`crates/DESIGN.md`); the Rust key methods capture into `Secret` | Holds for secrets the runtime creates. Key files the user provides (S2) stay files by design |
| P3 | Parsing hostile input fails with an error, never a panic or a hang | A1 to A3 on S6 | Property tests (`zbm-types`); the fuzz targets below; the no-panic lints of REFACTOR-ROADMAP.md, denied per crate as phases finish | Partly: the fuzz targets cover I1, I3, I7, I8, I9 and I11, and found one panic, now fixed (below); I4 and I5 parsers are covered when R3 moves them into domain crates |
| P4 | A pool property cannot add arguments to the kernel command line except through `org.zfsbootmenu:commandline`, the one documented property; `:rootprefix` sets only the `root=` prefix and `:devicetree` only a file below the boot environment's `/boot` | A3 on S5 | Parity tests of `zbm_boot::commandline` against the legacy code (recorded in `tests/golden/legacy/zbm-boot/commandline/`) | To verify (R3): a property test that no other property's value appears as an argument |
| P5 | Values read from untrusted input are passed to programs as single arguments, never through a shell | A2, A3 on S5, S6 | `Command` takes arguments one by one; its only shell form, `Command::shell`, is used by tests alone. Hooks and the recovery shell are programs started by path | To verify (R2): an `xtask` check that `Command::shell` stays out of non-test code |
| P6 | Every failure ends in a message and a recovery shell, never in a hang; every wait has a time limit or a way out by key | A1 to A3 on S6 | The supervisor and start-up stage tests; VM tests of failing pools, devices and keys | Holds for the paths the VM tests cover |
| P7 | The menu and prompts read only the console; a key press cannot run a command that the help screen does not list | A2 on S5, S6 | The key map is one table (`zbm-tui`'s `KEYMAP`) that input handling and the help screen both use | Holds for the pilot menu; R5 carries it to every screen |
| P8 | Host tools never write outside the paths their configuration names, and never leave a half-written image in place of a good one | Host misconfiguration on S5 | `generate-zbm` parity tests (files left behind are compared) | Holds where the parity tests reach |

## Risks that stay

These follow from features users rely on. They are documented so that users can choose, and so that a change does not
make them worse without saying so.

| Id | Risk | Why it stays | What a user can do |
| --- | --- | --- | --- |
| R1 | Hooks below `zbm.hookroot` run as root. Whoever controls the kernel command line (A1) or the dataset it names (A3) runs code in ZFSBootMenu | Hooks are the extension point of ZFSBootMenu | Sign the image with Secure Boot (A1); keep the hook root on a pool only you write; a foreign pool with the same name is imported like any other (to review in R3: refuse a hook root on a pool imported with an assumed host ID) |
| R2 | A disk plugged into the machine (A2, A3) is imported, and its `bootfs` can become the default boot environment when no pool is preferred | Importing every pool is the default, so a moved disk boots | `zbm.prefer=pool!!` imports that pool and no other; `zbm.prefer=pool!` requires it but imports others after it. An import policy that refuses foreign host IDs (`zbm.import_policy=strict`) keeps pools of other machines out |
| R3 | An image on the ESP is not authenticated by ZFSBootMenu (A1) | The firmware decides what it starts | Secure Boot with your own keys; `generate-zbm` signs images through its post hooks (`PostHooksDir`) |
| R4 | After `kexec`, the memory of ZFSBootMenu's kernel is not wiped; the new kernel hands only zeroed pages to its processes, but root in the booted system can read physical memory where the kernel allows it | `kexec` replaces the kernel without a reset | Boot environments with `CONFIG_STRICT_DEVMEM` and lockdown; P2 keeps secrets the runtime created out of that memory as far as it can |

## Out of scope

- Compromised firmware, or an attacker who can run code as root before ZFSBootMenu starts.
- Attacks on the cryptography of LUKS, ZFS native encryption, TPM2, FIDO2 or GnuPG themselves, and on the hardware
  tokens.
- Side channels (timing, power, electromagnetic) and cold-boot attacks on RAM.
- Denial of service by an attacker with physical access, who can always switch the machine off.
- The booted operating system's own security once `kexec` has started it, except what ZFSBootMenu leaves behind (A4).

## Fuzz targets

The targets are in `fuzz/` (`cargo fuzz`, nightly Rust; `docs/dev/testing.md` says how to run them). Each feeds
arbitrary bytes to the parsers of an input above and checks P3: no panic, no hang. CI runs each for a few minutes every
night.

| Target | Parsers | Inputs | Threat |
| --- | --- | --- | --- |
| `kernel_cmdline` | `zbm_cmdline::kcl::tokenize`, `ZbmArgs::from_cmdline`, `ZbmOptions::parse`, `Conf::parse` | I1, I2 | A1 |
| `domain_names` | `zbm_types`: `DevicePath`, `MappingName`, `PoolName`, `DatasetName`, `SnapshotName`, `KernelVersion`, `ImportPolicy`, `Stage`, `HookStage`, and the version order | I1, I3, I5, I7 | A1, A2, A3 |
| `crypttab` | `zbm_luks::config::load_config` | I1, I7 | A1, A2 |
| `luks_metadata` | `zbm_luks::tokens`: `parse_json_metadata`, `parse_dump_text`, `json_str`, `json_arr`, `json_bool` | I8 | A1, A2 |
| `program_output` | `zbm_zfs::parse_table`, `zbm_boot::import::scrape_hostid`, `zbm_luks::mappings::parse_mappings` | I9, I11 | A2, A3 |

A parser that a phase adds or moves gets a target, or joins one, in the same pull request, and this table says which.

Findings so far:

- `program_output`, on its first run: `zbm_boot::import::scrape_hostid` sliced the output of `zpool import` at byte
  positions, and panicked when text that is not ASCII came before `hostid=`. A pool someone else controls (A3) could
  stop the start-up program with it. Fixed, with the input as a unit test.
