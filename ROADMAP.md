# Roadmap

This roadmap lists features to add to ZFSBootMenu beyond what the legacy Bash and Perl code in `legacy/` does. Each
entry is a proposal, not a commitment. Two other plans carry the code there:

- [PORT-ROADMAP.md](PORT-ROADMAP.md) ported the host tools and the start-up of the boot runtime (packages A and B) to
  Rust, with the legacy code as the reference;
- [REFACTOR-ROADMAP.md](REFACTOR-ROADMAP.md) reshapes that code into layers (R0 to R4), builds the redesigned menu and
  the rest of the runtime (R5, R6), and removes the legacy runtime (R9).

A feature lands when the code it touches is in Rust and the phase it depends on allows it, so the features below are
grouped by what they wait for, not by importance. Their numbers never change, because code, tests and documents cite
them (`ROADMAP.md #18`).

Each feature lists why it is useful, how it could work and when it counts as done. Effort is a rough estimate for one
developer who knows the code: **Small** is under a week, **Medium** a few weeks, **Large** a few months.

## Principles

- The legacy code is the reference. A Rust part first behaves like the code it replaces; new behaviour comes after.
- Features that change how a system boots are off unless configured. Warnings and better messages can be on by default.
- Every feature comes with tests (unit, device or VM tests) and with documentation in `docs/`.
- Features marked **security** document what they protect against, and get a review before they ship.
- Kernel command line options, configuration files and `org.zfsbootmenu:*` properties stay compatible with upstream
  ZFSBootMenu, so boot environments work with either.
- A feature of the menu is first tried in the menu prototype (`cargo run -p zbm-tui -- --demo`, phase D of
  REFACTOR-ROADMAP.md) on a fake machine, with wireframes as snapshot tests, before it is built on real code.

## Where things stand

In October 2026, on branch `claude/zbm-refactor`:

- **Done:** #20 (console fonts as 256-glyph subsets, and Spleen).
- **Prototyped** in the menu prototype, designed and drawn on a fake machine but not built: #6, #7, #11 and #21, and
  the menu side of #4 ([boot health](docs/dev/menu-design/boot-health.md),
  [unlocking](docs/dev/menu-design/unlocking.md)).
- **Partly done** by the port and the refactoring: #1 (secrets are wiped when dropped and reach programs only on
  standard input), #9 (the runtime journal and crash reports), #12 (the menu is phase D and R5 of the refactoring),
  #17 (the console font follows the display's EDID when a graphics driver provides it) and #18 (the fonts carry the
  letters).
- **Proposed:** the others.

## Overview

