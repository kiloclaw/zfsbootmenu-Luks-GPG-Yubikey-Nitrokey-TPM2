# Changelog

## Unreleased

### Repository layout

The source tree has been reorganised to prepare the port of ZFSBootMenu to Rust. Installed files and built images are
unchanged; only paths in the repository moved:

| Old path | New path |
| --- | --- |
| `zfsbootmenu/` | `legacy/zfsbootmenu/` |
| `bin/` | `legacy/bin/` |
| `cpanfile` | `legacy/cpanfile` |
| `dracut/` | `packaging/dracut/` |
| `initcpio/` | `packaging/initcpio/` |
| `releng/docker/` | `packaging/container/` |
| `zbm-builder.sh` | `packaging/container/zbm-builder.sh` |
| `install-tree.sh`, `releng/version.sh`, `releng/rst2help.sh`, `releng/update-includes.sh` | `scripts/` |
| other files in `releng/` (release tools and signing keys) | `scripts/release/` |
| `etc/` | `config/` |
| `testing/` | `tests/` |
| `media/` | `assets/` |
| `docs/CHANGELOG.md` | `CHANGELOG.md` |
| `Gentoo Linux ZFS LUKS-V2.md` (and V3, V4) | `docs/guides/gentoo/zfs-luks-v2.md` (and v3, v4) |

The build container accepts source trees with either layout, so `zbm-builder.sh -l` and `-L` still work with upstream
source trees, and the container still builds from upstream source tarballs. Container images built before this change
know only the old layout: rebuild the image (`podman build -t localhost/zbm-builder packaging/container`) before
building from a tree with the new layout.

New files: a Cargo workspace (`Cargo.toml`, `rust-toolchain.toml`, `crates/zbm-core`) with its CI workflow, and
`CONTRIBUTING.md`, `PORT-ROADMAP.md`, `ROADMAP.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `.editorconfig`, a pull
request template and a Dependabot configuration.

### Rust host tools

`generate-zbm` and `zbm-kcl` are now Rust programs (phases 1 to 3 of `PORT-ROADMAP.md`). They accept the same options
and configuration, print the same messages and build the same images as the Perl and Bash tools, which parity tests
and image comparisons check. `make` builds them offline from vendored crates, `make install` installs them, and the
build container builds images with them; Perl is no longer needed to build images. `make install TOOLS=legacy`
installs the legacy tools, and the build container's `-g perl` option uses the Perl `generate-zbm`. A build container
without `cargo`, such as one built from an older `Dockerfile`, builds with the Perl `generate-zbm` and without `zbm`,
with a warning, since the legacy runtime does not need it; `-g rust` fails there.

Images now include `zbm`, the program of the future Rust runtime, as `/libexec/zbm`, but still boot the legacy runtime.
The dracut and mkinitcpio setting `zfsbootmenu_runtime` selects the runtime (`legacy` by default) and
`zfsbootmenu_zbm` names the program to include (default `/usr/libexec/zfsbootmenu/zbm`); see `zfsbootmenu(7)`.

Fixes found while porting, in both versions of the tools:

- `generate-zbm` quotes paths in the commands it runs, so kernels, hooks, the boot mount point and the dracut and
  mkinitcpio configuration may have spaces or quotes in their paths.
- `generate-zbm` unmounts the boot partition once, not twice, when it is interrupted.
- `generate-zbm` no longer counts a command killed by a signal as a success, and no longer exits with status 0 when
  `uname -r` prints nothing for `%{current}`.
- `generate-zbm` stops with an error when `objdump` reports no section alignment or sections for the EFI stub, instead
  of building a bundle with wrong offsets, and its debug log shows the section offsets correctly.
- `generate-zbm --enable` and `--disable` report a failed write of the configuration instead of truncating it silently.
- `%{parent}` in `org.zfsbootmenu:commandline` inserts the parent's command line as it is, in the runtime and in
  `zbm-kcl`; with Bash 5.2 and later, an `&` in it was replaced. The `spl_hostid` kernel option accepts only decimal
  and hexadecimal numbers.

### Rust runtime design

`crates/DESIGN.md` describes how the Rust runtime will be built (Phase 4 of `PORT-ROADMAP.md`). The crates it lays out
exist: the interfaces through which the runtime reaches the system, with fakes for unit tests (`zbm-system`), the state
directory `/zfsbootmenu` (`zbm-state`), and the crates the runtime's packages will fill (`zbm-boot`, `zbm-luks`,
`zbm-menu`).

The legacy runtime is frozen from now on: it gets bug fixes only, and each change is recorded in
`legacy/RUNTIME-CHANGES.md` so that the Rust runtime ports it. Images and the legacy runtime behave as before, except
for these fixes:

- A pool whose name is part of a restricted pool's name (`data` and an unhealthy `data2`), or matches it as a pattern
  (`a.b` and `axb`), is no longer refused read-write imports, and is recorded as restricted when it has unsupported
  features.
- When ZFSBootMenu unseals a TPM2 token with a PIN itself (without systemd's plugin), the authorization value derived
  from the PIN reaches `tpm2_unseal` on standard input instead of as an argument, which any process could read in
  `/proc`. A salted PIN still reaches `openssl kdf` as an argument, which `openssl` cannot avoid.
- Values from the kernel command line that contain a single quote (in `zbm.prefer`, `zbm.wait_for`, `zbm.hookroot`
  or `console`) are written to `/etc/zfsbootmenu.conf` so that sourcing the file gives them back, instead of breaking
  the file.
- dracut images keep the command line set with `embedded_kcl`; it was written to `/etc/zfsbootmenu.conf` before the
  file was created anew, and so was lost. (Release images also set `rd.hostonly=0` another way, so they were not
  affected.)

### Rust runtime, first boot

The first Rust runtime (Phase 5 of `PORT-ROADMAP.md`) can be selected with `zfsbootmenu_runtime=rust` in the dracut
or mkinitcpio configuration, and needs `zbm`. It starts ZFSBootMenu as process 1 under a supervisor, runs the start-up
stages (kernel modules, host ID, hooks, pool import), and boots the default boot environment of an unencrypted pool,
as the legacy runtime does with `zbm.skip`, with every start-up and import option (`zbm.import_policy`,
`zbm.force_import`, `zbm.prefer`, `zbm.wait_for`, `zbm.hookroot`, `zbm.skip_hooks`). Where a machine needs more (a
locked encryption root, `load-key.d` hooks or the menu), it logs what it cannot handle yet and opens the recovery
shell. The legacy runtime stays the default; see `zfsbootmenu(7)`.

### Rust runtime, LUKS

The Rust runtime unlocks LUKS volumes (package B of Phase 6): the crypttab, `zbm.luks` and `zbm.luks_key`, discovery
of volumes that may hold ZFS, missing and late devices, passphrases shared by several volumes, key files and key
devices, key scripts, TPM2 tokens (with cryptsetup's plugin or tpm2-tools, PINs and the lockout check), FIDO2 tokens,
clevis, GnuPG key files with a smart card or passphrase, and YubiKey challenge-response. In Rust images, `zluks` is
the Rust command, and the teardown hook closes the mappings with it. Unlike the legacy runtime, it writes no decrypted
key to a file, and computes the salted TPM2 PIN in the process instead of passing it to `openssl kdf` as an argument.

Also fixed in the legacy runtime: a LUKS mapping whose name, read as a pattern, matched a mapping recorded earlier
(`a.b` after `axb`) was not recorded, so the teardown hook left it open.

### Rust runtime, encrypted boot environments

The Rust runtime boots a default boot environment whose encryption root is locked, as the legacy runtime does: it
runs the `load-key.d` hooks, then loads the key from the passphrase prompt (`keylocation=prompt`), from a key file in
the image, or, for a `passphrase` key, from the prompt. When the key cannot be loaded it opens the recovery shell
with `unable to load key for <boot environment>`. The countdown's boot does not copy keys from a key source
(`org.zfsbootmenu:keysource`), as in the legacy runtime; the code that does, for boots chosen in the menu, is in
place for the menu.

### The new menu in Rust images

Images with the Rust runtime (`zfsbootmenu_runtime=rust`) start the new menu at the end of start-up, where the legacy
runtime counts down and starts the `fzf` menu. The countdown runs in the menu, which shows the environments meanwhile;
a key stops it. It runs as
`/bin/zfsbootmenu` (and `zbm`), so it also opens over SSH and from the recovery shell, and takes over from a menu
already running, as the legacy one does. It lists the boot environments with their kernels; Enter boots the selected
one, loading its key (from its key source, as boots chosen in the legacy menu do) and asking for a passphrase on the
console when it needs one. A locked environment is listed as locked, and its key asked for when it boots, rather than
before the menu shows. The recovery shell (`!`, then Enter) leaves the menu; after it, pools that became importable
are imported and the menu opens again. Reboot, power off and the firmware setup export the pools imported read/write
first. The rest of the menu (snapshots, kernels, the command line editor, pools and LUKS volumes) still answers
"not available yet" on the real machine; R5 steps 4 to 6 bring them. `fzf` stays in Rust images for the recovery
shell's tools (`logs`, `zsnapshots`, the help pages) until they are ported. The boot environment list of the
refactoring pilots (`zbm-menu-pilot`) is gone; the menu replaces it. `CHANGES-FROM-LEGACY.md` lists what behaves
differently from the legacy menu, and why.

### Start-up in the new menu

In Rust images, start-up now runs on the new menu's start-up view, when the terminal can run the menu: the stages
show as they run, and what they ask is asked in the menu's dialogs instead of console prompts. The unlock dialog asks
for the passphrase, TPM2 PIN or GnuPG passphrase of a LUKS volume, names its device, the tries left and what went
wrong last, and waits for a key device or security key with the seconds left; the import retries, missing devices and
unsupported pool features are questions that go on by themselves after the same delays as the console prompts. LUKS
volumes are unlocked in the start-up program, which reads the keys typed into buffers it wipes, so that no copy of a
passphrase stays in its memory. What the stages' programs print goes to the log rather than the screen; hooks and the
recovery shell still get the console. On a terminal that cannot run the menu, start-up keeps the console prompts.
`CHANGES-FROM-LEGACY.md` lists what changes.

### brush, the shell of Rust images

Rust images now include brush 0.4, a shell compatible with Bash and written in Rust, as `/usr/bin/brush`. The
recovery shell, hooks written for Bash or `sh`, interactive SSH logins and the runtime's own short scripts run on it;
`zbm.shell=bash` brings Bash back, and the recovery shell falls back to Bash when brush does not start. On serial
lines brush uses its basic line editor. GNU Bash stays in every image as dracut's `/bin/sh`. The image's brush has a
fix for a parser bug with `case` patterns inside `$( )` (`patches/`). `make install` and the build container build
and install it next to `zbm`; `zfsbootmenu_brush` names another one. It adds about 2.8 MB to a compressed image
(9.0 MB stripped). `CHANGES-FROM-LEGACY.md` lists what changes.

### Fixes from the review of the Rust port

- Rust images: a command line edited in the menu boots once, in place of the environment's own, without `root=` and
  with `noresume` and the host ID where they apply; it is no longer saved for every later boot.
- Rust images: when the menu cannot start, start-up counts down and boots without it; a hang-up (a dropped SSH
  session) ends the menu and removes its lock, and a lock left by a killed menu no longer blocks the next one; after
  another instance was seen, nothing boots by itself. `zbm debug crash` and `zbm report` run instead of the menu.
- Both runtimes: a command line embedded in the image (`embedded_kcl`) keeps quotes, `$` and backslashes as written;
  a line of `/etc/zfsbootmenu.conf` the Rust runtime cannot read is skipped with a warning instead of costing the
  whole file.
- `generate-zbm` piped into a pager that is quit early no longer dies between renaming the old image to its backup
  and installing the new one.
- `generate-zbm` (the Rust program; the Perl one is unchanged) installs images more safely:
  - an image directory or kernel prefix with spaces or glob characters no longer makes versioned images rewrite
    their first revision in place, and a new revision never replaces an existing file;
  - each image is copied to a temporary file, written to the disk, then renamed, so a failed copy, a full ESP or a
    power cut leaves the previous image as it was; a failed copy leaves no temporary file;
  - **the initramfs of a kernel prefix other than `vmlinuz` is named after it**: `initramfs-linux-lts-*.img` for
    `vmlinuz-linux-lts`, `initramfs-zbm-*.img` for the prefix `zbm`. With such a prefix, update the boot loader
    entries that name the initramfs (`initramfs-bootmenu.img` and `initramfs-<version>_<revision>.img` before). The
    images of two prefixes in one directory no longer replace each other, and pruning `vmlinuz-linux` no longer
    removes the images of `vmlinuz-linux-lts`;
  - `Components.Enabled` or `EFI.Enabled` without the section's `ImageDir` is an error, instead of installing in `/`;
  - an argument that is not valid UTF-8 is refused with a message (status 2) instead of a panic, as by `zbm` and
    `zbm-kcl`.
- Rust images, start-up: an interactive crypttab key script and `fido2-assert` asking for a FIDO2 PIN get the console
  while they run; a terminal that sends CR LF for Enter presses it once; a terminal smaller than 52x20 keeps the
  console prompts; a key device that appears is used once udev settled; what is typed is no longer shown during
  start-up, and the keys read and the digests of secrets are wiped.
- The menu keeps the selected environment and snapshot when the machine's lists change, rather than the row they
  were on, so a key no longer acts on what moved into their place.
- Rust images, menu: a boot chosen in the command palette shows the "may not boot" sheet as the lists do; the log
  scrolls to its last entry when lines wrap, and opens at its top from the palette and the recovery tools; the cursor
  of the command line editor follows the line once it wraps; a key sequence split on a serial console reached
  through the network is no longer taken for Escape in the main menu, which reads keys as start-up does; the menu
  keeps no secret typed at an unlock dialog.
- Rust images, console on the display (`zbm.console=drm`): Ctrl-C reaches a program that floods the screen; unplugging
  a keyboard, such as a YubiKey with OTP enabled, no longer ends the console, and keyboards plugged in later are used;
  a program that reads nothing cannot stop the console; Caps Lock lights its light, lost key events no longer leave
  a modifier held, and a dead key followed by a key it does not combine with types both, as the kernel's keymap does;
  the keys typed are wiped; a console that ended at once no longer delays each boot by 3 seconds; after a failed
  `kexec` the console takes the display again (`zbm console reattach`) to show the error.
- Rust images, process 1: logs saved after a crash (`zbm.save_logs`) are saved by a process of its own, so a hung
  device cannot stop the supervisor; `Ctrl-\` no longer kills start-up or the menu, as with the legacy runtime; the
  recovery shell after a crash gets a usable terminal (echo, Ctrl-C, the cursor) back; the EFI variables are made
  writable for `efibootmgr` in the recovery shell again, as the images have no `mountpoint` program to tell whether
  they were mounted; each command the menu runs is recorded once in the journal, not twice; when the console on the
  display ends, the recovery shell opens on the kernel console every time, where it could open on the console's dead
  terminal and end at once, leaving the menu to take the keys typed for the shell.
- Rust images, logs and diagnostics: with ZFS before 2.3, a command line property written over several lines or
  with tabs is read whole; saved logs (`zbm.save_logs`) are written before the teardown hooks, which may unbind a USB
  device the partition is on, the boot just saved is never pruned when the clock is behind, and a boot not saved
  whole is removed; without `loglevel`, errors and warnings reach the kernel log, as with the legacy runtime (4);
  long messages are split into records the kernel takes instead of being dropped; `zbm debug crash` shows the latest
  report; the hooks of a menu started over SSH get the variables of `/etc/zfsbootmenu.conf`; a LUKS mapping recorded
  by `zluks` over SSH while start-up closes others is no longer forgotten (and left open at teardown); the journal
  keeps no copy of its lines in memory.
- Shadow mode kills a command still running when its 5-second budget ends, so a hung `blkid` or `zpool status` no
  longer holds up a legacy boot.
- The VM tests rebuild the test boot environment when its files or the build container change, cover `config/`,
  `vendor/` and the build container in the image check, build each image once when run in parallel, report tests
  that did not run as `SKIP`, and fail a shadow-mode boot without a summary.

### Fixes from the bug review before the console's M4

- The menu lists an environment's kernels newest first, as it shows them: the kernel it names as the one that boots
  is the one that boots, and the kernels view no longer starts on the oldest.
- Rust images: start-up counts down and boots by itself when the menu crashes (a panic, or a signal), not only when
  it says it could not start.
- The menu asks for the environment's name to be typed before it clones and promotes a snapshot, as before every
  other change that cannot be undone; naming the clone and pressing Enter no longer promotes it at once.
- Rust images: leaving a brush recovery shell with `exit` goes back to the menu again. brush kept its own process
  group in the terminal's foreground when it ended, so the menu could read no key and every `exit` led to another
  shell; brush is patched to give the terminal back (`patches/`), and the supervisor, start-up and the menu take it
  back after every interactive shell, also one that was killed.
- The console on the display answers a question a program asked inside a synchronized update (such as the cursor's
  position) when the update's time runs out, not only at the program's next output, which a program waiting for the
  answer never sends.

### The new menu's text in a message catalogue

Every string of the new menu (`zbm-tui`) comes from a Project Fluent catalogue,
`crates/zbm-tui/locales/en/menu.ftl`, so that translations (ROADMAP.md #18) need no change of the code;
`cargo xtask lint` keeps literals out of the menu's code. `zbm-tui --demo --language pseudo` shows the menu in a
pseudo-language, accented and a third longer, as the snapshot tests draw it. See `docs/dev/menu-design/text.md`.

### Graphics drivers with the firmware of the GPUs an image is for (first step)

ROADMAP.md #17, its first step. `Kernel.GraphicsDrivers` in the `generate-zbm` configuration adds `amdgpu`, `i915`
or `xe` to images with the Rust runtime, with only the firmware of the GPUs they are for, instead of the tens of
megabytes `amdgpu` lists for every chip. A driver named alone is for the build machine's GPUs, found by their PCI
IDs in sysfs; for an AMD GPU that `amdgpu` drives during the build, the exact chip comes from the IP versions the
driver shows. A driver followed by chips or families (`amdgpu:gfx12`, `i915:gen12,mtl`) is for those, for an image
meant for another machine. `generate-zbm` reports the GPUs and the number of firmware files, takes only files the
build kernel's driver lists (`modinfo -F firmware`), removes `nomodeset` from `Kernel.CommandLine`, and stops before
building when a name is unknown, the machine has no GPU for a driver named alone, or the kernel lists no firmware for
a GPU. The dracut module and the mkinitcpio hook install the drivers past the `omit_drivers` of ZFSBootMenu's dracut
configuration and remove the firmware files nobody asked for (`zfsbootmenu_graphics_drivers`,
`zfsbootmenu_graphics_firmware`). In a test build on Linux 6.8, `amdgpu` and `i915` with the firmware of two AMD
chips and one Intel platform made the uncompressed image 9 MB larger; with all their firmware, 32 MB larger. The runtime loads the drivers itself, before anything is asked, so that the display does not change mode under a
passphrase prompt: the image keeps udev from loading them, and `zfsbootmenu-preinit` loads them before the console
on the display starts, which draws on the first GPU with a connected display. `zbm.gpu=` chooses the GPU they drive
(a PCI address, `amd` or `intel`); the others get a driver override no driver matches and stay as the firmware left
them. `zbm.gpu=off` loads none for one boot. The VM test `runtime-rust-gpu` checks this on QEMU's virtual GPUs, whose
drivers (`virtio_gpu`, `bochs`) the build option `zfsbootmenu_graphics_drivers` also takes. Before `kexec`, the
runtime hands each GPU its drivers took back: it unbinds the GPU from its driver, which runs the driver's removal,
and resets its PCI function where the device has a reset, so that the booted kernel finds it as at power-on; the
kernel log says how each GPU was handed back. When `kexec` fails, the drivers take the GPUs again, so the console can
show the error. The drivers take only GPUs known to come back after this, listed by PCI ID: for now QEMU's virtual
GPUs, until real ones pass twenty boots in a row; the others stay on the firmware's framebuffer, with the reason in
the kernel log, and `zbm.gpu_untested` on the command line loads their drivers to try them. With `zbm.save_logs`,
the saved kernel log and journal are written again after the hand-back, so that the booted system can read how each
GPU was handed back (the journal has it at any `loglevel`). Not finished: runs on
real hardware. Release images are unchanged.

### Console font on high-density (HiDPI) displays

In images built with `zfsbootmenu_runtime="rust"`, the early-setup hook `30-console-autosize` runs
`zbm console-autosize`, which sizes the console font to the display. Where a kernel graphics driver exposes the
display's EDID, it takes the font that makes text about 4.5 millimetres tall while leaving at least 110 columns, so a
4K laptop and a 4K monitor both get readable text; fonts doubled by `setfont -d` reach 64 pixels where the kernel
and `setfont` accept them. Without an EDID it keeps the legacy rule (the largest font up to 32 pixels that leaves 110
columns). `zbm.autosize=<pixels>` asks for a height. ZFSBootMenu images include no graphics drivers by default, so
the EDID is there only in images that add one (`add_drivers+=" i915 "`, for instance); see `zbm.autosize` in
`zfsbootmenu(7)`. Images with the legacy runtime are unchanged.

### Reproducible images

ROADMAP.md #5. With `SOURCE_DATE_EPOCH` set, two builds of the same tree in the same build container give the same
bytes: the Rust `generate-zbm` passes `--reproducible` to dracut, `objcopy` stamps the EFI bundle with that time, and
the image's `BUILD_ID` is that day. The build container takes the variable from its environment (`zbm-builder.sh`
passes it on) and builds `zbm` with its work directory remapped. Set it to the time of the last commit:
`SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"`. A CI job builds the release image twice, from checkouts in
different directories, and compares the bytes.

