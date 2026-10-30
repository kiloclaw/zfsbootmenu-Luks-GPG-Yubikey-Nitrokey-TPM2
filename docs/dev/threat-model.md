# Threat model

ZFSBootMenu runs before the operating system, holds the keys of encrypted volumes, and reads data that someone other
than its user may control. The rules about secrets in `crates/DESIGN.md` say how keys are handled; this page says
against whom, which inputs are hostile, what must hold, and what shows that it does. Every phase of
ROADMAP.md keeps it current: a change that adds an input, a secret or a privileged action adds a row here.

The page describes the Rust runtime and the host tools (`generate-zbm`, `zbm-kcl`). Where the legacy runtime differs,
it says so; the legacy runtime is frozen and is removed in R9.

- [Zones and data flows](#zones-and-data-flows)
- [Attackers](#attackers)
- [Assets](#assets)
- [Untrusted input](#untrusted-input)
- [What must hold](#what-must-hold)
- [Risks that stay](#risks-that-stay)
- [Out of scope](#out-of-scope)
- [Fuzz targets](#fuzz-targets)

## Zones and data flows

Today every process of the runtime runs as root in one zone: the supervisor (process 1), the start-up program, the
menu, the console on the display, the key methods and the recovery shell can all reach every secret and every device.
The arrows are the data that cross into it or out of it, with the attackers who control them and the assets they carry.

```text
             A1: ESP, image, EFI variables             A6: network (SSH, Tailscale, Tang)
                         |                                         |
                         v                                         v
  firmware --> ZFSBootMenu image (kernel, initramfs) -----+   SSH server (dropbear, the user's choice)
               I1 command line, I2 zfsbootmenu.conf       |        | key presses, a terminal (I10)
                                                          v        v
  +-------------------------- the runtime: one zone, all root --------------------------------------+
  |  supervisor (pid 1) -- start-up -- menu / console on the display -- recovery shell, hooks (I6)  |
  |                                                                                                 |
  |  in:  key presses (A2 keyboards: I10)      crypttab, key files (A2: I7)   pools, names (A3: I3) |
  |       LUKS headers, tokens (A1, A2: I8)    program output (I9)            EDID, fonts (I13)     |
  |  secrets S1-S3 --stdin pipes--> cryptsetup, tpm2_*, fido2-*, gpg, clevis --> kernel keys (S4)   |
  |  out: /dev/kmsg, journal, crash reports --> ESP when saved (S7, A1 reads them)                  |
  +---------------------------------------+---------------------------------------------------------+
                                          | kexec: kernel, initramfs, command line (S5)
                                          v
                              booted system (A4 as root, A5 without)
```

ROADMAP.md ("Processes and trust zones") splits this zone in four, Z0 to Z3: the supervisor, `zbm-unlock` (the only
process with secrets), the session, and the front ends, each with the least it needs.

## Attackers

| Id | Attacker | Can |
| --- | --- | --- |
| A1 | Someone with physical access to the powered-off machine ("evil maid") | Change the ESP and the ZFSBootMenu image on it, the firmware's boot entries and EFI variables, and the unencrypted parts of every disk |
| A2 | A malicious USB device | Claim to be a key device (a file system with a key file or a crypttab), a keyboard, a FIDO2 or smart card, or a disk with a ZFS pool |
| A3 | Whoever controls the contents of a pool the user imports | Choose pool, dataset and snapshot names, `org.zfsbootmenu:*` properties, `keylocation`, `bootfs`, the files in a boot environment's `/boot`, and the hooks below a hook root |
| A4 | A process in the booted system | Read what ZFSBootMenu left behind: files, memory it did not wipe, mappings and keys it left loaded, logs |
| A5 | A remote user of the booted system without root | Everything A4 can, without root |
| A6 | Someone on the network, when the image runs an SSH server (`dracut-crypt-ssh`, dropbear) or joins a tailnet (`mkinitcpio-tailscale`) for remote unlocking | Reach the SSH server and try logins; read and change what crosses the network in the clear (DHCP, and clevis's requests to a Tang server), or answer as either; with a key the user authorised, use the menu and the recovery shell as root, as at the console |
| A7 | Whoever can change what goes into a build or an image: a crate on crates.io, a package of the build container's distribution, its base image, a GitHub Action | Put code into `zbm`, into the programs an image carries, or into the CI that builds releases |

## Assets

| Id | Asset |
| --- | --- |
| S1 | Passphrases and PINs the user types |
| S2 | Key files: in the image, on key devices, cached from `org.zfsbootmenu:keysource` |
| S3 | Secrets derived or unsealed at boot: TPM2 and FIDO2 secrets, YubiKey responses, GnuPG-decrypted keys |
| S4 | The keys of open LUKS mappings and loaded ZFS encryption keys, in the kernel |
| S5 | The integrity of what `kexec` starts: kernel, initramfs, command line, device tree |
| S6 | The machine staying usable: every failure ends in a message and a recovery shell, never in a hang |
| S7 | The logs and reports saved on the ESP (`zbm.save_logs`, `zbm report`): the kernel log, the journal, crash reports and a summary of the machine, which A1 and whoever mounts the ESP in the booted system can read |

## Untrusted input

Each input names the attackers who control it and the code that parses it. "Parser" means code that turns text or
bytes into values; each one in this table must fail with an error on hostile input, never panic or hang (P3).

| Id | Input | Controlled by | Parsed by |
| --- | --- | --- | --- |
| I1 | The kernel command line of ZFSBootMenu itself (`/proc/cmdline`, `zbm.*` options, `cmdline.d`) | A1 | `zbm_config` (`tokenize`, `ZbmArgs`, `ZbmOptions::parse`), `zbm_types` |
| I2 | `/etc/zfsbootmenu.conf` in the image | A1 | `zbm_config::conf::Conf::parse` |
| I3 | Pool, dataset and snapshot names | A3 | `zbm_types` (`PoolName`, `DatasetName`), `zbm_zfs::parse_table` |
| I4 | `org.zfsbootmenu:commandline`, `:rootprefix`, `:kernel`, `:devicetree`, `:keysource`, `:active`, `:readonly`, `bootfs`, `keylocation` | A3 | `zbm_zfs`, `zbm_boot::commandline`, `zbm_boot::kernels`, `zbm_types::KernelSelector` (`:kernel`), `zbm_types::KernelArg` (`:rootprefix`) |
| I5 | Files in a boot environment's `/boot` and its `os-release` | A3 | `zbm_boot::kernels` (names, version order), `zbm_boot::commandline` (`os-release`) |
| I6 | Hooks below `zbm.hookroot` | A1 (names the root), A3 (fills it) | `zbm_boot::hooks` (they are run, not parsed); in Rust images, brush (`zbm-brush`) parses the hooks written for Bash or `sh` |
| I7 | crypttab and key files on removable devices | A2 | `zbm_luks::config::load_config`, `zbm_luks::keyfile`; mapping names become `zbm_types::MappingName` before `cryptsetup` sees them |
| I8 | LUKS headers: `cryptsetup luksDump` text and JSON metadata, token JSON, the UUID of the default name `luks-<UUID>` | A1, A2 | `zbm_luks::tokens` (`parse_json_metadata`, `parse_dump_text`, `json_str`, `json_arr`, `json_bool`); `zbm_luks::device::default_name`, which parses the name as a `MappingName` |
| I9 | Output of the programs the runtime runs (`zpool`, `zfs`, `blkid`, `cryptsetup`, `tpm2_*`, `fido2-*`, `gpg`, `ykchalresp`) | A2, A3 through the data they print | the adapters of each program; `zbm_pool::hostid::scrape_hostid` |
| I10 | Key presses on the console, or in an SSH session | A2 (a fake keyboard), A6 (after a login) | the menu (`zbm-tui`), prompts; in the recovery shell, brush's line editor (reedline, or its basic editor on serial lines) |
| I11 | The state files of `/zfsbootmenu` (`luks-mappings`, `degraded`, command line caches) | Only ZFSBootMenu itself writes them; listed because hooks (I6) can | `zbm_luks::mappings::parse_mappings`, `zbm_state` |
| I12 | EFI images that `zbm-kcl` edits, and `generate-zbm`'s configuration | The host's root user | `zbm-kcl`, `generate-zbm` (host tools: trusted input, but they must not crash on a broken file) |
| I13 | The EDID of connected displays (`/sys/class/drm/*/edid`) and the headers of the image's console fonts | A2 (a display or adapter plugged in), A1 (the fonts) | `zbm_startup::autosize` (`edid::parse`, `font::cell`, `choice`), `zbm_display::psf` (`Font::parse`, `Font::subset`, at image build) |
| I14 | What programs write to the console on the display (`zbm.console=drm`), escape sequences included | A3 (names and files that programs print), A1 | `zbm_console::screen::Console` (`feed`, Alacritty's parser, and `draw`) |
| I15 | The directories and files on the ESP where logs and reports are saved (`EFI/zfsbootmenu/logs`, `reports`) | A1 | `zbm_boot::saved_logs` (`list`, and the pruning that keeps the newest boots): names are compared as text, never parsed into commands or paths outside that directory |
| I16 | What crosses the network for clevis's Tang pin, and DHCP | A6 | `clevis` and `curl` (programs; their output is I9), the initramfs's network modules |
| I17 | Messages on the session's socket, `/run/zfsbootmenu/session.sock`, and the console's control socket, `/run/zfsbootmenu/console.sock` (`SOCK_SEQPACKET`, at most 64 KiB a message) | Root only: the directory is root's, and each socket refuses a peer the kernel names as another user (`SO_PEERCRED`); A6 once logged in as root | `zbm_app::session` (`Request::decode`, `Answer::decode`) and `zbm_app::console` (`Request::decode`, `Reply::decode`; a layout's name is a word of the image's list, never a path), JSON through `serde_json` |
| I18 | Messages on the client channel between the console on the display and the menu (ADR 0004, UI6): a socket pair the supervisor makes, its ends passed to the console (`--cells-fd`) and to the start-up program (`ZBM_CELLS_FD`), never to the recovery shell (`SOCK_SEQPACKET`, at most 64 KiB a message): cells and the cursor from the menu, keys and sizes from the console | ZFSBootMenu's own processes; a program the start-up program starts inherits nothing (the descriptor is closed on `exec`) | `zbm_display::channel::wire` (`FromClient::decode`, `FromConsole::decode`: exactly one message, at most 4096 cells of 64 bytes each, a cell's text cut at a character); the console keeps the cells inside its grid and shows control characters as spaces (`zbm_console::screen::cells`) |

## What must hold

Each property names the attackers and assets it is about and what shows that it holds. "Holds" means a test, fuzz
target or review shows it; "to verify" means it is believed to hold and the evidence is still owed, by the phase named.

| Id | Property | Against | Evidence | Status |
| --- | --- | --- | --- | --- |
| P1 | No secret reaches a command line, an environment variable, a file or a log: secrets go to programs on standard input and come back as `Secret`, which is wiped when dropped and prints as `Secret(..)` | A4, A5 on S1 to S3 | `crates/DESIGN.md` (Secrets); the `Secret` type; `FakeSystem::assert_no_leak`, which the key method tests end with; the journal (`/zfsbootmenu/journal.jsonl`) writes only the fields of an allow-list (`zbm_trace::KEPT_FIELDS`), any other as `[redacted]`; `Secret` is not `Clone` (copies are `duplicate()`, commands and actions share one behind an `Arc`) and compares in constant time (`ct_eq`); key files cached from key sources are written new with mode 0600 (`O_CREAT \| O_EXCL`) and removed before `kexec` | Holds for the Rust runtime. The legacy runtime writes GnuPG-decrypted keys and unsealed TPM2 and FIDO2 secrets to files below `/run` and passes a salted TPM2 PIN to `openssl kdf` as an argument (`legacy/RUNTIME-CHANGES.md`) |
| P2 | No secret outlives its use in ZFSBootMenu's memory: an unlocking run is a process of its own and drops its secrets when it ends; nothing secret is left in `/zfsbootmenu` | A4 on S1 to S3 | The process design (`crates/DESIGN.md`); the Rust key methods capture into `Secret` ; `Secret` keeps them in locked pages, out of core dumps and child processes, and out of the kernel's map of memory where `memfd_secret` is enabled; the runtime's processes are undumpable; the VM tests `secrets-passphrase` and `secrets-keyfile` search every process's memory, command line and environment, and every file in memory, just before `kexec` | Holds for secrets the runtime creates: the VM tests found neither secret anywhere (October 2026). Key files the user provides (S2) stay files by design |
| P3 | Parsing hostile input fails with an error, never a panic or a hang | A1 to A3 on S6 | Property tests (`zbm-types`); the fuzz targets below; the no-panic lints of ROADMAP.md, denied per crate as phases finish | Partly: the fuzz targets cover I1, I3, I7, I8, I9, I11, I13, I14, I17 and I18, and found one panic, now fixed (below); I4 and I5 parsers are covered when R3 moves them into domain crates |
| P4 | A pool property cannot add arguments to the kernel command line except through `org.zfsbootmenu:commandline`, the one documented property; `:rootprefix` sets only the `root=` prefix and `:devicetree` only a file below the boot environment's `/boot` | A3 on S5 | Parity tests of `zbm_boot::commandline` against the legacy code (recorded in `tests/golden/legacy/zbm-boot/commandline/`); in the Rust runtime the `root=` argument is one `zbm_types::KernelArg`, and a `:rootprefix` that does not start one argument is ignored (`a_root_prefix_cannot_add_kernel_arguments`, bug 27 of ROADMAP.md) | Holds for `:rootprefix` and `:devicetree`; to verify (R3): a property test that no other property's value appears as an argument |
| P5 | Values read from untrusted input are passed to programs as single arguments, never through a shell | A2, A3 on S5, S6 | `Command` takes arguments one by one; its only shell form, `Command::shell`, is used by tests alone. Hooks and the recovery shell are programs started by path | To verify (R2): an `xtask` check that `Command::shell` stays out of non-test code |
| P6 | Every failure ends in a message and a recovery shell, never in a hang; every wait has a time limit or a way out by key | A1 to A3 on S6 | The supervisor and start-up stage tests; VM tests of failing pools, devices and keys; the adapters give every program that waits on a device, a daemon or the network a time-out (SB1), and captured output a limit | Holds for the paths the VM tests cover; `zfs`, `zpool` and ZFS mounts have no time-out, as their time grows with the pool |
| P7 | The menu and prompts read only the console; a key press cannot run a command that the help screen does not list | A2 on S5, S6 | The key map is one table (`zbm-tui`'s `KEYMAP`) that input handling and the help screen both use | Holds for the pilot menu; R5 carries it to every screen |
| P8 | Host tools never write outside the paths their configuration names, and never leave a half-written image in place of a good one | Host misconfiguration on S5 | `generate-zbm` parity tests (files left behind are compared) | Holds where the parity tests reach |
| P9 | An unsealed TPM2 key does not cross the bus of a discrete TPM in the clear | A1, with a device on the TPM's bus | The native unlocking of systemd-tpm2 tokens salts its policy session with the storage root key and turns on response encryption (`zbm_crypt::tpm2::Tpm2::policy_session`: `tpm2_startauthsession -c`, `tpm2_sessionconfig --enable-encrypt`), and refuses to unseal over a session that cannot; the device suite checks both on swtpm | Holds for the Rust runtime's native unlocking. systemd's token plugin salts its own sessions; clevis's TPM2 pin and the legacy runtime do not |
| P10 | What is saved on the ESP holds no secret, and what is read back from it is untrusted | A1, A4, A5 on S1 to S3, S7 | P1's evidence: the journal keeps only the fields of an allow-list and crash reports redact the rest; the kernel log holds only what the runtime logs, which P1 keeps free of secrets; the VM test `runtime-rust-saved` checks what a save writes; I15 is handled as names alone | Holds for the Rust runtime |
| P11 | Remote access adds no way in that the console does not have: ZFSBootMenu itself listens on no port, and the SSH server is one the user adds, with keys the user chooses | A6 on S1, S5, S6 | No crate of the runtime opens a network socket (no `std::net` outside the VM test harness); the two Unix sockets, the session's and the console's (I17), are root's and ask what a root login could ask with a signal or a file before (the menu to end, the display handed back, a layout or a font's size); `docs/general/remote-access.rst` | Holds; the menu over SSH takes the session's menu over by its socket (ROADMAP.md PM10), and SE9's shell gate applies to it |
| P12 | What builds and fills an image is pinned and checked | A7 on S5 | Crates are vendored and locked (`vendor/`, `Cargo.lock`), their licences and advisories checked (`cargo deny`); CI names every action by its commit and the build container's base image by its digest; no release build turns on a test feature (`cargo xtask lint`, "test features"); `zbm` links no library but glibc (`cargo xtask check-links`, CD1) | Partly: audits of the crates (`cargo vet`, SE12) are to come, and the distribution packages the build container installs are not pinned |
| P13 | Text from outside ZFSBootMenu reaches no terminal raw, and a terminal's replies type nothing: names, files and program output cannot move the cursor, change what is shown around them, set a title or answer for the user | A2, A3 on S6 | `zbm_types::Untrusted` (no `Display`) holds the menu's text from pools, files, crypttab and programs, and `Printable` shows it with C0, C1, DEL and the bidi controls escaped (a property test checks that nothing it writes is one); prompts escape device labels and kernel paths, and the recovery shell the errors it reads back; the menu's key decoder drops the string sequences terminals reply with (`OSC`, `DCS`); fuzz targets `menu_names` and `console_replies` | Holds for the menu, the prompts, the recovery shell, `zbm debug` (journal, commands, state files, saved logs) and `zluks status`; messages that a format string builds and colours (`colorize`) are escaped where machine text goes into them, not as a whole |

## Risks that stay

These follow from features users rely on. They are documented so that users can choose, and so that a change does not
make them worse without saying so.

| Id | Risk | Why it stays | What a user can do |
| --- | --- | --- | --- |
| R1 | Hooks below `zbm.hookroot` run as root. Whoever controls the kernel command line (A1) or the dataset it names (A3) runs code in ZFSBootMenu | Hooks are the extension point of ZFSBootMenu | Sign the image with Secure Boot (A1); keep the hook root on a pool only you write; a foreign pool with the same name is imported like any other (to review in R3: refuse a hook root on a pool imported with an assumed host ID) |
| R2 | A disk plugged into the machine (A2, A3) is imported, and its `bootfs` can become the default boot environment when no pool is preferred | Importing every pool is the default, so a moved disk boots | `zbm.prefer=pool!!` imports that pool and no other; `zbm.prefer=pool!` requires it but imports others after it. An import policy that refuses foreign host IDs (`zbm.import_policy=strict`) keeps pools of other machines out |
| R3 | An image on the ESP is not authenticated by ZFSBootMenu (A1) | The firmware decides what it starts | Secure Boot with your own keys; `generate-zbm` signs images through its post hooks (`PostHooksDir`) |
| R4 | After `kexec`, the memory of ZFSBootMenu's kernel is not wiped; the new kernel hands only zeroed pages to its processes, but root in the booted system can read physical memory where the kernel allows it | `kexec` replaces the kernel without a reset | Boot environments with `CONFIG_STRICT_DEVMEM` and lockdown; `init_on_free=1` on ZFSBootMenu's command line, so that its kernel zeroes the pages it frees (`zbm report` says whether it does); P2 keeps secrets the runtime created out of that memory as far as it can, and the runtime drops the page cache before `kexec`. Buffers the kernel still holds at `kexec` (terminal and input buffers it reuses) are not wiped |
| R5 | `Ctrl-R` in the unlock dialog shows the passphrase being typed (proposed in `docs/dev/menu-design/unlocking.md`), so whoever sees the screen, or a camera, reads it (S1) | A passphrase set with another keyboard layout cannot be typed blind; seeing it is how the user finds the layout that works | It is off at every prompt and never on by default; the text is drawn only while the dialog is open. Turn it on only at a console nobody watches |
| R6 | Sealing a TPM2 key again (the re-seal alert, ROADMAP.md #11) trusts the machine as it is when the user confirms. If A1 changed the firmware settings or the boot path, the TPM then releases the key to that state (S3) | After a firmware update the user would otherwise type the passphrase at every boot | The alert lists each PCR that changed and what the event log says about it, and is for changes the user made; it needs the volume's name typed. Decline it when a change is unexplained, and keep using the passphrase |
| R7 | The shell of Rust images is brush, younger than Bash and less used. A script that behaves differently on it, or a bug in its parser, can make a hook do something other than what its author meant (S6) | brush is the shell of the Rust runtime (`ROADMAP.md`, decisions); the hooks are code the user already trusts (R1) | `zbm.shell=bash` runs everything on Bash; a hook can name another interpreter on its `#!` line; the recovery shell falls back to Bash when brush does not open |
| R8 | Whoever logs in over SSH (A6, with a key the user authorised) is root in ZFSBootMenu: the menu, the recovery shell and every pool, as at the console | Remote recovery is what the SSH server is for | Restrict the key in `authorized_keys` (`command=`, `from=`); keep the SSH server out of images that do not need it; SE9's locked menu and shell gate will put a PIN before the shell |

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
| `kernel_cmdline` | `KernelCommandLine::lenient`, `cmdline_files`, `ZbmArgs::new`, `ZbmOptions::parse`, `Conf::parse` | I1, I2 | A1 |
| `domain_names` | `zbm_types`: `DevicePath`, `MappingName`, `PoolName`, `DatasetName`, `SnapshotName`, `FilesystemName`, `KernelArg`, `KernelCommandLine`, `KernelVersion`, `ImportPolicy`, `Stage`, `HookStage`, the values of the options (`Timeout`, `RetryDelay`, `LogLevel`, `HostId`, `PoolRequirement`, `BootfsPreference`), and the version order | I1, I3, I5, I7 | A1, A2, A3 |
| `kernel_selector` | `zbm_types::KernelSelector`: the translation of `org.zfsbootmenu:kernel`, its compilation and matches | I4 | A3 |
| `crypttab` | `zbm_luks::config::load_config` | I1, I7 | A1, A2 |
| `luks_metadata` | `zbm_luks::tokens`: `parse_json_metadata`, `parse_dump_text`, `json_str`, `json_arr`, `json_bool` | I8 | A1, A2 |
| `program_output` | `zbm_zfs::parse_table`, `zbm_pool::hostid::scrape_hostid`, `zbm_luks::mappings::parse_mappings` | I9, I11 | A2, A3 |
| `display_data` | `zbm_startup::autosize`: `edid::parse`, `font::cell`, `Density::new`, `choice::order`; `zbm_display::psf`: `Font::parse`, `Font::subset`, `Font::to_bytes` | I13 | A1, A2 |
| `console_output` | `zbm_console::screen::Console`: `feed` in reads of any size, `draw` (scrolling and the cursor included), `tick`, `take_replies`, `text` | I14 | A1, A3 |
| `boot_files` | `zbm_boot::kernels`: `discover` (kernel names, their initramfs images and `.kcl` files, version order) and `select`; `zbm_boot::commandline`: `os_release`, `root_prefix` | I5 | A3 |
| `menu_names` | `zbm_tui::menu::view` over a snapshot whose text fields are hostile, on every screen, at three sizes; checks P13: no cell holds a character that acts on a terminal | I3, I5, I8, I9 | A2, A3 |
| `key_input` | `zbm_tui::menu::term_keys::decode`, in reads of any size against all at once; `zbm_display::input::parse_events` and `keys::Decoder` (`feed`, `resync`); `zbm_display::layout`: `Layout::from_keymap`, `compile::dead_keys`, `keys::compose_key` | I10 | A1, A2, A6 |
| `console_replies` | What `zbm_console::screen::Console` answers to any output (`take_replies`), read back by `term_keys::decode`; checks P13: no key | I14 | A3 |
| `messages` | What ZBM's processes leave each other: `zbm_trace::crash::Report::parse`, `zbm_platform::dmesg::records`, the state files through `zbm_state::State` (stages, the active menu, `bootfs`, degraded pools, the command line override, the console's process, the console that handed the display back, its layout), `Zoom::parse`, `compile::split`, `parse_size`, `ZbmArgs::from_cmdline`; the session's messages, `zbm_app::session::Request::decode` and `Answer::decode`, and the console's, `zbm_app::console::Request::decode` and `Reply::decode`; the client channel's, `zbm_display::channel::FromClient::decode` and `FromConsole::decode`, each encoding back to the bytes it came from | I11, I17, I18 | A1 (through hooks), A6 |

A parser that a phase adds or moves gets a target, or joins one, in the same pull request, and this table says which.

Findings so far:

- `program_output`, on its first run: `zbm_pool::hostid::scrape_hostid` sliced the output of `zpool import` at byte
  positions, and panicked when text that is not ASCII came before `hostid=`. A pool someone else controls (A3) could
  stop the start-up program with it. Fixed, with the input as a unit test.
- `console_replies`, by review while it was written (SE5): the console on the display answers a program that asks
  for its colours (`OSC 10`, `11` and `4`), and the menu's key decoder read the answer as Alt+`]` and the letters of
  the colour. A file of a pool shown on the console (A3) could type into the menu. The decoder now drops the string
  sequences terminals reply with, and a C1 control typed as a character; unit tests in `term_keys` and `zbm`.
- `key_input`, on its first runs (SE5): the menu's terminal key decoder lost or doubled Enter at the edges of broken
  input. A stray byte that starts a UTF-8 character, `ESC O` or a control sequence cut short took the Enter after it
  as part of themselves; and the reader, which joins CR LF cut between two reads, kept dropping line feeds after the
  first. Over SSH or a serial line, that could skip or confirm a dialog. Fixed: a broken character or sequence ends
  at the first byte that cannot belong to it, and only the byte right after the carriage return of an Enter is
  joined to it; the inputs are unit tests in `term_keys`.