| # | Feature | Group | Effort | Status |
| --- | --- | --- | --- | --- |
| 1 | [Wipe secrets from memory](#1-wipe-secrets-from-memory) | 1 | Small | Partly done |
| 2 | [Unlock with whatever arrives first](#2-unlock-with-whatever-arrives-first) | 3 | Medium | Proposed |
| 3 | [Parallel startup](#3-parallel-startup) | 3 | Medium | Proposed |
| 4 | [Clear unlock errors](#4-clear-unlock-errors) | 2 | Medium | Prototyped |
| 5 | [Reproducible images](#5-reproducible-images) | 1 | Small | Proposed |
| 6 | [Check the boot environment's ZFS before booting](#6-check-the-boot-environments-zfs-before-booting) | 2 | Small | Prototyped |
| 7 | [Boot counting with automatic fallback, and boot once](#7-boot-counting-with-automatic-fallback-and-boot-once) | 2 | Medium | Prototyped |
| 8 | [Locked menu](#8-locked-menu) | 2 | Medium | Proposed |
| 9 | [Saved boot log and bug-report bundle](#9-saved-boot-log-and-bug-report-bundle) | 1 | Small | Partly done |
| 10 | [Measured boot](#10-measured-boot) | 3 | Medium | Proposed |
| 11 | [Re-seal the TPM after firmware updates](#11-re-seal-the-tpm-after-firmware-updates) | 2 | Medium | Prototyped |
| 12 | [Built-in menu interface](#12-built-in-menu-interface) | 2 | Large | In progress |
| 13 | [Own init](#13-own-init) | 4 | Large | Proposed |
| 14 | [Built-in SSH server for remote unlock](#14-built-in-ssh-server-for-remote-unlock) | 4 | Large | Proposed |
| 15 | [Reclaim space from snapshots](#15-reclaim-space-from-snapshots) | 2 | Medium | Proposed |
| 16 | [Pool health in detail](#16-pool-health-in-detail) | 2 | Medium | Proposed |
| 17 | [Graphics drivers for AMD and Intel GPUs](#17-graphics-drivers-for-amd-and-intel-gpus) | 1 | Medium | Partly done |
| 18 | [Spanish, French, Dutch and Brazilian Portuguese](#18-spanish-french-dutch-and-brazilian-portuguese) | 2 | Large | Partly done |
| 19 | [Own console in Rust](#19-own-console-in-rust) | 1 | Large | Proposed |
| 20 | [Console fonts: Spleen and 256-glyph subsets](#20-console-fonts-spleen-and-256-glyph-subsets) | Done | Small | Done |
| 21 | [Restore files and see what an upgrade changed](#21-restore-files-and-see-what-an-upgrade-changed) | 2 | Medium | Prototyped |

Groups:

1. [**Now, in images with the Rust runtime**](#group-1-now-in-images-with-the-rust-runtime): the code they touch is
   already Rust, and they are opt-in or change nothing a user sees.
2. [**With the new menu**](#group-2-with-the-new-menu): they need the menu of REFACTOR-ROADMAP.md R5, or the boot
   environment code it brings (port package C).
3. [**After the menu, on the finished runtime**](#group-3-after-the-menu-on-the-finished-runtime): they change how
   start-up, unlocking or booting behave, which the refactoring allows from R5, or need the Rust `kexec` path of all
   images.
4. [**After the cutover**](#group-4-after-the-cutover): they replace parts of the image or add a way in, and wait until
   the legacy runtime is gone (R9).

Status is one of Proposed, Prototyped, Partly done, In progress or Done. Update it, and the entry's **Status** line, in
the pull request that changes it.

## Group 1: now, in images with the Rust runtime

These touch code that is already Rust. Each is added as a change of its own, for images with
`zfsbootmenu_runtime=rust`; legacy images stay frozen (`legacy/RUNTIME-CHANGES.md`).

### 1. Wipe secrets from memory

**Security.**

**Status:** partly done. In the Rust runtime secrets are `zbm_platform::Secret`, wiped with `explicit_bzero` when
dropped (`zbm_sys::wipe`), and reach programs only on standard input; the journal writes fields named for secrets as
`[redacted]`, and the key method tests end with `assert_no_secret_leaked!` (property P1 of
`docs/dev/threat-model.md`). Locked pages, the wipe before `kexec` and the memory search remain.

- **Why:** Bash keeps passphrases and key material in variables, here-strings and pipe buffers that cannot be reliably
  erased. Copies can remain in RAM after ZFSBootMenu hands over to the booted kernel. ZFSBootMenu already closes LUKS
  mappings before booting, which wipes the volume keys, but not the secrets used to unlock them.
- **How:** hold secrets in types that erase themselves when dropped (done: `Secret`; `zeroize` or `secrecy` are
  proposal 5 of REFACTOR-ROADMAP.md), lock their pages in RAM (`mlock`), and hand them to libcryptsetup and helper
  tools through memory or pipes: never through command-line arguments, environment variables or files. Wipe
  everything that is left before `kexec`.
- **Done when:** a test unlocks volumes with a known passphrase and key file, then searches process memory, command
  lines, environments and tmpfs for them and finds nothing.

### 5. Reproducible images

- **Why:** the same source and build container should give the same bytes, so that anyone can check a published image
  and sign it. Today two builds from the same tree have identical contents but different bytes, because the initramfs
  records timestamps.
- **How:** set `SOURCE_DATE_EPOCH` from the last commit and pass dracut's `--reproducible` option; check the EFI bundle
  that `objcopy` writes for timestamps. `generate-zbm` is Rust now, so the change goes there. Build paths are removed
  from the binaries with `--remap-path-prefix` (proposal 17 of REFACTOR-ROADMAP.md). With #13, the Rust generator
  writes the archive itself, with fixed timestamps, owners and file order.
- **Done when:** CI builds the release image twice, in different directories, and the two images are identical.

### 9. Saved boot log and bug-report bundle

**Status:** partly done. Every runtime program appends its events to the runtime journal
(`/zfsbootmenu/journal.jsonl`), with secrets redacted, and a panic leaves a crash report that `zbm debug crash`
prints and `cargo xtask symbolize` resolves (REFACTOR-ROADMAP.md R1). Both are lost with the memory file system at
`kexec` or restart. `zbm debug` grows its other reports in R4.

- **Why:** ZFSBootMenu logs to the kernel log of its own kernel, which is lost when it boots another kernel or the
  machine restarts. Failures on unattended machines cannot be diagnosed afterwards.
- **How:** optionally write the journal and crash reports to the EFI system partition, keeping the last few, and mount
  the partition only for the write. A `zbm report` command in the recovery shell writes pool status, LUKS header
  summaries, token and TPM status, versions and the journal to the EFI system partition or a USB stick. It never
  includes key material. Proposal 2 of REFACTOR-ROADMAP.md (`zbm debug bundle`, and `xtask replay` turning a bundle
  into a test) builds on the same bundle.
- **Done when:** a VM test restarts the machine and finds the previous journal on the EFI system partition, and a test
  checks that a report contains none of the test passphrases or keys.

### 17. Graphics drivers for AMD and Intel GPUs

**Status:** partly done. `zbm console-autosize` sizes the console font from the display's EDID when a graphics driver
provides one (HiDPI), and the VM tests `runtime-rust-autosize` and `runtime-rust-colours` add `virtio_gpu` to their
images. The opt-in drivers, the hand-back before `kexec` and the list of known-good GPUs remain. The Rust runtime owns
the `kexec` path of its images.

- **Why:** images leave out every graphics driver (`omit_drivers+=" amdgpu radeon nvidia nouveau i915 drm "` in
  `config/zfsbootmenu/dracut.conf.d/omit-drivers.conf`), and the release images boot with `nomodeset`, because a GPU
  that one kernel has set up rarely starts again under the kernel that `kexec` hands over to. The console then runs
  on the framebuffer the firmware left behind:
  - in whatever mode the firmware chose, often not the panel's own;
  - with nothing on a screen the firmware did not light up, such as a second monitor or one behind a dock;
  - with no EDID, so `zbm console-autosize` cannot size the font to the display and falls back to its old rule.
- **How:**
  - **Opt-in drivers.** The drivers are `amdgpu` for AMD GPUs from GCN on, and `i915` and `xe` for Intel graphics.
    `generate-zbm` adds them when asked, for example with `Kernel.GraphicsDrivers: amdgpu i915 xe`, for dracut and
    mkinitcpio alike, and drops `nomodeset` from the image's command line. Host-only builds include only the firmware
    files of the machine's own GPU: `amdgpu` firmware for every chip is tens of megabytes, and one chip needs a few.
    Release images stay without the drivers.
  - **Load before the first prompt.** At start-up the drivers load before anything is asked, so the console does not
    change mode under a passphrase field.
  - **Hand the GPU back before `kexec`.** The runtime unbinds the device from its driver, which runs the driver's
    shutdown path, and resets the PCI function where it supports a reset. The booted kernel then finds the GPU as at
    power-on.
  - **Known-good GPUs only.** A list of GPU families known to come back after this decides whether a driver loads at
    all. Other GPUs stay on the firmware framebuffer, and the log says why.
  - **A way out.** `zbm.gpu=off` on the command line skips the drivers for one boot, for a GPU that does not come
    back.
  - **What stays out.** `radeon` (AMD GPUs before GCN) and NVIDIA's drivers: the proprietary driver cannot ship in the
    image, and `nouveau` is not tested.
- **Done when:**
  - A VM test with a graphics driver QEMU can emulate (`bochs` or `virtio-gpu`) checks the sequence: the driver loads
    before the prompts, the EDID reaches `zbm console-autosize` (done in `runtime-rust-autosize`), the device is
    unbound before `kexec`, and the booted kernel brings up its display.
  - On real hardware, each listed machine (Intel Gen 9 and Xe graphics, an AMD APU, an AMD RDNA graphics card) boots
    twenty times in a row through `kexec`, with a working display both in ZFSBootMenu and in the booted system.
  - Image size and the time to the menu are measured against an image without the drivers.

### 19. Own console in Rust

**Security.**

Its first step, a prototype on the display without a terminal, can start now; the console itself needs the menu (R5)
and becomes the default only after the cutover.

- **Why:** the menu and the recovery shell run on the kernel's console, which takes only PSF bitmap fonts. #20 makes the
  best of them: 256-glyph subsets keep the 16 colours on VGA text consoles (the framebuffer console keeps them anyway),
  and Spleen's native 32×64 serves the densest displays. What stays out of reach:
  - at most 512 glyphs, so no font covers more than a few scripts, and 256 when the colours matter;
  - fixed bitmap sizes, between which a display gets either too small or too large a font;
  - 16 colours, not the 256 or true colour that terminal programs and themes expect;
  - keyboard layouts only as the image's one keymap, which the unlock dialog works around by translating keys
    itself.

  Fedora is replacing the kernel console with `kmscon`, a console in user space that draws on the display through DRM.
  `kmscon` is C with `libtsm`, `libxkbcommon`, `freetype` and `fontconfig`, a few megabytes in the image. A console in
  Rust fits this project better: it is one more applet of `zbm`, and it keeps typed secrets under the rules of
  `docs/dev/threat-model.md`.
- **How:** a `zbm console` applet, which the supervisor starts on the display before the first prompt.
  - **Display.** It draws into a dumb buffer through DRM (the `drm` crate, 0.15, on the workspace's `rustix` 1). It
    works on `simpledrm`, the firmware's framebuffer, without graphics drivers, and at the native resolution with #17.
  - **Text.** Any TTF or OTF font, at the size `zbm console-autosize` already computes from the display's EDID,
    rasterized with `fontdue` (pure Rust; a terminal grid of Latin text needs no shaping). Full Unicode and 256
    colours, so the four languages of #18 show without a 512-glyph subset.
  - **Terminal.** `alacritty_terminal`, the terminal state machine of Alacritty, on a pseudo-terminal (`rustix`), so
    `zfsbootmenu-init`, the menu and the recovery shell run in it and `vim`, `less` and `mc` work. `vte` and `vt100`
    are already vendored for the VM tests.
  - **Keyboard.** Key events from `evdev`, layouts from `xkbcommon_rs` (a Rust port of libxkbcommon) with the layout
    files the image needs. The unlock dialog's `Ctrl-K` then switches real keyboard layouts, dead keys and compose
    included.
  - **Secrets.** No scrollback while a prompt is open, and the grid and the PTY's buffers wiped when it closes.
  - **Fallback.** If the console fails to start or ends, the supervisor puts the menu back on the kernel console,
    which stays built in. The serial console is unchanged.
  - **Handing over.** Before `kexec` it releases the display and the keyboard, and gives the kernel console back, so
    that the booted kernel takes over as it does today.
  - **Kernel errors.** A panic is drawn by the kernel's DRM panic handler (Linux 6.10 and later, `simpledrm` among
    the drivers), so it is not hidden behind the console.
  - **Dependencies.** `drm`, `fontdue`, `alacritty_terminal` and the keyboard crates are not among the agreed crates
    (decision 1 of REFACTOR-ROADMAP.md): adding them is a decision of their own, with their size in the image.
- **Steps:**
  1. A prototype draws the menu on DRM in a VM (`zbm-tui --demo --drm`): fonts, sizes, `simpledrm`, keyboard input
     and the hand-over before `kexec`, without a terminal. The VM tests can read the screen with QEMU's `screendump`,
     as `runtime-rust-colours` does. Medium.
  2. The console: `alacritty_terminal`, the pseudo-terminal, the recovery shell, the fallback. Large; after R5.
  3. The default, once the VM tests and a list of machines pass. Images can keep the kernel console with an option.
- **Done when:**
  - the VM tests read the console's grid as text, as they read the serial console now, and pass with it;
  - snapshot tests compare its rendering, in memory, with reference images at three sizes;
  - a test types a passphrase and finds it in no buffer of the console once the dialog closed;
  - a VM test kills the console and the menu comes back on the kernel console;
  - image size and the time to the menu are measured against the kernel console.

## Group 2: with the new menu

These need the menu that REFACTOR-ROADMAP.md R5 builds, the boot environment code it ports (package C), or both. The
prototypes show how they look and behave; R5 builds the menu's core tasks first, and these follow on it.

### 4. Clear unlock errors

**Status:** prototyped. The unlock dialog of the prototype says why a method is asked instead of another, when a
secret was wrong, and when the keyboard layout is the likely cause, with `Ctrl-K` to type with another layout
([unlocking](docs/dev/menu-design/unlocking.md)). In the runtime, `zbm_luks::LuksError` and `TokenFailure` carry each
failure's cause, and `cryptsetup`'s exit statuses map onto typed errors (a wrong key, no device, busy).

- **Why:** several different failures look alike today. Users need to know whether the passphrase was wrong, the
  volume has no key slot or token for the method, the LUKS header is damaged, the TPM refused because the system
  changed (PCR mismatch), the FIDO2 key or GnuPG card is missing, or its PIN was wrong. The TPM lockout is already
  detected.
- **How:** map the outcomes of the `zbm-crypt` adapters to one error type with a message and a hint for each case,
  shown by the unlock dialog. Where a tool's output cannot tell two cases apart, call libcryptsetup through
  `libcryptsetup-rs` and the TPM through `tss-esapi`; neither is among the agreed crates yet (decision 1 of
  REFACTOR-ROADMAP.md).
- **Done when:** every failure case of the LUKS device suites (`crates/zbm-luks/tests/devices/edge.rs`) produces its own
  message, and the test checks it.

### 6. Check the boot environment's ZFS before booting

**Status:** prototyped: the menu runs five checks on each environment, shows them on its row and in the inspector,
and asks before booting one that fails ([boot health](docs/dev/menu-design/boot-health.md)).

- **Why:** ZFSBootMenu warns when its own ZFS does not support a pool's features. Nothing warns when the selected boot
  environment's ZFS is too old for the features active on its root pool. That boot environment then fails to import
  its root pool after `kexec`, away from ZFSBootMenu's recovery tools.
- **How:** before booting, read the ZFS module version that the boot environment's initramfs (or, failing that, its
  module tree) contains for the selected kernel. Look up the features that version supports in the `compatibility.d`
  files that OpenZFS ships (`openzfs-2.2-linux` and so on), and compare them with the features that are active on the
  pool. Features that are enabled but unused do not block an import. List the features the boot environment does not
  support, and ask before booting.
- **Done when:** a VM test with an old ZFS in the boot environment, like `complex-oldzfs`, shows the warning before
  booting, and boot environments with a recent enough ZFS boot without it.

### 7. Boot counting with automatic fallback, and boot once

**Status:** prototyped: `zbm-tui --demo --failing` shows the fallback, `--next-boot` a boot asked for once by the
running system ([boot health](docs/dev/menu-design/boot-health.md)). The commands `zbm bless` and `zbm next-boot` and
the option `zbm.boot_tries` are proposals; their names are open until they are built.

- **Why:** after an update that does not boot, an unattended or remote machine stays down until someone reaches the
  console. And a running system cannot ask for the next boot only, as `systemctl reboot --boot-loader-entry` does for
  systemd-boot.
- **How:** a boot environment or kernel can be marked for trial, from the menu or with a tool in the booted system.
  ZFSBootMenu decrements a counter before each trial boot, and when it reaches zero boots the last known-good boot
  environment instead. A small service in the booted system marks the boot good, as `systemd-bless-boot` does for
  systemd-boot. ZFSBootMenu imports pools read-only, so the counter is kept in an EFI variable; systems without UEFI
  are not supported at first. The same variable can name the environment to boot once.
- **Done when:** a VM test boots a boot environment that crashes and ZFSBootMenu falls back after the configured number
  of tries, a boot environment whose service marks the boot good keeps booting, and a boot asked for once happens once.

### 8. Locked menu

**Security.**

- **Why:** anyone at the console can edit the kernel command line (for example `init=/bin/sh`), open the recovery
  shell, or boot another boot environment or snapshot. On a system that unlocks with a TPM and no PIN, that means
  anyone who can start the machine reaches the decrypted data.
- **How:** optionally require a PIN (stored as an Argon2id hash in the image) or a FIDO2 key before editing the command
  line, opening the recovery shell, choosing anything but the default boot environment, or managing snapshots. The
  default boot stays unattended. The lock only helps when the image itself cannot be replaced, so the documentation
  pairs it with Secure Boot and signed images.
- **Done when:** VM tests check that each locked action asks for the PIN, that a wrong PIN is refused with a delay,
  and that the default boot still needs no input.

### 11. Re-seal the TPM after firmware updates

**Security.**

**Status:** prototyped: `zbm-tui --demo --tpm-changed` shows which PCR changed and why, asks for the passphrase
instead of the TPM2 PIN, and offers the re-seal alert ([unlocking](docs/dev/menu-design/unlocking.md)).

- **Why:** a firmware update changes PCR values. TPM unlocking then fails, and the passphrase is needed at every boot
  until someone re-enrolls the token from the booted system.
- **How:** when a TPM2 token fails because of a PCR mismatch and the volume is then unlocked with a passphrase or
  recovery key, show which PCRs changed and offer to re-seal the token to the current values. Re-sealing trusts the
  current state of the machine, so it needs explicit confirmation. The same code gives `zluks` commands to enroll
  TPM2 and FIDO2 tokens and recovery keys.
- **Done when:** a VM test changes a PCR the token is bound to, unlocks with the passphrase, re-seals, and the next
  boot unlocks with the TPM alone.

### 12. Built-in menu interface

**Status:** in progress, as the refactoring's menu: designed in phase D, with a prototype that runs every core task
on a fake machine; the R0 pilot showed the boot environment list in the Rust image on a serial console. R5 builds it,
and `fzf` then leaves the images (decision 3 of REFACTOR-ROADMAP.md).

- **Why:** the menu runs `fzf` with Bash previews, which starts many processes and depends on the `fzf` version in the
  image. A built-in interface behaves the same everywhere and can add mouse support, themes, and a plain mode for
  serial consoles and screen readers.
- **How:** Ratatui for the layout and `nucleo-matcher` for filtering, redesigned from the tasks users come to do rather
  than rebuilt screen for screen (REFACTOR-ROADMAP.md, "The menu redesign"). The `fzf` menu's `MOD+key` bindings stay
  as aliases in their old views, so the help pages and users' habits stay valid (decision 8, open until the usability
  sessions).
- **Done when:** the exit of R5: the VM tests drive every core task through the menu, including on a serial console.

### 15. Reclaim space from snapshots

- **Why:** a nearly full pool makes writes fail during boot, and only destroying snapshots frees the space they hold.
  Finding which ones hold it takes `zfs list -o space` and `zfs destroy -nv` in the recovery shell, and space shared
  by neighbouring snapshots is freed only when all of them go, which no single snapshot's `used` shows. The menu's
  boot checks already warn when a pool has less than 5% free; this is what to do next.
- **How:** a *Reclaim Space* view lists an environment's snapshots by the space destroying each frees. The user marks
  several, and the header shows what destroying them together frees, from a dry run (`zfs destroy -nvp` on the marked
  snapshots). One confirmation, with the environment's name typed, imports the pool read/write and destroys them.
  Snapshots with a `zfs hold`, and those a clone depends on, are shown as protected and cannot be marked; a key adds or
  releases a hold, so a snapshot can be kept from automatic pruning. The pool's checkpoint is never offered.
- **Done when:** a VM test fills a pool, marks snapshots and checks that the space freed matches the dry run, and that
  held snapshots and snapshots with clones cannot be destroyed.

### 16. Pool health in detail

- **Why:** after a disk fails or is replaced, the menu shows a pool as `DEGRADED` and nothing more. Which device
  failed, how many errors each has, whether a resilver runs and when it ends, and which files are damaged take
  `zpool status -v` in the recovery shell.
- **How:** opening a pool shows its device tree (each vdev and device with its state and read, write and checksum
  errors), the progress of a scrub or resilver with the time left, and the damaged files, from `zpool status -j`
  (OpenZFS 2.3 and later; `zpool status -p` before), through the typed ZFS port of `zbm-zfs`. Actions, each confirmed:
  clear the errors (`zpool clear`), bring a device online or take it offline. Replacing a device stays in the recovery
  shell. ZFSBootMenu imports pools read-only, and a read-only pool neither resilvers nor takes these actions: the view
  says so and offers to import it read/write first.
- **Done when:** VM tests with a degraded mirror and a resilvering raidz show the same devices, errors and progress as
  `zpool status`; clearing errors and bringing a device back online work; a read-only pool offers the import first.

### 18. Spanish, French, Dutch and Brazilian Portuguese

**Status:** partly done: the console fonts carry every letter the four languages use, in both families, and a test
checks it (#20). The catalogue comes with the menu in R5.

- **Why:** ZFSBootMenu speaks only English, at the moment a user is least able to look words up: a boot that went
  wrong, a passphrase refused, a pool that did not import. Spanish, French, Dutch and Brazilian Portuguese cover
  many of the users who ask for help in other languages today.
- **What is translated:**
  - about 500 strings the user reads: the menu, the unlock and import prompts, and the errors of unlocking,
    importing, start-up and booting;
  - the help inside the menu;
  - the man pages `zfsbootmenu(7)` and `zfsbootmenu-luks(7)`.
- **What stays English:**
  - the log, the runtime journal and bug-report bundles (#9), so that maintainers can read and search them;
  - command line options, `org.zfsbootmenu:*` properties and configuration keys, which ZFSBootMenu shares with
    upstream;
  - the distribution guides, which change too often for translations to keep up.
- **How:**
  - **Catalogues.** One Fluent file per language (`locales/es`, `fr`, `nl`, `pt-BR`), built into `zbm`: a few tens of
    kilobytes each. Fluent is already vendored, as a dependency of `uucore`. Its plural rules follow CLDR (French
    counts 0 as singular, for example).
  - **Choosing the language.**
    1. `zbm.lang=fr` on the kernel command line.
    2. Otherwise, the language `generate-zbm` read from the build host's `/etc/locale.conf`.
    3. Otherwise, English.

    The command palette changes it for the rest of the session. A string missing from a catalogue is shown in
    English.
  - **Characters.** The image's console fonts, Terminus or Spleen, installed as 256-glyph subsets in images with the
    Rust runtime, keep every letter and mark the four languages use: ñ ¿ ¡ é è ç œ « » ã õ ë. The only gap is the
    Dutch ligature ĳ, which Dutch usually writes as two letters. Where the terminal has no UTF-8 (`--theme ascii`, old
    serial terminals), accents are dropped (é becomes e), as line drawing already falls back to ASCII.
  - **Keys stay the same in every language.** `s` opens the snapshots whatever the word for them, so that each key
    keeps one meaning, and the classic bindings, the VM tests and the documentation stay valid. Help shows each key
    beside its translated label.
  - **Layout.** Translations run 20 to 35% longer than English. Titles use sentence case, as these languages do, not
    English title case. Numbers, sizes and ages follow the language (`1,2 Go`, `il y a 12 h`), from small rules per
    language rather than ICU, which is too large for the image.
  - **Keyboard layouts.** The layout translation of the unlock dialog gains `es` and `br-abnt2`. Dutch users mostly
    type on the US layout.
  - **Translators.** Each language needs a native speaker who reviews its changes. A language without one stays at
    its last reviewed state, its missing strings in English, and never blocks a release.
- **Steps:**
  1. In R5 (REFACTOR-ROADMAP.md), the menu takes every string from the catalogue, in English. CI checks that every
     key exists and that placeholders match. A pseudo-language that lengthens and accents every string finds text
     that does not fit before any translation exists. Small, if done then.
  2. The four catalogues of the menu, the prompts, the errors and the help. Medium.
  3. The two man pages, through Sphinx's translation tools (`sphinx-intl`, `.po` files). Large, and ongoing.
- **Done when:**
  - every view's wireframes, at 120×40, 80×25 and 52×20, exist in each language and fit without clipping;
  - a VM test boots with `zbm.lang=` set to each language, unlocks a volume and boots an environment;
  - a test checks that log and journal entries stay English whatever the language;
  - each language has had a native speaker's review.

### 21. Restore files and see what an upgrade changed

**Status:** prototyped: `f` on a snapshot opens the Changes view, with a Files segment that restores marked paths and
a Packages segment that lists what the upgrade installed, removed, upgraded and downgraded
([boot health](docs/dev/menu-design/boot-health.md), wireframes `diff`, `diff-marked`, `diff-packages` and
`restore-confirm`).

- **Why:** after an upgrade that broke something, the user wants one file back, or to know which package did it.
  Today that takes a rollback, which loses everything since the snapshot, or `zfs diff` in the recovery shell and
  reading package databases by hand.
- **How:**
  - **Files.** What changed since the snapshot (`zfs diff`), one row per path with its kind and a preview. Restoring
    copies marked paths back from `.zfs/snapshot/<name>/`, keeping owner, mode and times, after importing the pool
    read/write and taking a safety snapshot `@before-restore`; the confirmation wants the environment's name typed.
  - **Packages.** The package database in the snapshot compared with the live one: `xbps`, `dpkg`, `pacman` or `rpm`
    (its SQLite database, read without `rpm`). Each row says what happened and how to undo it, and lists the
    package's changed files.
- **Done when:** a VM test changes and removes files in a boot environment, restores them from a snapshot and checks
  their contents, owners and modes and the safety snapshot; fixture tests read each package database format and give
  the same changes as the distribution's own tools.

## Group 3: after the menu, on the finished runtime

These change how start-up, unlocking or booting behave. The refactoring changes no behaviour before R5, and package C
of the port, which R5 builds, brings the Rust `kexec` path to every image.

### 2. Unlock with whatever arrives first

- **Why:** the Bash code tries unlock methods in turn. A FIDO2 key or a USB key stick plugged in while the passphrase
  prompt is shown is not noticed until the prompt ends, and a slow TPM delays the prompt.
- **How:** run one task per method (TPM2, FIDO2, YubiKey challenge-response, GnuPG smart card, key file or key device,
  passphrase) and watch device hot-plug through kernel uevents. The `KeyMethod` strategies of `zbm-luks` (R3) are the
  units to run. The first method that unlocks the volume cancels the others. Configuration keeps its meaning: a volume
  configured for a key file alone never shows a prompt. PIN attempts against the TPM stay one at a time, so they do
  not use up the TPM's lockout counter. One concurrency model for the runtime is proposal 7 of REFACTOR-ROADMAP.md.
- **Done when:** the hot-plug and lockout VM tests (`complex-hotplug`, `complex-hotplug-auto`, `firmware-usbkey`,
  `firmware-usblate`, `keys-lockout`) pass, and a new test plugs in a key while the passphrase prompt is shown.

### 3. Parallel startup

- **Why:** kernel discovery mounts one boot environment after another, and several LUKS volumes that share a
  passphrase each run a slow Argon2id key derivation in turn. Systems with many boot environments or disks wait the
  longest.
- **How:** scan boot environments with a thread pool, and unlock volumes in parallel. Each Argon2id derivation can use
  up to 1 GiB of RAM, so the number of parallel unlocks depends on free memory. Pool imports stay serial where the
  `zpool` command needs it.
- **Done when:** the VM tests measure the time from kernel start to the menu, it drops on a layout with four LUKS
  members and on a pool with twenty boot environments, and the menu shows the same entries as before.

### 10. Measured boot

**Security.**

- **Why:** the firmware measures ZFSBootMenu itself into the TPM when it boots as an EFI image, but nothing measures the
  kernel, initramfs and command line that ZFSBootMenu boots. The booted system cannot tie its own TPM secrets to
  "this kernel, booted with this command line".
- **How:** before `kexec`, extend a PCR with the hashes of the kernel, the initramfs and the command line, and record
  the events in a log the booted system can read. The PCRs to use (GRUB uses PCR 8 for command lines and PCR 9 for
  files) and the log format are to be decided and documented. Extend only after ZFSBootMenu's own TPM unlocks, so the
  booted system cannot unseal those secrets again, much as systemd measures boot phases into PCR 11. A tool predicts
  the PCR values for a boot environment, so policies can be prepared before an upgrade.
- **Done when:** a VM test with a software TPM compares the PCR values after booting with the prediction, and checks
  that ZFSBootMenu's TPM tokens no longer unseal after the extension.

## Group 4: after the cutover

These replace how the image is built or add a way into it, so they wait until the Rust runtime is the only one (R9)
and the image assembly is Rust (R6).

### 13. Own init

- **Why:** dracut and mkinitcpio add size and boot time, and they differ between distributions. For example, Ubuntu's
  dracut builds host-only images by default, which left the ZFS and dm-crypt modules out of container builds.
- **How:** a Rust program runs as PID 1 in a minimal initramfs. It mounts `/proc`, `/sys` and `/dev`, loads modules
  using `modules.dep`, waits for devices through kernel uevents instead of udev, sets up the console and keymap, and
  starts the menu. The Rust `generate-zbm`, which computes the image's file list from R6, writes the image itself.
  dracut and mkinitcpio stay supported until the new image passes every test. The risk is hardware that dracut handles
  today: USB keyboards, NVMe, firmware files and microcode.
- **Done when:** every VM test passes with the new image type, and its size and boot time are measured against dracut
  images.

### 14. Built-in SSH server for remote unlock

**Security.**

- **Why:** remote unlocking needs dropbear, `dracut-crypt-ssh` and NetworkManager in the image, assembled by
  `contrib/remote-ssh-build.sh`.
- **How:** an SSH server written in Rust (`russh`) with key authentication only and host keys created at build time.
  The network is configured by DHCP or from the kernel command line (`ip=`). A session offers unlocking and the menu;
  a shell only when configured.
- **Done when:** a VM test unlocks a pool over SSH, password logins and unknown keys are refused, and a security review
  is done.

## Done

### 20. Console fonts: Spleen and 256-glyph subsets

For images with the Rust runtime, where `zbm console-autosize` chooses the font; the legacy hook keeps its fonts. It
helps the kernel console until, and wherever, #19 replaces it.

**Status:** done. The subsets: the image build runs `zbm console-fonts`, the code is `zbm_types::psf`, the image
and wireframe tests check them, and the VM test `runtime-rust-autosize` loads them on a real kernel, doubled too.
Spleen is done too: Spleen 2.2.0 in 8×16, 12×24, 16×32 and 32×64 (`legacy/zfsbootmenu/fonts/spleen/`, with its
licence and where it came from; 5×8 and 6×12 lack `œ Œ`), chosen with `Global.ConsoleFont: spleen` or the build option
`zfsbootmenu_console_font=spleen`; without a display density, `zbm console-autosize` still keeps to 32 pixels. The
VM test passes with both families (64984bd): the Spleen image, built through `generate-zbm`, holds Spleen only, loads
8×16 from the EDID and the native 32×64 for `zbm.autosize=64`. The check of the colours is the VM test
`runtime-rust-colours`, which counts the colours of a 16-colour bar in QEMU screenshots of the VGA text console and the
framebuffer console, with the image's font and the whole 512-glyph one: it passes (803de05). The image's fonts show all
16 colours on both consoles; the whole font shows 8 on the VGA text console and 16 on the framebuffer console, which
keeps the bright foreground colours with 512 glyphs: the subsets are what the VGA text console needs.

- **Why:**
  - **Colours.** The six Terminus fonts of the image hold 512 glyphs each (their sizes say so: `ter-v32b.psf` is
    512 glyphs of 64 bytes and a Unicode table). `setfont(8)` warns that a font of more than 256 glyphs leaves 8 of
    the 16 colours. On a VGA text console that loses the bright ones, which the menu uses for muted text, titles
    and warnings; the framebuffer console keeps the 16 foreground colours (measured by `runtime-rust-colours`).
  - **Dense displays.** The largest Terminus font is 16×32, so the densest displays get it doubled to a blocky 32×64.
- **How:**
  - **Subsets.** The image build makes each font of the image a 256-glyph PSF2 subset: ASCII, the characters the
    menu draws (box drawing, `▶ ‹ · • … √ ↑ ↓ →` and the others of the theme), Latin-1, and `œ Œ €` for the
    languages of #18. That is about 230 characters in fewer glyphs, so all 16 colours stay. The Unicode table maps
    characters a font lacks to a glyph it has (`▶` to `►`, `“ ”` to `"`), so a font missing one can still be used.
    The subsets are modified fonts under the same licence; a PSF file carries no font name, so none uses a reserved
    one.
  - **Spleen.** An option chooses the family, one per image, so the text looks the same at every density:
    `Global.ConsoleFont: terminus` (the default, as today) or `spleen`. Spleen (BSD-2) has native sizes from 5×8 to
    32×64, so the densest displays get a font drawn at that size instead of a doubled one. `zbm console-autosize`
    already reads the cell of each font from its header, and prefers a native size to a doubled one of the same
    height.
- **Done when:**
  - a test reads every font `generate-zbm` builds and checks that it has at most 256 glyphs, every character the
    menu draws and the letters of the four languages of #18, as the wireframe tests check the fonts of the image
    today;
  - the VM test of `zbm console-autosize` passes with both families, and on the 4K display chooses Spleen's 32×64
    without doubling;
  - on a VGA text console and on a framebuffer console, muted text, titles and warnings show in their colours.

## Not planned

- **Bindings to `libzfs`.** Its interface changes between OpenZFS releases. The port calls the `zfs` and `zpool`
  commands as the Bash code does, and reads their JSON output (`-j`, OpenZFS 2.3 and later), falling back to `-H -p`
  for older ZFS in images built on a host. `libzfs_core`, which is more stable, can serve the few operations where
  speed matters.
- **Changes that break compatibility with upstream ZFSBootMenu** command line options, configuration or properties.