### Secrets in locked memory

ROADMAP.md #1, in images with the Rust runtime. Passphrases, PINs and keys are kept in memory of their own: locked so
that it is never swapped out, left out of core dumps and child processes, and, on kernels that allow `memfd_secret`,
out of the kernel's own map of memory. They are wiped when no longer needed, the runtime's processes are undumpable,
and the console on the display wipes the key codes it reads. Before booting a kernel, ZFSBootMenu drops the page
cache, so that key files read from key devices leave memory. Boot Rust images with `init_on_free=1` on their command
line (`Kernel.CommandLine`) so that the kernel zeroes the memory it frees; `zbm report` says whether it does. Two VM
tests search the whole machine for the passphrase and the key file just before `kexec`.

### Saved boot logs and bug reports

ROADMAP.md #9, in images with the Rust runtime. With `zbm.save_logs` on the kernel command line, ZFSBootMenu saves
its runtime journal, its crash reports and the kernel log on the EFI system partition just before it boots a kernel,
and after one of its programs crashes, so that they survive the boot and a restart. They go to
`EFI/zfsbootmenu/logs/<time>`; the newest five stay, and the partition is mounted only for the write. The option
takes the partition as a tag or a device (`zbm.save_logs=LABEL=EFI`); without one it is the partition ZFSBootMenu was
loaded from, or the machine's only EFI system partition. In the recovery shell, `zbm debug saved` lists the saved
boots and `zbm debug save` saves now. `zbm report` writes a bug report to attach to an issue, on the same partition
(`EFI/zfsbootmenu/reports`), in a directory such as a mounted USB stick, or on another partition: versions, pool
status, a summary of each LUKS header (UUID, tokens' types and key slots), the TPM's state, the crash reports, the
journal and the kernel log. It holds no key material, which a test checks.

### Reports for finding out why a boot went wrong

In images with the Rust runtime, `/libexec/zbm debug` prints reports from the recovery shell or an SSH session,
described in the new `zbm-debug(8)`: `config` (each option of the kernel command line, its value and where it came
from), `state` (the files of `/zfsbootmenu`, without key files), `journal` (the runtime journal, from a level up),
`commands` (each program the runtime ran, its exit status and how long it took), `luks` (the LUKS volumes, their
state and key methods) and `pools` (the pools and boot environments). They only read, and print no key material.

### The console on the display: starts reliably, draws correctly

The console of `zbm.console=drm` (milestone M1 of `docs/dev/console-roadmap.md`):

- it starts without a keyboard, and takes a USB keyboard when it enumerates, where the boot stayed on the kernel
  console;
- it draws on the output the kernel console shows, in the mode it shows, rather than on the first connected one in its
  preferred mode: a closed laptop's panel stays dark, and the monitor neither flickers nor loses its signal;
- the cursor takes the shapes programs ask for (block, underline, bar) and blinks, as on the Linux console;
- text is drawn with five kinds of underline (single, double, curly, dotted, dashed), in the colour programs give them,
  and struck out;
- a program that writes a lot is drawn once per burst of output, and a scrolled screen is moved rather than drawn
  again;
- the keypad types digits with Num Lock and moves the cursor without it; Num Lock starts as the keyboard's light shows
  it, and the light follows it;
- what hooks and tools write to `/dev/console` shows on the console, not on the kernel console it hides;
- after a failed `kexec`, the console takes the display again with the screen it had, not a blank one.

### The console on the display: a terminal of its own

The console of `zbm.console=drm` (milestone M2 of `docs/dev/console-roadmap.md`) is now a terminal that programs can
use fully:

- programs on it run with `TERM=xterm-256color` and `COLORTERM=truecolor` when the image has that terminfo entry,
  which Rust images now include, and see 256 colours and true colour; `/etc/profile` keeps that `TERM` for login
  shells on the console;
- Ctrl, Alt and Shift with the cursor keys, Home, End, Page Up and Down and the function keys send xterm's
  sequences, so that Ctrl-Left moves back a word in the recovery shell; the keypad's application mode, new line mode
  and the kitty keyboard protocol work for the programs that ask for them;
- characters two cells wide, such as Chinese or Japanese, are drawn across both cells, and glyphs that reach beyond
  their cell are not cut;
- bold, italic and bold italic text is drawn with DejaVu Sans Mono's own faces, about 840 KB in the image, which
  `zfsbootmenu_console_faces=regular` leaves out; `zfsbootmenu_console_fallback_fonts` adds fonts for the characters
  DejaVu lacks;
- underlines and strikeout sit where the font puts them; Braille, the diagonals of box drawing, the Powerline arrows
  and the sextants are drawn to fill their cells, also with the bitmap console fonts; the cursor takes the colour
  programs give it (OSC 12);
- what programs send to a terminal named `linux` works: the palette (`setterm`'s `ESC ] P`), the cursor size, and
  `CSI 8 ]`, which makes the current colours the default ones; blinking text blinks, and the reverse screen (`setterm
  --reverse`) swaps the colours; XTVERSION answers `ZFSBootMenu` and its version.

The last items needed changes to `alacritty_terminal` and `vte`, which the build takes from `patches/` until their
authors release them. Double-width and double-height lines (DECDWL, DECDHL) are left for later. A test card drawn
with three fonts is compared with reference images in the repository.

### The console on the display: a comfortable recovery shell

The console of `zbm.console=drm` (milestone M3 of `docs/dev/console-roadmap.md`):

- Shift+PgUp and Shift+PgDn show the lines that scrolled off, 2000 by default (`zbm.console_scrollback`), kept in
  memory that is never swapped. While a prompt reads a passphrase without echo, or the menu's passphrase dialog is
  open, the scrollback is overwritten and none is kept, so no passphrase or what came before it stays in memory.
  Ctrl+Shift+F and Ctrl+Shift+B search it, Ctrl+Shift+Space starts vi mode, and Ctrl+Shift+S saves it for
  `zbm report`;
- Ctrl+Alt+`=`, `-` and `0` make the text larger, smaller and as it was, as does `zbm console font`;
  `zbm.console_palette` chooses a colour scheme, and `zbm.console_large=on` large text in high contrast;
- `zbm.console_status=on` keeps a status line with Caps Lock, Num Lock, the layout, the title, the time and the
  battery; `zbm.console_bell` makes the bell flash the border or sound the PC speaker; Print Screen saves the screen's
  text and image for `zbm report`, but not while a passphrase is asked for;
- `zbm.numlock=on` starts with Num Lock on, Scroll Lock holds the output, `zbm.keyboard_repeat` sets the key repeat,
  and F13 to F24, Print, Pause, Menu and the media keys reach programs;
- Super+Space switches to the image's next keyboard layout, the waiting accent of a dead key shows at the cursor,
  Ctrl+Shift+U types a character by its code, `zbm.keyboard_compose` chooses a compose key, and
  `zbm.keyboard_layouts` gives USB keyboards layouts of their own.

### The console on the display: quieter, cleaner, accents

Five fixes to the console of `zbm.console=drm`:

- it sleeps while nothing happens: `zbm console release` and `layout` wake it through their signals, and new keyboards
  through an inotify watch on `/dev/input`, where it woke five times a second and read sysfs each time;
- the screen's text, which only the VM tests' mirror reads, is no longer built after every frame;
- a TrueType character is drawn from the font's cache without copying its pixels for each cell;
- what the programs wrote is wiped once drawn or relayed, and the terminal's cells and pixels when the console ends or
  hands the display back;
- combining marks (`e` followed by U+0301) are drawn over their character, centred on its cell, and kept in the
  screen's text.

### Keyboard layouts on the console on the display

Step 4 of ROADMAP.md #19. Images with the Rust runtime carry keyboard layouts for the console of `zbm.console=drm`:
`zfsbootmenu_keyboard_layouts` names them (XKB names such as `de` or `de(nodeadkeys)`; by default the build host's
`XKBLAYOUT` from `/etc/vconsole.conf`, then `us`, `de`, `fr` and `gb`), and the new `zbm keyboard-layouts` compiles
each from the build host's XKB data into a self-contained keymap of about 70 KB in `/usr/share/zfsbootmenu/layouts`,
with the dead keys of the `en_US.UTF-8` compose rules. The console types with the first, or with
`zbm.keyboard_layout`: Shift, Caps Lock, AltGr and dead keys as XKB defines them, and Ctrl with a key of a non-Latin
layout gives the US letter. `zbm console layout <name>` switches layouts while it runs. Images built without the XKB
data (`xkb-data` or `xkeyboard-config`) and the compose rules (`libx11-data` or `libX11`) warn and keep the US layout;
the build container installs both, so rebuild it. The recovery shell's prompt takes those characters too: Rust images
get `/etc/inputrc` with 8-bit input and output for readline, and the build host's `C.UTF-8` character set, when its C
library has one, with `LC_CTYPE=C.UTF-8`. New dependency: `kbvm`, chosen over `xkbcommon-rs`.

### TrueType fonts on the console on the display

Step 3 of ROADMAP.md #19. Images with the Rust runtime carry DejaVu Sans Mono 2.37 (340 KB, Bitstream Vera licence),
and the console of `zbm.console=drm` draws with it, through `swash`, in cells as high as the console font
`zbm.autosize` chooses: Latin, Greek and Cyrillic without a 512-glyph subset, anti-aliased and hinted. Box drawing and
block characters are drawn to fill their cells. `zbm.console_font=psf` keeps the console fonts. New dependency:
`swash` with `skrifa`, chosen over `fontdue`, whose parser is unmaintained (RUSTSEC-2026-0192).

### The console on the display (`zbm.console=drm`)

Step 2 of ROADMAP.md #19, for images with the Rust runtime, off by default. With `zbm.console=drm` on the kernel
command line and a virtual terminal as the control terminal, ZFSBootMenu runs its start-up program and recovery shell on
a console of its own, `zbm console`: Alacritty's terminal (`alacritty_terminal`) on a pseudo-terminal, drawn through DRM
with the image's console fonts, with 256 colours and true colour, no scrollback, and keys read from the keyboards on the
US layout. Before `kexec` it hands the display, the keyboards and the virtual terminal back to the kernel and relays to
the kernel console; when it cannot use the display, or ends, ZFSBootMenu goes on on the kernel console. The image needs
the `evdev` module and the display's DRM driver. New dependencies: `alacritty_terminal`, with `rustix` and
`signal-hook` used directly.

### The menu on the display without a terminal (prototype)

The first step of ROADMAP.md #19, for developers and VM tests only: images are unchanged. The new crate `zbm-display`
draws through DRM, with any DRM driver including `simpledrm` on the firmware's framebuffer, using the image's console
fonts at the size `zbm console-autosize` chooses, and reads keys from the keyboards' input devices (US layout). While it
draws, the virtual terminal is in graphics mode with its keyboard off; both, and the display, go back to the kernel
console when the menu ends. `zbm-tui --demo --drm --font <psf>` runs the menu prototype that way on a virtual console,
as root; images built with the `menu-demo` feature have `zbm debug menu-demo`, which the VM test `runtime-rust-drm`
runs. The only new dependency is the `drm` crate.

### Spleen console fonts

Images with the Rust runtime can use the Spleen fonts (BSD-2) instead of Terminus: `ConsoleFont: spleen` in the
`Global` section of `config.yaml`, which `generate-zbm` passes on as the new build option
`zfsbootmenu_console_font=spleen`. Spleen has native 8×16, 12×24, 16×32 and 32×64 cells, so the densest displays get
a font drawn at 64 pixels instead of a doubled 32-pixel one; the image carries Spleen's licence as
`/usr/share/zfsbootmenu/fonts/LICENSE.spleen`. Like Terminus, the fonts are installed as 256-glyph subsets. Without a
display density, `zbm console-autosize` keeps the legacy limit of 32 pixels whatever the family. Images with the
legacy runtime, whose hook loads the Terminus fonts by name, warn and keep Terminus, as the build does for any other
value. `generate-zbm` refuses a `ConsoleFont` other than `terminus` or `spleen` before it builds anything.

### Console fonts of 256 glyphs

Images built with `zfsbootmenu_runtime="rust"` install their console fonts as 256-glyph subsets, made at build time by
`zbm console-fonts`. A console font of more than 256 glyphs costs colours (`setfont(8)`): on a VGA text console the
eight bright ones, which the menu uses for secondary text, titles and warnings. The subsets keep ASCII, Latin-1, the
letters Spanish, French, Dutch and Brazilian Portuguese use beyond it, the menu's symbols and the line characters,
and draw the screen exactly as the whole fonts do; a character a font lacks is drawn with a look-alike it has. ASCII
keeps its codes and the line characters their codes of code page 437. The six Terminus fonts take 65 KB instead of
130 KB. When `zbm` cannot make them, the image build installs the whole fonts, as legacy images do.

### Refactoring pilots

`REFACTOR-ROADMAP.md` plans the refactoring of the Rust code into layers (domain types, ports, adapters, an
application layer and front ends) and a new menu on Ratatui. Two pilots run the new layers beside today's code in
images built with `zfsbootmenu_runtime="rust"`; nothing uses them unless asked:

- `zluks --pilot open DEVICE NAME --key-file PATH` opens one LUKS volume with a key file, which goes to `cryptsetup`
  on standard input; `--journal FILE` writes every step, and every program run, as JSON lines. Mappings it opens are
  not closed by the teardown hook.
- `zbm-menu-pilot` (run as `/libexec/zbm zbm-menu-pilot`) shows the boot environments of the imported pools in the
  new menu and prints the one chosen; it boots nothing. `--demo` shows an example machine.

Fixed in the Rust runtime: when a pool could not be imported because another host had used it, and `zpool import`
printed text that was not ASCII before the host ID, the start-up program stopped instead of reading the host ID
(found by fuzzing).

## ZFSBootMenu v3.1.0 (2026-01-01)

ZFSBootMenu v3.1.0 includes fixes, additional platform tooling and feature enhancements. 

Binary images are built with OpenZFS 2.4.0 and Linux 6.6, 6.12 and 6.18 series kernels. 

### New features

`aarch64` and device tree support has been added to both `generate-zbm` and the bootloader process. A UEFI image for compatible `aarch64` systems, such as the Lenovo X13s, can now be built.

The `zbm.prefer` kernel argument has been extended to understand datasets as well as pools. When a dataset value is passed to `zbm.prefer`, that dataset will override any `bootfs` value, if set, for that pool.

`org.zfsbootmenu:readonly` is now an understood *pool* property. ZFSBootMenu will refuse to import the pool *read/write* when the property is set to any value other than `-` or `off`. Since ZFSBootMenu is unable to detect encrypted hibernate images, this property can be set as a fail-safe on those systems.

When available, ZFSBootMenu will use new `fzf` features to improve readability when filtering long lists, e.g. when searching for a specific snapshot.

### Fixes

Systems identifying themselves as CachyOS will now have the correct root argument set on the environments kernel command line.

Any dataset defined by `org.zfsbootmenu:keysource` must now be an encrypted dataset, or else the property will be ignored. This dataset, by design, must hold encryption keys; it should not effectively be world-readable.

Filesystems manually mounted inside a boot environment chroot are now unmounted on exit. This corrects an error inhibiting access to the environment until a reboot or manual corrections took place.

`zbm-kcl` can now correctly distinguish between a dataset and an EFI file for certain operations.

Installation guides provided by ZFSBootMenu will now recommend ZFS 2.3 as the default pool compatability setting, now that ZFS 2.3.x is the long term support release provided by OpenZFS.

Multiple new contributors provided documentation updates and fixes. Thanks @FingerlessGlov3s, @ckenna, @aptalca, @woodsb02, @zipcode!

### Significant commits in this release
* e4c1a0e zfsbootmenu: fix fzf color spec for the ancient ones (Andrew J. Hesford)
* 0d96299 zfsbootmenu: look less bad on BIOS consoles (Zach Dykstra)
* b80ac54 zfsbootmenu: add and use recursive_umount function (Zach Dykstra)
* d63a844 docs/guides/fedora: use DNF5 syntax and update ZFS RPM packages (Ben Woods)
* 455c50c zfsbootmenu-core.sh: do not cache unencrypted keysources (Andrew J. Hesford)
* d76b38b zfsbootmenu: pair CachyOS and mkinitcpio for root= (Zach Dykstra)
* 0ef53d5 github: try to force parsing as a string (Zach Dykstra)
* dfdc704 github: move version text to new block (Zach Dykstra)
* f38c983 github: update default version, add new kernel version field, help text (Zach Dykstra)
* 89bb1d5 docs/guides/fedora: move package installation to reusable template (Andrew J. Hesford)
* c552142 zfsbootmenu: look less bad on the console (Zach Dykstra)
* b8b534e zfsbootmenu: use fzf --raw when available (Zach Dykstra)
* ca3a81b dracut: exclude clevis due to missing systemd module (zip)
* 754d337 docs/guides/debian: Debian Trixie (FingerlessGloves)
* 5b6fa96 docs/guides/almalinux: AlmaLinux 10 is here (FingerlessGloves)
* 29e0743 zfsbootmenu: add org.zfsbootmenu:readonly gate (Zach Dykstra)
* f616019 zfsbootmenu: extend zbm.prefer to set bootfs (Zach Dykstra)
* 932d725 shellcheck: fix warnings from 0.11.0 (Zach Dykstra)
* aece6fe docs/guides/almalinux: add AlmaLinux 9 guide (FingerlessGloves)
* fd56dad docs/guides: collapse layers of indirection for single-purpose guides (Andrew J. Hesford)
* 19e7618 docs/guides/: recommend openzfs-2.2-linux pool compatibility (Zach Dykstra)
* 318cf23 docs: dracut-crypt-ssh keys name and format fix (JkktBkkt)
* 6905048 zfs-chroot: unmount everything under chroot mountpoint (Zach Dykstra)
* 28a2db9 docs/guides/chimera: ensure initramfs-tools config directory exists (Andrew J. Hesford)
* 54bbde2 docs: mention use of dhclient with mkinitcpio-rclocal for DHCP (Andrew J. Hesford)
* fc9721e docs: fix dropbear path in containerized build example (Chris Kenna)
* c77d5f4 Remove obsolete section on xhci-teardown from docs (aptalca)
* 9ed49ad aarch64 support: efi stub and devicetree selection (Zach Dykstra)
* c6ce98c bin/zbm-kcl: fix detection of EFI files (Andrew J. Hesford)
* 1130cc9 docs/: remove dead Arch wiki link (Zach Dykstra)
* 51d9697 docs: typo fix in instructions (aptalca)
* 4e716eb docs: copy dropbear keys and config instead of symlink (aptalca)
* f1c873a docs: we made it to 2025 somehow (Zach Dykstra)
* 4f5fff1 sign-assets.sh: add manual passphrase input option (Zach Dykstra)

## ZFSBootMenu v3.0.1 (2025-01-29)

ZFSBootMenu v3.0.1 includes fixes for two issues. The `zbm.skip` and `zbm.timeout=0` arguments are now correctly handled, breaking the infinite loop on countdown prompts. Hooks listed in the System Report help section correctly indicate if they're enabled or not, and now show if they've run once.

Binary images are built with OpenZFS 2.3.0 and Linux 6.1, 6.6 and 6.12.

### Significant commits in this release

* ff7c935 zfsbootmenu-core.sh: fix instant timed prompts (Zach Dykstra)
* 849207e zreport: handle hooks that run once and are completed (Zach Dykstra)

## ZFSBootMenu v3.0.0 (2025-01-26)

ZFSBootMenu v3.0.0 collects several bug fixes and refinements to the user experience within the loader as well as the process of creating images.

Binary images are built with OpenZFS 2.3.0 and Linux 6.1, 6.6 and 6.12.

### New features

The ZFSBootMenu initialization process, previously encapsulated in a monolithic function run once at boot, has been split into independent parts. The initialization process is now easier to customize, because additional initialization scripts can be interleaved with the system process. Furthermore, execution of individual components is now easy to track, and ZFSBootMenu uses this facility to ensure that each initialization script is run only once. The main `zfsbootmenu` process uses this guarantee to retry initialization whenever it is launched, rather than assuming that all initialization was successfully completed at boot. In particular, this can be helpful for remote connections when the initialization process (*e.g.*, pool imports) require manual intervention that may not have been completed on the console.

UEFI systems will benefit from several enhancements. Image creation should now include kernel modules necessary to mount VFAT filesystems, which ensures that external ZFSBootMenu hooks installed on an EFI system partition will always be loadable. On systems that support it, ZFSBootMenu now provides users with an option to reboot into EFI firmware.

A new kernel argument, `zbm.waitfor`, allows the specification of devices that must be present before ZFSBootMenu will attempt to boot pools. This may be useful, for example, to ensure that encryption keys on external devices are present for automatic unlocking.

### Fixes

The `zsnapshot` utility provided in the recovery environment now verifies that a specified filesystem is actually ZFS. When invoked without a filesystem argument, a menu of candidate filesystems will be presented.

Handling of symbolic links in the install helper and in the initramfs creation modules ensure that helpful aliases like `zbm` and `reboot` will be available in images generated with dracut as well as mkinitcpio.

Console initialization that was previously done only in dracut images is now done in mkinitcpio images.

ZFSBootMenu will print the kernel and boot environment it is attempting to boot immediately before it invokes `kexec`, which removes ambiguity about unexpected hangs in the boot process.

When building custom images, the `generate-zbm` utility now properly ignores kernels for which it cannot determine a version either from the kernel contents or the file name.

The ZFSBootMenu build container better handles multiple kernel versions. A new `-k` option allows users to specify any kernel series offered by Void Linux, and the container will ensure that all components are necessary to build the ZFS kernel module for that series.

### Significant commits in this release

* b5fb075 - early-setup.d/: create generic hook, console-init now dracut only (Zach Dykstra)
* ed2272c - zfsbootmenu: print bootenv and kernel just before kexec (Zach Dykstra)
* 3ee4f29 - bin/generate-zbm: require at least one kernel version value (Zach Dykstra)
* db78c98 - dracut: explicitly require the bash module (Zach Dykstra)
* 693b64a - install-helpers.sh: fix realpath -> readlink (Andrew J. Hesford)
* dbb23f1 - init.d/50-import-pools: tailor text if using a binary release (Zach Dykstra)
* 6eebde0 - lib/zfsbootmenu-core.sh: add initramfs generator to zreport (Zach Dykstra)
* 9f5e426 - releng/docker/build-init.sh: make it easier to customize kernel versions (Andrew J. Hesford)
* 6075572 - docs/index: fix repeated word. (Érico Nogueira)
* b1c12af - docs/guides/alpine/: add zfs-scripts to packages installed (camckay)
* 074d83b - zfsbootmenu-ui.sh: fix typo in fallback message (zenodotus280)
* d48408b - *: remove execute bit on files not intended for execution (Andrew J. Hesford)
* 4231230 - install-tree.sh: use tar to capture symlinks properly (Andrew J. Hesford)
* 9a6e7dc - docs/guides/fedora: update to Workstation 40 (Kevin K.)
* b7124bb - contrib: add megaraid_sas unbind script (Zach Dykstra)
* 1b496a6 - contrib: remove system hook, clean up docs (Zach Dykstra)
* 1a6c694 - zfsbootmenu/install-helpers.sh: preserve relative symlinks (Andrew J. Hesford)
* 14e4c1d - docs/general/container-building: fix InitCPIOHookDirs parent key (Bojan Petrović)
* dc755a8 - docs/guides/alpine: include nvme initramfs module (eli)
* 08dda60 - zfsbootmenu: add modal power menu on main screen (classabbyamp)
* 29fb17e - zfsbootmenu/lib/zfsbootmenu-ui.sh: add modal prompt (classabbyamp)
* 02d3755 - zfsbootmenu/bin/reboot: add reboot-to-uefi functionality (classabbyamp)
* 2994512 - zfsbootmenu: attempt initialization if necessary (Andrew J. Hesford)
* 1538998 - lib/zfsbootmenu-core: allow timed_prompt to delay indefinitely (Andrew J. Hesford)
* e3b1ff3 - init: split initialization into quasi-idempotent parts (Andrew J. Hesford)
* 5b61ab9 - bin/generate-zbm: support including a splash image in unified EFI bundles (classabbyamp)
* 4a034be - *: prefer systemd-boot stub to gummiboot (classabbyamp)
* 3b34348 - releng/tag-release.sh: no longer ship zbm-kcl as a release asset (Zach Dykstra)
* 96c4d5e - zfsbootmenu-ui: fix new populate_be_list for encrypted filesystems (Andrew J. Hesford)
* dae859d - releng/version.sh: fix git-based version detection (Andrew J. Hesford)
* c83f534 - zsnapshots: allow filesystem selection when no argument is specified (Andrew J. Hesford)
* cd7d55d - zfs-chroot: remove PROMPT_COMMAND from chroot env (Zach Dykstra)
* cd37f6a - zfsbootmenu-core: discard stderr on zfs/zpool cmds (Zach Dykstra)
* 2611f36 - zfsbootmenu: clean up main loop (Zach Dykstra)
* 0f87c5f - docs: fix curl arguments in container-example (Winston Hoy)
* 98f7dac - zfsbootmenu: generalize zbm.import_delay as zbm.retry_delay (Zach Dykstra)
* 075cba0 - zfsbootmenu: add zbm.waitfor argument handler (Zach Dykstra)
* ad127f3 - recovery shell: stop double printing errors on first launch (Zach Dykstra)
* 4868300 - install-helpers.sh: fix date template syntax (Fermín Olaiz)
* aa8654b - zfsbootmenu logs: remove alias, fix header with alt name (Zach Dykstra)
* f9b3f70 - recovery shell: print zerrors since the last command was executed (Zach Dykstra)
* 3ac3466 - recovery image: remove broken cryptsetup (Zach Dykstra)
* fd4d012 - recovery shell: ESP-related tooling (Zach Dykstra)
* 14d8554 - zsnapshots: check if input is a valid ZFS filesystem (Zach Dykstra)
* be37517 - 20-console-autosize.sh: respect mkinitcpio consolefont (Zach Dykstra)

## ZFSBootMenu v2.3.0 (2023-12-12)

ZFSBootMenu v2.3.0 introduces a few new capabilities and refactors some existing features.

### New features

The ZFSBootMenu hook system has been enhanced. In addition to "early setup", "setup" and "teardown" hooks, users can install "load-key" and "boot-selection" hooks to better control the unlocking of ZFS filesystems and the process of booting a selected environment. Hook installation and management has been simplified: the `zfsbootmenu_{early_setup,setup,teardown}` variables in `dracut` and `mkinitcpio` configurations have been deprecated in favor of `zfsbootmenu_hook_root`, which points to a directory containing subdirectories that correspond to each hook stage. ZFSBootMenu now includes "system" hooks that are automatically installed in every generated image; the USB-teardown and font-resizing hooks provided as `contrib` scripts are now installed as system hooks. A new command-line argument, `zbm.skip_hooks`, allows individual hooks to be skipped at runtime.

When ZFSBootMenu is unable to import any ZFS pools, it will capture and display import errors when entering an emergency shell.

The commands `reboot`, `poweroff` and `shutdown` are provided in ZFSBootMenu images, providing a convenient means to interrupt or restart the boot process from an emergency shell.

Various UI and UX enhancements improve the user experience within ZFSBootMenu:

* The `zreport` utility provides more information about the environment
* Online help now includes a "System Report" tab that displays the output of `zreport`
* Pre-built ZFSBootMenu images will no longer complain on hostid mismatches
* The main menu now shows the key on which entries are sorted

### Fixes

Chimera Linux boot environments are now properly recognized and will receive the appropriate root prefix by default.

Pre-built recovery images now include `sgdisk` and SSL certificates that will allow `curl` to communicate with HTTPS servers. The pre-built release and recovery images both include the `zbm-kcl` utility to allow convenient editing of command-line arguments for boot environments as well as ZFSBootMenu EFI bundles.

The `zbm` and `debug` commands, previously shell aliases, are now symlinks; this makes these commands accessible to users of images built with `mkinitcpio`.

Searches for kernels and initramfs images in boot environments have been improved. Although results of searches should be the same, the searches should happen slightly faster.

The `contrib/zbm-sign.pl` script now respects the `EFI.ImageDir` variable in the `generate-zbm` configuration file.

### Removal

Support for managing `syslinux` configurations with `generate-zbm`, long deprecated, has been removed. Users must now manage syslinux configurations manually.

### Significant commits in this release

* cb8dfaa - zfsbootmenu-core: show enabled/disabled hooks in zreport (Zach Dykstra)
* 3739e89 - zreport: show canmount dataset property (Zach Dykstra)
* efea0c1 - zfsbootmenu: use symlinks for two useful aliases (Zach Dykstra)
* 0d29988 - lib/zfsbootmenu-kcl: clean up argument reading (Andrew J. Hesford)
* 07d6e73 - zfsbootmenu-preinit: single-quote values written to /etc/zfsbootmenu.conf (Andrew J. Hesford)
* 23bdb71 - zfsbootmenu-ui: fix argument order when listing snapshots (Andrew J. Hesford)
* 14fb79f - zfsbootmenu-core: use new zbm-release file, make zreport pretty (Zach Dykstra)
* 691e332 - zfsbootmenu-help: add system report section to online help (Zach Dykstra)
* 9d7eba4 - Improve version handling in the repository and ZBM images (Andrew J. Hesford)
* 8f1570f - contrib/zbm-sign.pl: respect EFI.ImageDir (John Zimmermann)
* d7ebc84 - examples: modernize, install (Zach Dykstra)
* 55f08b9 - generate-zbm: remove integrated syslinux support (Zach Dykstra)
* f23fc69 - zfsbootmenu-core: fix typo in warning text (Zach Dykstra)
* 9588659 - zfsbootmenu/: tab-delimit kernel lists for boot environments (Andrew J. Hesford)
* 831d55a - releng/docker: Dockerfile parity with image-build.sh (Min Idzelis)
* 57b5572 - zfsbootmenu: remove legacy root= usage/design (Zach Dykstra)
* e93de79 - zfsbootmenu: add sysrq-based reboot/poweroff (Zach Dykstra)
* 6afe2ff - zfsbootmenu-core: remove SIGINT trap when launching e-shell (Min Idzelis)
* abf9649 - zfsbootmenu: rename UI library, normalize imports (Zach Dykstra)
* 40f7928 - zfsbootmenu-core: optimize initramfs search (Andrew J. Hesford)
* d7caa8c - releng/docker/image-build.sh: use tmpfs for XBPS cache (Andrew J. Hesford)
* 3f3e831 - zfsbootmenu: make zbm.autosize a system hook (Zach Dykstra)
* 9b348b4 - zfsbootmenu-core: disable spl_hostid warnings (Zach Dykstra)
* bd1319f - zfsbootmenu: log errors for unimportable pools (Zach Dykstra)
* 2762b7e - zfsbootmenu-core: run boot environment hooks after environment mount (Agorgianitis Loukas)
* 9524215 - releng/docker/build-init.sh: add rc.pre.d hooks (Andrew J. Hesford)
* 640af57 - Refactor user runtime hooks installation and processing (Andrew J. Hesford)
* 7045d1f - zfsbootmenu-lib: show sort key where needed (Zach Dykstra)
* ff59562 - zfsbootmenu: refactor installation, support system and user runtime hooks (Zach Dykstra)
* e5cfb46 - releng/docker/image-build.sh: improve repo and kernel selection (Andrew J. Hesford)
* 1526bb8 - zfsbootmenu-core: improve kernel searches (Andrew J. Hesford)
* 955ac2f - zfsbootmenu-core: remove trailing whitespace (Andrew J. Hesford)
* e7a3b89 - releng/docker: update Void base images (Andrew J. Hesford)
* 3682393 - releng/docker/image-build.sh: support custom XBPS repositories (Andrew J. Hesford)
* c8c9e8d - contrib/remote-ssh-build.sh: Add pool key to system mkinitcpio config (Emad Elsaid)
* 56cbbe8 - dracut, initcpio: make libgcc_s search universal (Andrew J. Hesford)
* deefabd - release,recovery: add tools to images (Zach Dykstra)
* 2431eff - zfsbootmenu-core: log error when be is locked in mount_zfs (Zach Dykstra)
* 3616009 - zfsbootmenu: add default Chimera Linux KCL (Zach Dykstra)
* 9343eb5 - zfs-chroot: small visual fixes/improvements (Zach Dykstra)

## ZFSBootMenu v2.2.2 (2023-10-24)

ZFSBootMenu v2.2.2 contains no runtime changes relative to v2.2.1. The creation of UEFI bundles (so-called unified kernel images, or UKIs) in `generate-zbm` has been updated to properly support newer UEFI stub loaders that were known to cause boot failures in the past. These changes were inspired by similar functionality in mkinitcpio and the `ukify` utility included with systemd.

### Significant commits in this release
* 9424356 - bin/generate-zbm: add .linux section last (Zach Dykstra)
* 36a0a66 - bin/generate-zbm: use mkinitcpio method to calc offsets (Zach Dykstra)

## ZFSBootMenu v2.2.1 (2023-10-13)

ZFSBootMenu v2.2.1 offers a few minor bug fixes over the prior release. More significantly, the build-container images and release assets for this version include OpenZFS 2.2.0 to provide access to upgraded pools.

All users of the binary releases or the ZFSBootMenu build container are encouraged to update to ZFSBootMenu v2.2.1 before upgrading any ZFS pools.

### Significant commits in this release
* e4102ee - zfs-chroot: load keys again after setting pool r/w (Zach Dykstra)
* 7b3dcc1 - zfs-chroot: add read-write prompt (Zach Dykstra)
* 09e4d76 - docs/man/zfsbootmenu: fix typo (Wesley H. Gimenes)
* 7532d1f - zfsbootmenu: protect stdout of main interface (Zach Dykstra)
* 6528649 - zfsbootmenu-help: use cat in place of less (Zach Dykstra)
* bf20728 - zfsbootmenu: make 'less' an optional binary (Zach Dykstra)
* d03f9de - Add dm-crypt to recovery images, so cryptsetup works (Andrew J. Hesford)
* b212b23 - zbm-builder.sh: add mount-options argument, document use (Andrew Gunnerson)

## ZFSBootMenu v2.2.0 (2023-05-21)

Perhaps the most significant change in ZFSBootMenu v2.2.0 is an overhaul of the project documentation. We are thrilled to welcome new member @classabbyamp to the project, who oversaw this substantial effort. Disparate documentation previously stored in Markdown files, the GitHub Wiki, POD files, and another git repository have now been combined into a single tree that is published on Read The Docs. The documentation is now maintained as an integral part of this repository and is published at https://docs.zfsbootmenu.org/. This includes:

* Man pages
* Run-time help pages
* Distribution installation guides
* Configuration and how-to documentation

Installation guides for Void Linux, Alpine, Debian Bullseye, Ubuntu 22.04 and openSUSE Leap are available, with similiar steps shared between the guides for easier maintenance and revision.

### New features

Kernel command-line handling for ZFSBootMenu and boot environments has received a revamp with the `zbm-kcl` tool. This comprehensive tool can modify the kernel command-line for both boot environments and for the embedded command-line in binary EFI releases. Refer to [zbm-kcl.8](https://docs.zfsbootmenu.org/en/v2.2.x/man/zbm-kcl.8.html) for additional usage details.

UEFI variable handling in ZFSBootMenu has now been normalized to be consistent between Dracut and mkinitcpio. When chrooting into a boot environment,  `efivarfs` is now mounted in the chroot. The filesystem is mounted read-only if the pool is read-only, and read-write if the pool is read-write. Additionally, if `efibootmgr` is present in the ZFSBootMenu initramfs, `efivarfs` is mounted read-write in the recovery shell.

To help users with high-DPI displays, ZFSBootMenu binary releases will now attempt to set the largest possible font size (up to 32 pixels) that ensures at least 100 text columns on the screen. Users who build their own custom ZFSBootMenu images can add [contrib/20-console-autosize.sh](https://github.com/zbm-dev/zfsbootmenu/blob/v2.2.x/contrib/20-console-autosize.sh) as an early-setup hook to achieve the same effect. Consult [zfsbootmenu.7](https://docs.zfsbootmenu.org/en/v2.2.x/man/zfsbootmenu.7.html) for instructions on configuring custom hooks.

The `generate-zbm` configuration key **Kernel.Version** now supports globs, such as `6.1.*` or `5.*`, to select the latest among a series of matching kernel versions for building a ZFSBootMenu image. The containerized build process now includes Linux versions 6.1, 5.15 and 5.10 and pre-compiled ZFS modules for each to simplify builds of custom images with recent LTS kernels.

### Fixes

A mutex to prevent infinite recursion when attempting to load keys for an encryption root is now restricted to the process that created it. This fixes an edge case where the interface launched from SSH would not be able to unlock an encryption root.

Artix is now a recognized distribution: ZFSBootMenu will specify the proper root argument for Artix boot environments.

### Significant commits in this release
* f688389 - zfsbootmenu: warn when canmount=on is set for root dataset (Zach Dykstra)
* aca5ae1 - zfsbootmenu/install-helpers.sh: hard depend on `chroot` (Andrew J. Hesford)
* dacce21 - contrib/zbm-sign.pl: add hook to sign ZBM EFI images for Secure Boot (Liam)
* da94578 - zsnapshots: recovery environment helper tool (Zach Dykstra)
* ddcee9d - generate-zbm: allow globs for kernel-version specs (Andrew J. Hesford)
* 989449c - zfsbootmenu: add zbm.kcl_override argument handler (Zach Dykstra)
* 534adcf - omit-drivers: blacklist intel screen orientation sensor (Zach Dykstra)
* 7f38b12 - zfsbootmenu-core: recognize Artix in root-prefix detection (Andrew J. Hesford)
* 84da18e - contrib/: expand font setting capabilities (Zach Dykstra)
* f78f723 - zfsbootmenu: manage efivarfs when needed (Zach Dykstra)
* c28a44f - zbm-kcl, zbm-efi-kcl: unify, allow streaming EFI modifications (Andrew J. Hesford)
* d73fad8 - UEFI: improve stub handling (Zach Dykstra)
* 5b61268 - releng: add kpartx and wipefs to recovery images (Zach Dykstra)
* c578ef5 - Key management: properly normalize Booleans, make cache mutex per-PID (Andrew J. Hesford)

## ZFSBootMenu v2.1.0 (2022-12-19)

### Deprecated features

* `syslinux` support as a core part of `generate-zbm` will be removed in the next release. [contrib/syslinux-update.sh](https://github.com/zbm-dev/zfsbootmenu/blob/master/contrib/syslinux-update.sh) should be used to create `syslinux.cfg` moving forward. Refer to the script for usage documentation.
* Awareness of platform endianness when writing `/etc/hostid` was rendered moot when support for `skim` was removed because `fzf` is only supported on little-endian systems. All hostid writes assume little-endian byte order by default.

### New features

* The command line option `zbm.prefer` has been extended with a `!!` marker to import exactly one pool when multiple are available on a system. Refer to [docs/man/zfsbootmenu.7.rst](https://github.com/zbm-dev/zfsbootmenu/blob/master/docs/man/zfsbootmenu.7.rst) for more details.
* Users of the binary releases are now able to use their own custom hooks with the new `zbm.hookroot` command line option. Using this, a partition and directory specification can be provided which allows for additional scripts to be loaded at runtime. Refer to [docs/man/zfsbootmenu.7.rst](https://github.com/zbm-dev/zfsbootmenu/blob/master/docs/man/zfsbootmenu.7.rst) for more details.
* [contrib/remote-ssh-build.sh](https://github.com/zbm-dev/zfsbootmenu/blob/master/contrib/remote-ssh-build.sh) has been provided by a new contributor. This script helps ease the creation of a custom EFI file with an embedded SSH server and keys.
* A global application header has been added to highlight the pages that are available and the currently selected page.
* Unless explicitly configured, `generate-zbm` will now default to `dracut` but fall back to `mkinitcpio` when it cannot find `dracut` in the path.
* The build container `ghcr.io/zbm-dev/zbm-builder` has been substantially improved, making it easier to manage custom images built in a controlled, compatible environment using podman or docker.

### Fixes

* On some systems, Dracut was incorrectly using `dash` where `bash` should be used. Forbidding the inclusion of `dash` with Dracut resolves this issue.
* The `drm` Dracut module has been blacklisted. ZFSBootMenu should never attempt to load firmware for video cards.
* When using ZFSBootMenu to pin a kernel, ensure that an anchor is attached to the end of the pin. This resolves incorrect matches on systems that have unversioned kernels.
* To avoid a potential conflict between ZFS pool names and files/directories created by ZFSBootMenu, all detected boot environment mountpoints have been moved to a dedicated `environments/` sub-directory.

### Significant commits in this release
* 8e6ca4a - set_default_kernel: properly clear default when no kernel is specified (Andrew J. Hesford)
* 0b40f44 - interface: enable left/right arrow key navigation (Zach Dykstra)
* 235eb17 - Stop installing zpool.cache (Andrew J. Hesford)
* 5427883 - Improve containerized builds (Andrew J. Hesford)
* c023517 - Automatically select initramfs generator when not forced (Andrew J. Hesford)
* 6223804 - Update artifact uploader to latest version (Zach Dykstra)
* 00ef434 - Add/use border label feature flag (Zach Dykstra)
* 562b14a - Fix booting test VMs on some hardware (Zach Dykstra)
* 8d3bbff - Fix kernel selection after introducing $BASE/environments (Alexander Lobakin)
* b743893 - contrib/README.md: document contrib scripts (Andrew J. Hesford)
* a3f15ed - generate-zbm: deprecate syslinux support (Andrew J. Hesford)
* 9f85003 - Include the OpenSSH client in recovery images (Zach Dykstra)
* 91900d3 - Explicitly require bash, blacklist dash (Andrew J. Hesford)
* 3399f4b - Move dataset mountpoint to $BASE/environments/ (Zach Dykstra)
* 7eb7780 - Allow imports of hooks from external sources at runtime (Andrew J. Hesford)
* 57b46a2 - Extend capabilities of zbm.prefer (Zach Dykstra)
* af1c74b - Remove diff from early days of playing around (Zach Dykstra)
* 3da5547 - Center key bind text on horizontal layout fzf screens (Zach Dykstra)
* df8ab05 - Support busybox as /bin/sh in chroot (Zach Dykstra)
* 2fa207e - fix: ensure $uefi_stub is defined before checking if it is a file (Zach Dykstra)
* 906ce3b - Add anchor to end of string when pinning a kernel (Zach Dykstra)
* 9c91afa - Make default efi path distro agnostic (gardar)
* 333b6d8 - Wrapper Build Script for ZBM with SSH access (Gerhard Roethlin)
* 05dbf11 - Show /etc/zbm-commit-hash in zreport (Zach Dykstra)
* d4b35a0 - zbm-build.sh: don't upgrade packages when installing custom software (Andrew J. Hesford)
* 10e3624 - Exploit common configurations for recovery/release images (Andrew J. Hesford)
* 1be8ed0 - Drop drm module from standard dracut config (Andrew J. Hesford)
* 9b57d8a - testing: move Ubuntu to 22.04 LTS, make column available to Debian/Ubuntu (Andrew J. Hesford)
* feb6a26 - documentation: provide a link to the wiki (Zach Dykstra)
* f2d3336 - Add a contrib script to snapshot the BE prior to boot (Zach Dykstra)

## ZFSBootMenu v2.0.0 (2022-06-28)

ZFSBootMenu 2.0.0 introduces a major internal reorganization that allows images to be built with initramfs generators other than dracut and includes some helpful command-line utilities. This release is based on Linux 5.10.125 and ZFS 2.1.5.

### New features

* Dracut is now optional; ZFSBootMenu images may currently be built with mkinitcpio
* Generalizations to support mkinitcpio should also apply to, *e.g.*, initramfs-tools, although installation hooks for initramfs-tools have not (yet) been written
* A new utility, `zbm-efi-kcl`, provides the ability to edit the kernel command-line embedded in a ZBM EFI bundle rather than requiring regeneration of the bundle
* A new utility, `zbm-builder.sh`, provides a simple interface for creating custom, local images using the official ZBM build container; it is now possible to build local images without installing ZBM or its Perl dependencies

### Fixes

* Video drivers are now omitted from images by default to avoid GPU initialization issues in the final boot environment
* General fixes to shell functions were made in support of using busybox in mkinitcpio images
* The online command-line editor now provides an option to revert to the default

### Significant commits in this release

* 8933a57 - Basic EFI KCL editor (Zach Dykstra)
* ada36c8 - Include cryptsetup in recovery images (Zach Dykstra)
* f1b0270 - Include cryptsetup in containarized builds (Grzegorz Uriasz)
* fd52d7c - Omit video drivers by default (Zach Dykstra)
* 65dd0cd - Add option to revert to default KCL via ctrl-t in draw_be (Zach Dykstra)
* 632b24f - Add special escape sequences to PS1 (Zach Dykstra)
* 3d9f57b - Build recovery images in CI/CD (Zach Dykstra)
* 9368171 - Accept any root= value as valid (Zach Dykstra)
* 7982971 - zbm-builder.sh: support custom build/source paths and hooks (James R. Todd)
* fc83894 - Document scrolling keys in help viewer (Zach Dykstra)
* 6f09d39 - Fix broken links (Jip de Beer)
* 64936f3 - Add minimal documentation to ESP sync hook (Zach Dykstra)
* d559458 - Add BUILD.md, a quick-start containerized build guide (James Todd)
* 1aca5d1 - Bug fixes and use local repository option for zbm-builder.sh (James Todd)
* 8b1aade - Make the emergency shell slightly fancy (Zach Dykstra)
* 5a5ab0b - Write data to console to recalculate size (Zach Dykstra)
* eac0547 - install-helpers.sh: explicitly require setsid (Andrew J. Hesford)
* dbd9642 - Replace 'tr' with bash string manipulation (Zach Dykstra)
* 8916a10 - Update luks-unlock.sh to work with current libraries (Zach Dykstra)
* 33883e5 - zfsbootmenu-core.sh: drop preload_be_cmdline, improve KCL caching (Andrew J. Hesford)
* 7548af5 - zbm-kcl: remove dracut from ZBM_MODULEDIR warning (Andrew J. Hesford)
* 306f36b - Fix install location for common module files (Zach Dykstra)
* 644b0f1 - Use an early-setup hook to force console init in release images (Andrew J. Hesford)
* ec89a7c - Use column to layout footer (Zach Dykstra)
* 2bd6757 - generate-zbm: remove INI migration support (Zach Dykstra)
* 65167e6 - Define all ZBM requirements in install-helpers.sh (Andrew J. Hesford)
* 812f9b4 - Remove support for deprecated KCL sources (Andrew J. Hesford)
* 3dcaa88 - Improve container-based builds and support mkinitcpio images (Andrew J. Hesford)
* f93687f - Annotate mkinitcpio.conf to explain ZBM specifics (Andrew J. Hesford)
* 91b4b3b - De-prioritize Dracut as the primary generator (Zach Dykstra)
* af34fa9 - Support building mkinitcpio images in generate-zbm (Andrew J. Hesford)
* 50a2da4 - General fixes to support busybox in mkinitcpio images (Andrew J. Hesford)
* 1fe8c4c - Add support for mkinitcpio (Andrew J. Hesford)
* e1bd708 - zbm-builder.sh: add script to build custom images using GHCR container (Andrew J. Hesford)
* 20a8ac7 - Add manual page for zbm-kcl (Andrew J. Hesford)


## ZFSBootMenu v1.12.0 (2022-01-25)

This release brings multiple changes to how ZFSBootMenu works at run-time. These changes were introduced in an effort to:

1. Reduce the ZFSBootMenu startup time,
2. Remove dependencies on Dracut-specific helper functions, and
3. Make ZFSBootMenu more modular and more easily maintained.

Prior to this release, the only Dracut helper functions in use were those that retrieve command-line arguments to dynamically configure ZFSBootMenu. These have all been replaced by internal functions that are both more correct and roughly an order of magnitude faster.

With the help of the [flamegraph](https://github.com/brendangregg/FlameGraph) visualization tool, multiple unnecessary Dracut modules have been pruned from the binary releases. The net effect of these changes is a large decrease in the time spent booting to either the menu or directly to a boot environment.

ZFSBootMenu v1.12 is expected to be the last release series before ZFSBootMenu v2.0. The internal changes and dramatically reduced dependence on Dracut will allow ZFSBootMenu v2.0 images to be built using either dracut or mkinitcpio.

### Deprecated features

* Support for `skim` has been removed. ZFSBootMenu now requires `fzf` for all menu functionality.
* Support for using `/etc/default/grub` and `/etc/default/zfsbootmenu` for boot environment kernel command line parameters will be removed in the next release.

### Fixes

* Use `kexec --kexec-syscall-auto` to try multiple different ways to load a kernel
* Change the release files names to better indicate to rEFInd that they are Linux kernels
* `root=` is always removed/suppressed when passing a commandline into the boot environment
* More kernel commandline parameters are validated for correctness

### New feaures

* Add the `zbm-kcl` userland tool to view and edit boot environment kernel command lines
* `generate-zbm` can now execute user hooks before and after an initramfs or EFI bundle has been created
* Teardown hooks now have access to variables indicating which boot environment, kernel and initramfs were selected
* We now provide `recovery` builds that include networking and a few disk-related tools
* Buildah is now used to create build images
* Countdown timers now have colored text by default

###  Significant commits in this release

* bd736f1 - Add a shell utility to modify and review KCL properties (Andrew J. Hesford)
* 466e4d2 - Allow rollbacks from the snapshot menu (Andrew J. Hesford)
* b9c4ed9 - Add Alpine to the ZBM test suite (Andrew J. Hesford)
* 2c8eacc - Add support for early/late hooks in generate-zbm (Zach Dykstra)
* 1ca3cf4 - Define BE selection variables in teardown hook environment (Andrew J. Hesford)
* 89b6273 - Enable profiling framework in core ZFSBootMenu tools (Zach Dykstra)
* 46b0eb9 - Completely rewrite KCL handling (Andrew J. Hesford, Zach Dykstra)
* 4b836f3 - Generate release and recovery image builds (Zach Dykstra)
* 6789ee2 - Provide, and use, a buildah script to construct zbm-builder images (Andrew J. Hesford)
* 70bad38 - Add initial boot-environment guide (Andrew J. Hesford)
* 4b836f3 - Generate release and recovery image builds (Zach Dykstra)
* 044df51 - Separate core library routines from main UI routines (Andrew J. Hesford)

## ZFSBootMenu v1.11.0 (2021-10-31)

### Updated defaults

* The chroot hot-key has been changed to `MOD+J` to avoid a key conflict with the usage of `CTRL+I`.

### Deprecated features

* Support for reading the KCL from /etc/default/zfsbootmenu or /etc/default/grub will be removed in a future release. During the transition period, ZFSBootMenu will attempt to automatically convert a deprecated KCL configuration to the org.zfsbootmenu:commandline property.

### Fixes

* The Makefile now correctly installs all files (this fixes the missing help pages)
* Make the width of the count down menu consistent as digits are dropped from the timer
* Remove a spurious function call in the preview renderer
* Where possible, prefer built-in Bash regex support over forking to grep
* Remove uses of basename, prefering Bash string manipulation
* Properly find the zpool executable when creating an initramfs
* Set a consistent environment for SSH and recovery shells
* Set default columns and rows for serial consoles when none are provided
* Improve the reliability of detecting which libgcc_s.so to install
* Set keylocation and a corrected encryption root for full-copy clones

### New features

* Allow taking over a running instance of ZFSBootMenu via `zbm`
* Create a docker/podman image, based on Void Linux, that can be used to create ZFSBootMenu images
* Allow diffing a snapshot with another snapshot, instead of just with the current state of the filesystem
* Add early hooks, just after kernel modules have been loaded but before any pools have been imported
* org.zfsbootmenu:keysource can now be an arbitrary ZFS filesystem instead of another boot environment
* Snapshots can now be created, providing a starting point for a new boot environment
* `zreport` is available in the recovery shell, which can provide details-at-a-glance for bug reports
* The `testing/` infrastructure has been updated to allow the installation of multiple distributions in one command
* The Debian testing environment has been updated to Bullseye
* EFI bundles can be built and tested under `testing/`

### Significant commits in this release
* 32f138f - Try to inherit key (encryptionroot) in duplicate_snapshot (Andrew J. Hesford)
* 357aa5e - Make keylocation of target match that of source in duplicate_snapshot (Zach Dykstra)
* c048ead - testing: move debian to bullseye (Andrew J. Hesford)
* 7bb154d - testing: allow installation of multiple distros in one setup run (Andrew J. Hesford)
* 8fab2b5 - Rework GitHub actions to also build assets on push (Zach Dykstra)
* 8922f87 - Add zreport function (Zach Dykstra)
* f0d402c - Support creating EFI bundles in run.sh, along with booting them. (Zach Dykstra)
* 2c58479 - Improve search for libgcc_s in module-setup.sh (Andrew J. Hesford)
* 726894f - Default stty cols/rows values for serial consoles (Zach Dykstra)
* 78656b0 - Small fixes for SSH usage (Zach Dykstra)
* 0e14c7b - Update zfsbootmenu.7 to reflect new org.zfsbootmenu:keysource behavior (Andrew J. Hesford)
* 891e44a - Allow snapshot creation / move logic to function (Zach Dykstra)
* d208103 - Make cache_key aware of mountpoints on key sources (Andrew J. Hesford)
* 2d03ff5 - Remove explicit path to zpool ldd check (Zach Dykstra)
* 29a1049 - Deprecate KCL in /etc/default/{zfsbootmenu,grub} (Andrew J. Hesford)
* c2a1bee - Support early hooks, add example luks-unlock.sh (Zach Dykstra)
* 62f7315 - Enable snapshot <> snapshot diffing (Zach Dykstra)
* c920c76 - Replace mod+i with mod+j (Zach Dykstra)
* a79b9d0 - zkexec: recovery shell arbitrary kexec wrapper (Zach Dykstra)
* 8d4d776 - Makefile: recursively install dracut module (Andrew J. Hesford)
* c1feb56 - recovery shell quality of life improvements (Zach Dykstra)
* 5876ca9 - Create a global debug/trace log viewer (Zach Dykstra)
* 83b1766 - Overhaul, generalize containerized build scripts (Andrew J. Hesford)
* cfa0455 - releng/make-binary.sh: use containerized image builds (Andrew J. Hesford)
* 4166967 - Add Dockerfile and Compose service to build ZBM images (Thomas Oster)
* 687295a - Allow zfsbootmenu to takeover a running instance (Zach Dykstra)
* 9000687 - Start adding aliases for missing niceties (Zach Dykstra)
* 804ef02 - Make sure feature flags are available everywhere (Zach Dykstra)
* 739540a - Error out in testing if machine type isn't known to run.sh. (Érico Nogueira Rolim)

## ZFSBootMenu v1.10.1 (2021-07-04)

ZFSBootMenu 1.10.1 brings a pair of fixes to issues seen in the wild. 

### Fixes

* Remove a spurious warning when `generate-zbm` incorrectly detected a mismatch between `/etc/hostid` and the run-time `spl_hostid` SPL module parameter.
* Fix a race condition between ZFSBootMenu and udev when attempting to load ZFS/SPL kernel modules, resulting in an incorrect drop to a recovery shell. 

### Significant commits in this release

* 2a9a6ab - Relax insmod spl failures, try to load zfs.ko (Zach Dykstra)
* f657717 - generate-zbm: fix hostid comparison (Andrew J. Hesford)

## ZFSBootMenu v1.10.0 (2021-06-27)

ZFSBootMenu 1.10.0 brings some minor new features and some behavior changes that should improve the booting and configuration experience. Notably, some default behaviors have changed in this release. Read on for details about how this may impact your configuration.

### Updated defaults

Previous releases have had `zbm.import_policy=strict` and `zbm.set_hostid=0` set by default. Starting with this release, the default values are `zbm.import_policy=hostid` and `zbm.set_hostid=1`. `zbm.import_policy=hostid` can help ZFSBootMenu automatically and safely import a pool when the wrong hostid is provided. `zbm.set_hostid=1` passes the hostid used to import the pool to the boot environment, ensuring that it can also correctly import the pool.

Please refer to [Command-Line Parameters](https://github.com/zbm-dev/zfsbootmenu/blob/master/docs/man/zfsbootmenu.7.rst#command-line-parameters) for a compete description of both of these feature flags.

### Fixes
* When no hostid is provided or discoverable, use `0x00bab10c` instead of `0x0`.
* Force SPL to use `/etc/hostid` and always ensure that a valid hostid is stored in the file.
* Blacklist Plymouth; the splash screens it draws interferes with the ZBM interface.
* Persist runtime configuration to a file that is used when the interface is launched normally or through SSH.

### New features
* When exiting the diff browser, exit back to the snapshot list with the snapshot selection preserved.
* Set a hostname in ZFSBootMenu so that it shows up in the pool history for read/write operations.
* When generating an initramfs, warn if `/etc/hostid` doesn't match `spl_hostid` provided as a module parameter to SPL.
* Support `console=` kernel parameters with a `,speed` suffix. This is normally used when setting a serial port as the machine console.
* Add a shortcut key to remove the pinned kernel value for a boot environment.

### Allow references to parent properties in org.zfsbootmenu:commandline

Any reference to `%{parent}` in `org.zfsbootmenu:commandline` will be replaced with the value of the same property on the parent filesystem (with parent references above recursively expanded), allowing easy specification of common options at a mutual parent of two BEs and overrides or additions of individual options per-BE. The value of `%{parent}` is always an empty string on a root filesystem.

This is not intended to be sophisticated, and `%{parent}` appearing within other words will be replaced regardless. The assumption is that `%{parent}` is unique enough and will not conflict with real KCL options, so dumb global replacement is sufficient.

### zpool import process improvements

The existing `zbm.prefer` option has been extended to support defining a mandatory pool. Append `!` to the pool name to indicate that the specific pool MUST be imported before any other pool imports will be attempted.

* `zbm.prefer=zroot!` will require that `zroot` be imported on boot.

Between pool import attempts, `zbm.import_delay` (default of 5 seconds) controls how long to pause. During this delay window, the escape key can be used to access a full recovery shell.

Either one of `spl_hostid` or `spl.spl_hostid` can be provided on the ZFSBootMenu kernel command line, in either hex or decimal format. The parameter value is checked to ensure that it's either valid hex or decimal, and then normalized to an 8 digit hex value.

Additional steps have been taken to ensure that SPL can be loaded if an invalid `spl.spl_hostid` value is provided on the kernel command line. A more strict test is now used to determine if the ZFS kernel module has been loaded and drop to a recovery shell if not.

### Significant commits in this release
* c759df9 - For musl/non-dracut compat, change default hostid to non-zero value (Zach Dykstra)
* 8886db3 - Force spl.spl_hostid=0 when matching hostid (Andrew J. Hesford)
* 24e6e6a - Blacklist plymouth; it directly conflicts with how we manage the tty (Zach Dykstra)
* 9875115 - Store options from KCL in a file for easy sourcing (Andrew J. Hesford)
* 98f3164 - Use fzf's execute[] function to render draw_diff (Zach Dykstra)
* 64bd359 - Set a hostname in ZBM, check if spl.spl_hostid matches /etc/hostid (Zach Dykstra)
* 40dd0e3 - Small quality of life improvements (Zach Dykstra)
* 635d140 - Add keyboard shortcut to remove pinned kernel (Zach Dykstra)
* 04f5b87 - Allow references to parent properties in org.zfsbootmenu:commandline (Andrew J. Hesford)
* 5142aa1 - Support pool import retries (Andrew J. Hesford)
* e2caa81 - Improved pool imports (Andrew J. Hesford)
* 31cc1b3 - Validate spl_hostid, control loading spl.ko (Zach Dykstra)
* ccfc92c - Change ZBM hostid handling defaults (Zach Dykstra)

## ZFSBootMenu v1.9.0 (2021-03-29)

This release is dedicated to the late Jürgen Buchmüller (@pullmoll), a major contributor to the Void Linux project. Although ZFSBootMenu strives to support as many Linux distributions as possible, Void Linux is the distribution of choice for the entire ZFSBootMenu team. We have benefited greatly from pullmoll's enduring commitment to Void Linux. He will be missed.

### Fixes

* Snapshot duplication now carries over ZFS properties from the source (cloning has always copied properties)
* When forcing a pool read-write via MOD+W, the screen is now cleared before a password prompt
* Prompts in Arch/Ubuntu/Debian chroots are now correctly set
* Build-time checks for version-specific flags in fzf/sk/dmesg improve compatibility with older versions found on Debian and Ubuntu
* Add `stty` to the list of required binaries
* When `generate-zbm` is executed with the `--debug` flag, `-q` is no longer passed in to Dracut
* When possible, try to let Dracut determine the path to the EFI stub file. The path can be explicitly specified by setting `EFI.Stub` in `config.yaml`
* Update the list of allowed characters in a ZFS filesystem name; removing `,` and adding `:`
* Use the Dracut built-in `inst_rules` to install udev rules, instead of hard-coding a file path
* Correctly set a `root=` prefix for Gentoo systems
* Improve kernel version detection in unversioned file names
* Update documentation to clarify driver exclusions and teardown hooks

### Major New features

### `hostid` configuration assistance
A pair of new features have been developed to combat the delicate dance sometimes required to synchronize `hostid` in the boot environment (BE), the initramfs for the BE, and the initramfs for ZFSBootMenu. Both of these are controlled by ZFSBootMenu kernel command line options.

Set `zbm.import_policy=hostid` to allow run-time reconfiguration of the SPL hostid. If a pool is preferred via `zbm.prefer` and the pool can not be imported with a preconfigured hostid, the system will attempt to adopt the hostid of the system that last imported the pool. If a preferred pool is not set and no pools can be imported using a preconfigured hostid, the system will adopt the hostid of the first otherwise-importable pool. After adopting a detected hostid, ZFSBootMenu will subsequently attempt to import as many pools as possible.

Setting `zbm.set_hostid` will cause ZFSBootMenu to set the `spl.spl_hostid` command-line parameter for the selected boot environment to the hostid used to import its pool. The SPL kernel module will use this value as the hostid of the booted environment regardless of the contents of `/etc/hostid`. As a special case, if the hostid to be set is zero, ZFSBootMenu will instead set `spl_hostid=00000000`, which should be used by dracut-based initramfs images to write an all-zero /etc/hostid in the initramfs prior to importing the boot pool.

### Minor new features

### Boot environment / snapshot sorting
The main boot environment screen and snapshot list screen can now be sorted by multiple criteria. By default, these listings are sorted by name. One of `zbm.sort_key=name`, `zbm.sort_key=creation`, or `zbm.sort_key=used` can be set on the command line to change the default used. `MOD+O` can be pressed at run time to change the sort order to the next in the list.

### Helper functions in the recovery shell

The internal `zfsbootmenu-lib.sh` library is now sourced by default in the recovery shell. This makes a number of helper functions available for general use.

### Combined command line printer

Since `Dracut` can find command line options from multiple places, it can be difficult to determine which option was used to control bootloader behavior. The command `zbmcmdline` will show the command line as seen by Dracut.

### Shorcut key layout improvements

Instead of flowing the helper key text to the width of the screen, shortcut key text is now arranged in one or more columns. Low resolution screens or terminals with small dimensions will use a single column to show the text. As the terminal dimensions increase, text will be laid out in up to a maximum of three left-aligned columns.

### Logging system

ZFSBootMenu has an internal logging system backed by `/dev/kmsg` and `dmesg`. Logging is now enabled throughout the entire system, tracking both `debug` level messages as well as `warnings` and `errors`. When a warning or error condition has occurred, a yellow `[!]` or red `[!]` notification will appear in the upper left of the screen. Pressing `MOD+L` will access this logging system, allowing you to easily see these messages.

To aid in debugging issues, debug logging has been added in all of the internal library functions. These messages can be enabled by setting `loglevel=7` on the ZFSBootMenu command line.

### Binary release for x86_64

Binary releases in the form of a standalone EFI file and a kernel/initramfs pair for `x86_64` will now be made available with each future tagged release. To allow for the most compatiblity with the myriad system configurations out there, a few features will be embedded in the builds:

* `zbm.import_policy=hostid`
* `zbm.set_hostid`
* [xhci-teardown.sh](https://github.com/zbm-dev/zfsbootmenu/blob/master/contrib/xhci-teardown.sh)

The EFI binary can be used as a recovery tool by naming it `BOOTX86.EFI` and adding it to an `EF00` partition on a USB drive. It can also be used as a drop-in bootloader for your system without needing to locally build a copy. See [UEFI Booting](https://docs.zfsbootmenu.org/en/latest/general/uefi-booting.html#booting-the-bundled-executable) for example `efibootmgr` commands.


### Other changes

Some command line options have now been deprecated with the inclusion of `zbm.import_policy`. See [zfsbootmenu(7)](https://github.com/zbm-dev/zfsbootmenu/blob/master/docs/man/zfsbootmenu.7.rst#deprecated-command-line-parameters).

### Testing improvements

* Ability to install Arch / Ubuntu / Debian / Void / Void-musl on ZFS, in a VM, with a single command
  * This is huge.
* Verify that ZBM builds on Arch / Ubuntu / Debian / Void / Void-musl
* Verify that ZBM built on any distro boots any other distro
* Improvments to `run.sh` and `setup.sh` testing scripts to simplify usage

### Significant commits in this release
* fadfd0f - Use --props and --raw when send | recv duplicating (Andrew J. Hesford)
* 0a4b1f7 - Use signify to sign binary assets (Andrew J. Hesford)
* 98cfc69 - Show if BE is encrypted, add path to chroot prompt (#157) (Zach Dykstra)
* 848bfc9 - Do not drop to emergency shell if zbm.preferred cannot be imported (Andrew J. Hesford)
* fd94635 - Fix PS1 in arch/debian/ubuntu, set custom prompt in emergency shell (#151) (Zach Dykstra)
* dee6228 - Support older versions of fzf/sk/dmesg (Zach Dykstra)
* 6e28f42 - Non-zero return from chroot should be debug info, not an error (Andrew J. Hesford)
* 82ab9a7 - Add `stty` to required exectables in module-setup.sh (Andrew J. Hesford)
* f962f50 - Use configurable destinations for all paths in Makefile (Andrew J. Hesford)
* 03a38b5 - Column view for help key footers (Andrew J. Hesford)
* 951111e - Add support to discover and assume a hostid, as well as fix arguments passed to a BE (#147) (Zach Dykstra)
* 29cf15c - Logging lifecycle (#146) (Zach Dykstra)
* 629346f - Automagically source zfsbootmenu-lib in emergency shell (Zach Dykstra)
* 73dc5c2 - Move more helper functions to -lib, cleanup scripts (#144) (Zach Dykstra)
* 44ed892 - Allow BE and snapshot sorting by different criteria  (#143) (Zach Dykstra)
* 9afa8b4 - comma is invalid, colon is valid (Zach Dykstra)
* a99a8de - Update README.md to explain driver exclusions and teardown hooks (RoundDuckKira)
* 5ac481f - Use inst_rules to install rule programs (Witaut Bajaryn)
* 6cf753f - Use genkernel root prefix by default on Gentoo (#139) (Witaut Bajaryn)
* a6e02c0 - Only specify `--uefi-stub` when EFI.Stub is configured (Andrew J. Hesford)
* 893cbe9 - Improve kernel version detection. (Andrew J. Hesford)
* 8db6d8f - README: Describe more Perl dependencies (#134) (Witaut Bajaryn)

## ZFSBootMenu v1.8.1 (2021-01-12)

Happy New Year! ZFSBootMenu 1.8.1 provides a few minor enhancements and bug fixes.

### Fixes
* Properly handle encryption keys as raw devices rather than normal files. (#127)
* Improve detection of latest kernels when `/boot` contains some unversioned kernel files. (#128)
* Accept `Ctrl` and `Ctrl-Alt` as hotkey modifiers in addition to `Alt` to fix issues with some non-US keymaps. (#124)
* Everywhere a chroot hotkey is offered, use the same hotkey.

### New features
* Add hard wraps to the hotkey menus to improve clarity; ignored for small screens.
* In the snapshot list, add an option to jump into a read-only chroot for that snapshot.
* The `force_import` and `timeout` kernel command-line options are now expected to be `zbm.force_import` and `zbm.timeout`; the old forms are deprecated but will continue to work for the forseeable future.
* New `zbm.show` and `zbm.skip` command-line options force the menu to appear or be skipped if `zbm.timeout` is not also set.

### Significant commits in this release
* ab95346 - Expect names of the form <prefix>-<version> when finding latest kernel (Andrew J. Hesford)
* 798fce8 - Normalize tests for paths. (Andrew J. Hesford)
* d02865e - Change chroot keys, add help text (#126) (Zach Dykstra)
* 83c58b2 - Support Ctrl and Ctrl-Alt in addition to Alt as keybind modifiers (Andrew J. Hesford)
* 70c3d70 - Namespace our KCL args, organize parsing (#120) (Zach Dykstra)
* c1d6ce5 - Support chroot'ing into a snapshot (Zach Dykstra)
* 7a63beb - Support for hard wrap points in header_wrap (Andrew J. Hesford)

## ZFSBootMenu v1.8.0 (2020-12-15)

ZFSBootMenu 1.8.0 offers a significant list of new features, fixes and general improvements.

### Fixes
* When duplicating snapshots, the process can now be interrupted with SIGINT (Ctrl-C).
* Availability of sufficient free space is confirmed before attempting snapshot duplication.
* The `generate-zbm` command now ensures that the target directory for boot images has sufficient space, rather than copying partial and generally broken files.
* Because ZFSBootMenu never modifies filesystem contents, ZFS filesystems are always mounted read-only, even if the pool is writable.
* Changes to the handling of encryption keys correctly handle some corner cases, such as duplicating a snapshot of a filesystem with a different encryptionroot than its parent.

### New features
* Colored text and timed messages are used to bring emphasis to important messages presented by ZFSBootMenu.
* Extensive logging to the kernel ring-buffer has been enabled throughout ZFSBootMenu, with verbosity controlled by the `loglevel` kernel-command-line argument.
* Much of the core menu functionality has been separated into a standalone `zfsbootmenu` program on the initramfs, allowing the menu to be accessed over SSH using something like the [dracut-crypt-ssh](https://github.com/dracut-crypt-ssh/dracut-crypt-ssh) module.
* Boot images can be configured to include [tmux](https://github.com/tmux/tmux), allowing the boot menu to be presented in a multi-pane, detachable view. This is primarily aimed at development/debugging efforts. See descriptions of the `zbm.tmux` command-line option and the `zfsbootmenu_tmux` dracut option in the [zfsbootmenu(7)](https://github.com/zbm-dev/zfsbootmenu/blob/master/docs/man/zfsbootmenu.7.rst) manual page.
* ZFSBootMenu can now execute arbitrary, user-supplied "setup" hooks before the menu is displayed and "teardown" hooks immediately before jumping into a selected boot environment. See descriptions of the `zfsbootmenu_setup` and `zfsbootmenu_teardown` dracut options in the [zfsbootmenu(7)](https://github.com/zbm-dev/zfsbootmenu/blob/master/docs/man/zfsbootmenu.7.rst) manual page.
* Encryption keys can now be cached by pointing the `org.zfsbootmenu:keysource` property of a ZFS encryptionroot to a specific filesystem. When ZFSBootMenu attempts to load a key from a `file://` location, it will first attempt to load the key at that location relative to the filesystem specified by `org.zfsbootmenu:keysource`. If this succeeds, ZFSBootMenu will retain a copy of the key in the initramfs so that subsequent need for the key (for example, when re-importing a pool read-write to set default boot options or duplicate a snapshot) will not require re-entry of the passphrase. See descriptions of the `org.zfsbootmenu:keysource` ZFS property in the [zfsbootmenu(7)](https://github.com/zbm-dev/zfsbootmenu/blob/master/docs/man/zfsbootmenu.7.rst) manual page.
* The `Alt+C` hot key provides the means to chroot into a selected boot environment. If the pool is mounted read-write, the chroot will be writable, allowing recovery operations directly from ZFSBootMenu.

### Significant commits in this release
* 8ffe139 - Add a keybind for zfs-chroot, rework script with -lib in mind (Zach Dykstra)
* 2c3e9b5 - Support configurable, opt-in caching of key files (Andrew J. Hesford)
* 45d0066 - Add zlog() logging helper, along with debug-level logging (Zach Dykstra)
* 47aa434 - Add support for richer setup and teardown hooks (Andrew J. Hesford)
* 88818a9 - Support optional "teardown" script to run before kexec (Andrew J. Hesford)
* 2045315 - Optionally launch under tmux (Zach Dykstra)
* 7f71d4f - Mount ZFS filesystems readonly (Andrew J. Hesford)
* d559b96 - Improve key handling (Andrew J. Hesford)
* ee468aa - Initial split of core menu logic from initialization (Andrew J. Hesford)
* 25baa48 - Log dracut command, add man page documentation (Zach Dykstra)
* 3f02b8f - Prevent copying of partial files when target volume is full (Andrew J. Hesford)
* 258d18a - Merge fuctionality of createInitramfs and unifiedEFI (Andrew J. Hesford)
* 4b6e9ae - Add subroutine for debug logging (Zach Dykstra)
* 82bbdc4 - Rely on local IFS override instead of global changes (Zach Dykstra)
* 402474b - Make alt-w a toggle between R/O and R/W imports (Andrew J. Hesford)
* 953c724 - Richer color handling in timed_prompt (formerly warning_prompt) (Andrew J. Hesford)
* eb39ea6 - Show countdown in warning_prompt, use to display auto-boot countdown (Zach Dykstra)
* f10e7e8 - Rough avail space validation in duplicate_snapshot (Zach Dykstra)
* bc0e117 - Move duplicate to sub shell, exit on sigint (Zach Dykstra)

## ZFSBootMenu v1.7.1 (2020-11-18)

This is a minor bug-fix release.

### Fixes
* When ZFSBootMenu fails to import any usable pools on startup and drops to an emergency shell, the user can now manually import a pool if possible and exit the shell to attempt to continue the boot process. Previously, a reboot was required to retry the boot process.
* An oversight in the loading of encryption keys caused a harmless error message to be displayed above the password prompt. This oversight has been fixed and the error no longer appears.
* Changes made to the handling of `/etc/hostid` in the ZFSBootMenu dracut module in anticipation of similar changes in the upcoming OpenZFS 2.0.0 release. This caused inconsistent behavior on systems using the musl C library with current versions of ZFS on Linux, resulting in potentially unbootable systems without forcing the `spl_hostid` command-line parameter. Now, the ZFSBootMenu dracut module attempts to discover the installed version of ZFS and behave consistently.

### Significant commits in this release
* 940cd4c - Fall back to legacy hostid creation for ZFS < 2.0 (Andrew J. Hesford)
* 6cc0076 - Fix key_wrapper calls with out CLEAR_SCREEN defined (Zach Dykstra)
* 65a1a33 - Loop the emergency shell when initial pool imports fail (Andrew J. Hesford)

## ZFSBootMenu v1.7.0 (2020-11-15)

In addition to a bug fixes, this release targets refinements that improve usability and offer contextual help within the menus.

### Fixes
* ZFSBootMenu now respects the `console` kernel command-line option and should behave as expected over a serial console.
* Command lists at the bottom of each menu are now sensibly wrapped to the terminal width, with extra coloring to highlight key combinations.
* Rather than rely solely on the `hostid(1)` command to populate the default hostid in the ZFSBootMenu initramfs, the dracut module will prefer to copy the `/etc/hostid` from the host, which should produce more consistent behavior on musl systems.
* Boot environments are now explicitly sorted, with the default boot environment appearing at the top of the list and selected by default.

### New features
* An online help system, accessible from `alt-h` within any menu, provides descriptions of functionality provided by ZFSBootMenu.
* The description at the top of the menu now indicates whether the selected boot environment is on a pool currently imported readonly or writable.
* New command-line arguments `zbm.lines` and `zbm.columns` allow the size of the terminal at boot time.
* When `generate-zbm` fails to parse the YAML configuration, more detailed messages pinpoint parsing errors.

### Significant commits in this release
* dbe91a1 - Fix console handling when attached to a serial line (Andrew J. Hesford)
* 9959d10 - Respect ZFS hostid behavior on musl (Andrew J. Hesford)
* f4a60e6 - Capture and print config.yaml eval failure (Zach Dykstra)
* 2ebad45 - Sort environments, fix preview (Zach Dykstra)
* cc6e27c - Control size/target of ZFSBootMenu output (Zach Dykstra)
* bb294b3 - Enable dynamic line wrapping for header (Zach Dykstra)
* 8993591 - Enable global help system (Zach Dykstra)
* 7879876 - Read-only helpers (Zach Dykstra)

## ZFSBootMenu v1.6.1 (2020-10-27)

Revert omitting `rootfs-block` by default from the ZFSBootMenu initramfs. `rootfs-block` is a hard requirement of `crypt`, which is used to setup LUKS beneath ZFS.

## ZFSBootMenu v1.6.0 (2020-10-24)

This release brings significant improvements to the pool import process. Previously, all available pools were discovered and then their health was scraped to confirm that they were in an 'ONLINE' state before importing them. This process had a few subtle shortcomings that were highlighted by the pending release of OpenZFS 2.0.0. In particular, if a zpool has been upgraded via `zpool upgrade` to enable OpenZFS 2.0.0 feature flags, but the ZFSBootMenu initramfs contains an older version of OpenZFS, the pool was not able to be automatically imported in ZFSBootMenu. The import process now relies on `zpool import -N -a -o readonly=on` to attempt to import all available and otherwise healthy pools in read-only mode. By using zpool itself to determine all of the pools that can/should be imported, ZFSBootMenu now avoids the fragile process of scraping and interpreting the human-friendly text output of `zpool import`.

### Fixes
* Improve the reliability of returning the correct kernel command line at all times. A helper function can now return corrected arguments in cases where `resume` is on the command line, but the pool has been imported read-write. This function is now used when generating a preview, making the modification more transparent to user inspection.
* Omit systemd and other modules from ZFSBootMenu by default. These install hooks that interfere with the correct operation of ZFSBootMenu.
* The testing/virtualiztion frame work has received a lot of attention during this development cycle. This has allowed us to create a variety of pool configurations that would be otherwise difficult to accomplish with physical hardware.

### New features
* When accepting user input (new filesystem name, resume protections), allow CTRL-C to be used to cancel the process.

### Significant commits in this release
* 219a632 - Rewrite pool import ahead of OpenZFS 2.0.0 (Zach Dykstra, Andrew J. Hesford)
* fb866c8 - Use zfsbootmenu-input in noresume prompt (Andrew J. Hesford)
* 1bb4f41 - Allow ctrl-c to be used during user input (Zach Dykstra)
* ca8eda8 - use -F to display the file type for more filtering options (Zach Dykstra)
* 08f22e2 - Simplify handling of BE command lines, use in previews (Andrew J. Hesford)
* cd8f889 - omit rootfs-block; it will never generate a correct KCL (Zach Dykstra)
* c96165c - Omit systemd related modules (Zach Dykstra)

## ZFSBootMenu v1.5.0 (2020-09-16)

### Fixes
* The required binaries were audited, cleaning up tools that were no longer used.
* The installation of required binaries, core scripts and other files is checked during initramfs creation. If any of these critical files can not be installed, the image is not created.
* Parameters are passed to the pool import function to control how a pool is imported. This allows for more than just readonly changes when importing.
* `config.yaml` defaults were adjusted to more closely match normal use cases. This works in tandem with the `--enable` toggle in `generate-zbm` to provide a better out-of-the-box experience.

### New features
* When importing a pool as R/W, and a resume image is found, the kernel command line is modified to remove `resume=` and to subsequently append `noresume`.
* Support `skim` in place of `fzf` for greater platform availability. `fzf` is preferred when creating an initramfs.
* The health of discovered pools can now be viewed. Optionally, a checkpoint rewind can be performed if one has been set. Use caution, this action can NOT be undone.
* Global image creation can be toggled via `generate-zbm --enable` or `generate-zbm --disable`

### Significant commits in this release
* cfec416 - Support automatic "noresume" when importing pools R/W (Andrew J. Hesford)
* a6a36bf - Add support for pool status and checkpoints (Zach Dykstra)
* 79bcca3 - Fix logic inversion in import_pool handling of $read_write (Andrew J. Hesford)
* 14b88b9 - generate-zbm: enable components in config.yaml (Andrew J. Hesford)
* 71cd075 - Add --enable/--disable (Zach Dykstra)
* 999e6c2 - Drop import_args variable and let import_pool build its own arguments (Andrew J. Hesford)
* b85d836 - support sk, an fzf workalike, for menu presentation (Zach Dykstra)
* 2225fea - module-setup.sh: catch installation failures, warn or fail as appropriate (#72) (Andrew J. Hesford)
* 5801e00 - Prune everything not explicitly needed (Zach Dykstra)


## ZFSBootMenu v1.4.1 (2020-08-19)

ZFSBootMenu 1.4.1 is a minor update, updating the provided Makefile for packagers.


## ZFSBootMenu v1.4 (2020-08-19)

ZFSBootMenu 1.4 includes significant internal changes and some user-visible functional changes in the `generate-zbm` script.

### Fixes
* Correct an issue that required two attempts to set default boot environments.
* Internal improvements to `generate-zbm` to improve consistency and facilitate future development.
* Management of versioned image retention should now be more consistent with expectation. Versioned ZBM images now increment a revision number when existing images with the same version already exist, and the retention policy preserves a configurable number of revisions for the current version alongside the latest revision of each of the same number of prior versions.
* Improved error handling should make failures in `generate-zbm` easier to understand.

### New features
* Provide man pages (generate-zbm.5, generate-zbm.8 and zfsbootmenu.7) to document the creation and use of ZFSBootMenu images.
* Move from an INI configuration format to YAML, which should provide more flexibility for future enhancements.
* Provide a `--migrate` command-line option to convert existing INI configurations to the new format.
* Add configuration options to change default behavior for `--kernel`, `--kver` and `--prefix` to make `generate-zbm` easier to incorporate on non-Void systems.
* Add a configuration option to change the default behavior for `--version` to allow customized output versioning of images.
* Support string interpolation of `%current` or `%{current}` tags in `--kver` and `--version` values.
* Add a `--cmdline` command-line option to override the configured `CommandLine` value without editing the configuration file.

### Significant commits in this release
* 0979051 - Add documentation for generate-zbm, its config and initramfs options (Zach Dykstra, et al.)
* ee1d9d8 - Unmask `import_args` in functions calling `import_pool` (Zach Dykstra)
* 3b2b2f0 - Add explicit `--migrate` option to generate-zbm (Andrew J. Hesford)
* 3cd3a8e - Improve error handling and automatic config conversion (Andrew J. Hesford)
* 80e0c30 - Switch syslinux entry to heredoc, fix syslinux.cfg file copy (Zach Dykstra)
* 6351226 - Move to YAML configuration, improve version handling (Andrew J. Hesford, Zach Dykstra)
* 5fdb872 - Add configuration options for kernel, version and prefix (Andrew J. Hesford)
* 79295ec - Add an optional parameter to safeCopy: (Zach Dykstra)
* 8aa133f - Clean up control flow in generate-zbm (Andrew J. Hesford)


## ZFSBootMenu v1.4rc1 (2020-08-11)

Except for the addition of man pages and the fix in commit ee1d9d8, the new features and fixes in this release are fully described in the final v1.4 release notes.


## ZFSBootMenu v1.3.1 (2020-07-14)

This release fixes an issue found minutes after v1.3 was tagged and released - such is life. After timing out on the countdown menu, the screen is now cleared before displaying a prompt for the pool password.

## ZFSBootMenu v1.3 (2020-07-14)

This release features several fixes and new features.

### Fixes
* When creating a boot image, `generate-zbm` will fail if the EFI System Partition (`BootMountPoint` in the configuration) is not and cannot be mounted.
* When `generate-zbm` creates backup kernels, initramfs images or UEFI bundles, timestamps of the original files will be preserved when possible, which may help boot loaders like rEFInd properly order the images.
* Some display issues in the boot menu have been fixed.

### New features
The following new features should allow ZFSBootMenu to work with distributions such as Arch or Ubuntu:
*  The Dracut module now searches for a much broader range of kernel/initramfs pairs in boot environments, including unversioned kernels with names like `linux` or `vmlinuz`.
* `generate-zbm` now has a `--kver` argument that can specify a version number when one cannot be correctly determined from the name of a kernel file, allowing creation of ZFSBootMenu images on systems like Arch that do not encode version information in kernel names.
* When starting a boot environment, the `root=` command-line argument is now set with a prefix (*e.g.*, the `zfs:` part of `root=zfs:pool/ROOT/void`) that is chosen based on distribution ID in `/etc/os-release` or `/usr/lib/os-release`, if available; the default selection can be overridden by setting the `org.zfsbootmenu:rootprefix` property.

In addition:
* In the ZFSBootMenu snapshot browser, an option to view a `zfs diff` between the live boot environment and a selected snapshot allow convenient review of the changes since the snapshot was taken.
* ZFSBootMenu now attempts to detect an active suspend-to-disk image and prevent any operations on ZFS pools that could lead to corruption on resume.
* The currently selected boot environment and kernel are displayed throughout submenus.
* In addition to identifying boot environments by the property `mountpoint=/`, ZFSBootMenu will also identify boot environments with the properties `mountpoint=legacy` and `org.zfsbootmenu:active=on`.
* Boot environments with `mountpoint=/` can be hidden from ZFSBootMenu by setting the property `org.zfsbootmenu:active=off`.

### Significant commits in this release
* 7122be9 - Use mountpoint to check for ESP (Zach Dykstra)
* 315e326 - Check return of mount operation (Zach Dykstra)
* 8e434b1 - Allow root prefix to be customized for other distributions (Andrew J. Hesford)
* fcaba86 - Support unversioned kernel naming in generate-zbm (Andrew J. Hesford)
* 294a84d - Broaden search for kernels and initramfs images (Andrew J. Hesford)
* 2263dbe - Handle kernels with multi-part versions (Zach Dykstra)
* 95f65a6 - Initial support for org.zfsbootmenu:active visibility (Andrew J. Hesford)
* 69c3d63 - Draw the preview header on kernel, snapshot and diff screens (Zach Dykstra)
* 83b2cbb - Initial support for resume guard (Andrew J. Hesford)
* 6828550 - Initial snapshot diff browser (Zach Dykstra)
* 4c0a968 - Report source size when cloning/duplicating a snapshot (Zach Dykstra)


## ZFSBootMenu v1.3rc2 (2020-07-09)

This release contains all of the fixes and new features in v1.3rc1, as well as some fixes to command-line generation that should allow ZFSBootMenu to properly boot Arch Linux systems.


## ZFSBootMenu v1.3rc1 (2020-07-07)

The new features and fixes in this release are fully described in the final v1.3 release notes.


## ZFSBootMenu v1.2 (2020-06-22)

This release features substantial code and idea contributions from @ahesford . Thank you for all of your help writing features, debugging code and improving documentation.

### Snapshot overhaul

Previously, snapshots could be cloned to a boot environment with a pre-generated, and often long, BE name. Substantial quality of life improvements were made here, including:

* Add the ability to do a full zfs send | zfs recv clone from a local snapshot. This lets you establish a new boot environment that is not dependent on any other environments or snapshots.
* Add the ability to clone and promote a snapshot, or simply clone it.
* When cloning a snapshot, local ZFS properties of the parent filesystem are now transferred to the clone.
* For all snapshot operations, you can now directly enter a boot environment name. This name is checked for character validity, and to confirm that it is not already taken.

### Always up-to-date menu system

Any time you transition from one menu to another (Snapshots, Kernels, recovery shell), the list of boot environments and kernels is completely regenerated. This helps remove potential disconnects between the state of your pool and boot environments and the ZFSBootMenu interface.

### UEFI bundle improvements

If you create unversioned UEFI bundles for static boot entries, `generate-zbm` will now create a `-backup` file for you on upgrade. This will allow you a recovery option if the active UEFI bundle has a problem.

### Protect against missing kernel modules

When creating a new initramfs, the `zfsbootmenu` Dracut module needs to install a number of ZFS-related kernel modules. Previously, the modules were installed through a Dracut helper function that did not verify if the copy succeeded. This process has been reworked to ensure that all of the required kernel modules are installed in the initramfs, or the creation of the image is marked as a failure. If it is marked as a failure, existing/current images are not deleted or otherwise replaced.

### Set default kernel

Much like setting a default boot environment, you can now set a default kernel for a specific boot environment. You no longer need to manually set a kernel version in your booted OS, you can simply do it from the menu!

### Shellcheck

On commit, the shell scripts that power the boot menu and Dracut setup are run through a validator to check for common errors and pitfalls. This can help reduce some classes of bugs.


## ZFSBootMenu v1.1 (2020-06-11)

This release includes a number of small fixes and improvements.

### Fixes
* Correctly handle exiting the recovery shell, fixing an infinite loop. A reboot is no longer required to recover from the recovery shell. Thanks, @ahesford.
* Check that the EFI stub file is present on disk at the specified location. If the file is missing and an EFI bundle is requested, exit with an error.
* Minor documentation fixes.

### Features
* Handle console fonts defined on the kernel command line. This is useful for systems with a 4k display. You should now be able to read the screen without a magnifying glass. Thanks to @ahesford for the significant time spent on tracking this down.
* Instead of calling `objcopy` directly, `dracut` is now used to generate a bundled EFI file. This means that the bundled EFI file can now be signed by Secure Boot keys! Thanks to @ericonr for this feature - and learning a bit of Perl to do it!

No configuration file changes are needed to use this release. Enjoy!


## ZFSBootMenu v1.0 (2020-05-15)

We're jumping straight up to v1.0!

### Small changes
* Set the kernel log level to 0 when in the menu system, then restore it to the original value on boot
* Reverse sort the kernel list, so the most recent is always first
* Add an initial chroot helper script, `zfs-chroot` for the recovery shell. It can be invoked as `zfs-chroot pool/ROOT/BE`.
* Set the default config path in `generate-zbm` to `/etc/zfsbootmenu/conf.d`
* Allow the EFI stub file to be defined in config.ini
* Optionally read the kernel command line from `/etc/default/zfsbootmenu`

### Large changes
* Clean up or whitelist all issues in `zfsbootmenu-lib.sh`, `zfsbootmenu-preview.sh` and `zfsbootmenu.sh` noted by shellcheck.
* Support entering a custom kernel command line via `alt-c` on the main menu. The input line is pre-filled with the command line that would have been used on the next boot, for that environment. This command line is NOT persisted between reboots, it's simply here to let you recover an unbootable system.
* Add support for reading the kernel commandline from the ZFS property `org.zfsbootmenu:commandline`.  This property is now considered the default/primary source of truth for the kernel command line - it takes precedence over `/etc/default/zfsbootmenu` and `/etc/default/grub`.

A big thank you to @ahesford for his code contributions and testing leading up to this release!


## ZFSBootMenu v1.0rc2 (2020-05-12)

This release contains all of the fixes and features from 1.0rc1, as well as the following:

* Add support for reading the kernel commandline from the ZFS property `org.zfsbootmenu:commandline`. This property takes precedence over `/etc/default/grub` and `/etc/default/zfsbootmenu`
* Clean up or whitelist all issues in `zfsbootmenu-lib.sh`, `zfsbootmenu-preview.sh` and `zfsbootmenu.sh` noted by shellcheck.

A big thank you to @ahesford for his code contributions and testing leading up to this release candidate. It is very much appreciated!


## ZFSBootMenu 1.0rc1 (2020-05-11)

This release has been a long time coming. In no particular order, it contains the following:

* Set the kernel log level to 0 when in the menu system, then restore it to the original value on boot
* Reverse sort the kernel list, so the most recent is always first
* Add an initial chroot helper script for the recovery shell
* Set the default config path in `generate-zbm` to `/etc/zfsbootmenu/conf.d`
* Allow the EFI stub file to be defined in config.ini
* Support entering a custom kernel command line via `alt-c` on the main menu. The input line is pre-filled with the command line that would have been used on the next boot, for that environment. This command line is NOT persisted between reboots, it's simply here to let you recover an unbootable system.


## ZFSBootMenu 0.8.1 (2020-01-19)

This release adds a few improvements to generate-zbm.

* When finding kernels in /boot to use as the base for zfsbootmenu, the list is properly sorted so something like 5.4.11 is now a higher version than 5.3.18.
* Output files now have `_1` appended to them (both kernel and initramfs) to workaround a gotcha with Petitboots' syslinux.cfg parser ignoring files ending in `.0`.
* If the bootloader partition is defined in config.ini, generate-zbm will now mount it and unmount it for you. If it's already mounted, it's left alone.
* Syslinux mode now builds off of [Components] mode, instead of adding yet another code path for generating the initramfs. This makes syslinux mode simply a syslinux.cfg renderer.


## ZFSBootMenu 0.8.0 (2020-01-12)

This release adds a preview function to the primary menu screen. Two lines are shown at the top of the display.

Line 1: Selected boot environment (if it's the bootfs) - and the default kernel
Line 2: The discovered kernel arguments for the kernel in that boot environment

Additional features added in include:

* Set bootfs for a pool via `alt+d` on the main menu screen
* Ability to clone the same snapshot up to 1000 times. After cloning a snapshot, it can be set as the bootfs in the menu.
* Reduce the helper text to one line on all screens, to make it not as visually cluttered
* Finding kernel arguments has now been moved to a function, which can be extended to support multiple OS's as the need arises


## ZFSBootMenu 0.7.7 (2020-01-08)

This release adds support for generating a syslinux/extlinux-compatible configuration file. This can be used both with Petitboot on POWER hardware and with extlinux on x86_64.


## ZFSBootMenu 0.7.6 (2019-12-31)

This is a fairly minor release, with the removal of an external VERSION file and the inclusion of a basic Makefile being the major impacting changes.


## ZFSBootMenu 0.7.5 (2019-12-24)

A previous release defaulted pool imports to read-only mode. Because of this, cloning a snapshot would fail. This release fixes that by detecting if a pool is imported as read-only, exporting it and re-importing it read-write. If the snapshot being cloned is encrypted, keys will be loaded again (from file or via prompt).

To control read-write behavior, the command line argument `read_write` has been added. If unset, this option defaults to 0, importing the pool in read-only mode. Set to 1 to enable read-write on imported pools by default.


## ZFSBootMenu 0.7.4.1 (2019-12-20)

Point release to fix issues on a fresh installation with missing target paths.


## ZFSBootMenu 0.7.4 (2019-12-20)

This release fixes a glaring integration issue with rEFInd. Unified EFI files now use the platform kernel base (vmlinux, vmlinuz, etc), with the version and EFI appended. This allows them to integrate nicely with rEFInd's boot options and kernel roll-up features.


## ZFSBootMenu 0.7.3 (2019-12-20)

This release adds the helper bin/generate-zbm which can help control the lifecycle of a the components needed to boot a system. It can generate a versioned kernel and initramfs, and/or a combined kernel/initramfs/commandline EFI executable. The helper script is able to able to do un-versioned files as well, creating initramfs-zfsbootmenu.img and then rotating that into initramfs-zfsbootmenu-backup.img when a new initramfs is created.

To better integrate with boot loaders like rEFInd, the option 'timeout' can be set on the command line when booting into the bootmenu.

A value of 0 will bypass the countdown timer and menu screens, attempting to boot the environment set by bootfs.
A value of -1 will force you into the menu system, where you can pick a boot environment, kernel, snapshot etc.
Any positive integer value will enable the countdown timer, where the system will then attempt to boot the environment set by bootfs.

The default timeout value remains at 10 seconds, consistent with previous behavior.


## ZFSBootMenu 0.7.2 (2019-12-10)

Default the creation of the initramfs to hostonly mode, to substantially reduce the size of the generated file.


## ZFSBootMenu 0.7.1 (2019-12-10)

Minor bug fix release. Correctly handle a zpool value on the command line pointing to a non-existent pool. Additionally, when trying to read a pinned kernel version, read it from the selected BE, and not the BE pointed to by `bootfs`


## ZFSBootMenu 0.7.0 (2019-12-09)

This release includes the following:

* Include documentation on how to use ZFSBootMenu on a system with native encryption.
* Add the ability to prefer a specific kernel version via `org.zfsbootmenu:kernel` set on a filesystem. Refer to 17302b7d for additional details.
* Split out functions into `zfsbootmenu-lib.sh`, with some degree of documentation
* Modify clone snapshot functionality to clone the snapshot and then discover all kernels present in it. After cloning a specific snapshot, you can now access it from the main BE menu and boot any kernel, or the one set by `org.zfsbootmenu:kernel` at the time the snapshot was taken.
* Fix a small syntax error when handling no pools being imported
* Add supporting files for xbps packaging
* Switch pools to readonly on import, for additional safety
* No longer export a pool before kexec - saving us some number of seconds on boot.
* Do not sanity check memory in kexec for faster booting


## ZFSBootMenu 0.6.5 (2019-11-16)

This is largely a bug-fix release, built on top of Linux 5.3.10 and ZFS 0.8.2 for x86_64 and ppc64le (POWER8+).

* Properly sort the list of kernels by version, so that 5.x.10 is
considered a higher version than 5.x.9
*  Correctly set a return value for all zfs load-key operations
* Greatly simplify when a BE is mounted when trying to find kernels in
/boot
* Only add a BE to the environment list if one or more kernel/initramfs
pairs were found


## ZFSBootMenu 0.6 (2019-11-06)

This release brings support for native ZoL encryption! It supports encryption on the entire pool, or enabled for a specific boot environment!

Prompting for the passphrase happens if the key needs to be loaded to boot the environment set in `bootfs` or if you escape the auto-boot screen to enter the environment/snapshot/kernel browser.

A patch is provided for 90zfs/mount-zfs.sh which detects if the keylocation is a file, and then attempts to load it from disk. If the key file is not present, and the type is passphrase, it will prompt.

The default auto-boot screen now attempts to center itself in your tty, for a slightly easier to read output.

Booting from a snapshot has been fixed - the snapshot is now correctly unmounted after a kernel/initramfs pair is located in /boot from the snapshot.


## ZFSBootMenu 0.5 (2019-10-22)

Initial release!

The dracut module has been built into an initramfs for both x86_64 and ppc64le (POWER8+) - with Linux 5.3.7. A sample grub.cfg is provided, demonstrating how to enter the boot menu. Update your pool name, and set spl_hostid based on the output of 'hostid' on your machine.
