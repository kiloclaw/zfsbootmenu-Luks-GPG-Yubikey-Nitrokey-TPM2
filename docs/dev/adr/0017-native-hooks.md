# ADR 0017: Built-in hooks as boot steps, the hook API version 2, and the generated `zfunc` library

- **Status:** proposed, 3 November 2026
- **Decides:** item SB5 of [ROADMAP.md](../../../ROADMAP.md), with fixes 21 and 22 of the review of 27 October 2026,
  and which of SB5 and SB6 owns the compatibility library that `crates/DESIGN.md` gives to "package F"
- **Governs:** `zbm-bootenv`'s `hooks` module, the stages that run hooks (`zbm-startup`'s `40-early-hooks`, the
  menu's `setup.d`, key loading, the boot sequence's `boot-sel.d` and `teardown.d`), `zbm_tools::programs::run_hook`
  and `zbm_platform::shell`, `zbm-host`'s image plan (`image/core.rs`, `image/shell.rs`, `image/graphics.rs`), a new
  `zbm zfunc` applet, `docs/man/zfsbootmenu.7.rst`'s User Hooks, and `CHANGES-FROM-LEGACY.md`

## Context

**The built-in hooks.** Every image installs five hooks from `legacy/zfsbootmenu/hooks/`, and `zbm-host` writes or
rewrites three in Rust images:

| Hook | What it does | In Rust images today |
| --- | --- | --- |
| `early-setup.d/05-screen-readers.sh` | starts `brltty`, loads `speakup` (ADR 0011) | written by `zbm-host` when asked |
| `early-setup.d/10-generic-efi-init.sh` | prints an escape sequence so the EFI framebuffer recomputes its size | the legacy script, on brush |
| `early-setup.d/20-dracut-console-init.sh` | dracut only: runs `/lib/udev/console_init` on the control terminal | the legacy script; it reads `ZBM_BUILDSTYLE`, which early-setup hooks do not get, so it seems to do nothing |
| `early-setup.d/30-console-autosize.sh` | picks a console font size | rewritten as `exec /libexec/zbm console-autosize` |
| `teardown.d/80-luks-close.sh` | closes the LUKS mappings before `kexec`, so the kernel wipes their keys | rewritten as `/bin/zluks close` |
| `teardown.d/90-xhci-unbind.sh` | unbinds the USB controllers before `kexec` | the legacy `#!/bin/sh` script |

They run among the user's hooks, in the order of their names (`zbm_bootenv::hooks::run_stage`). `zbm.skip_hooks`
names hooks by file name, `80-luks-close.sh`, and removes their permissions; a user hook with a built-in's name, in
the image or in `zbm.hookroot`, replaces it (`zfsbootmenu.7`). Nothing in the boot sequence closes LUKS but that hook.
Start-up has a `Step` trait for its stages (`zbm-startup/src/step.rs`); there is no `BootStep` and no table of named
built-ins.

**Running a hook.** With a shell chosen (`ZBM_SHELL`, brush in Rust images), `run_hook` reads the first line: a `#!`
line naming a shell, or no `#!` line at all, runs `brush HOOK`; anything else is executed. So the options of
`#!/bin/sh -e` are dropped (fix 21), and an ELF hook, which has no `#!` line, goes to brush (fix 22).

**The environment.** The supervisor starts with `/etc/zfsbootmenu.conf` exported, so the start-up program and the
menu's hooks inherit it; each stage adds its own variables. Against the legacy runtime, `early-setup.d` gets no
`ZFSBOOTMENU_INITIALIZATION`, `load-key.d` no `BOOTFS`, a menu started over SSH no configuration for its key hooks,
and no hook gets the `fzf` variables (`FUZZYSEL`, `PREVIEW_HEIGHT`, `FZF_DEFAULT_OPTS`). The environment is assembled
in four crates, though `crates/DESIGN.md` promises "a pure function, so unit tests check it for every stage". No
hook gets anything a program not written in a shell can read easily.

**The library.** Rust images still install `legacy/zfsbootmenu/{lib,libexec,bin}`. Hooks may source
`/lib/zfsbootmenu-core.sh` and `/lib/kmsg-log-lib.sh`, which source `/lib/zfsbootmenu-kcl.sh`: `contrib/`'s hooks do,
the recovery shell's `/root/.bashrc` does, and `crates/DESIGN.md` makes their 69 functions a contract ("Compatibility
library"), but gives it to "package F", which is SB6, while ROADMAP gives "the generated `zfunc` library" to SB5.
`/libexec/zfunc` is a Bash script that sources five files and calls any function by name; the legacy menu's `F12`,
`zfsbootmenu-help` and `docs/general/grub-migration.rst` use it. There is no `zfunc` applet. SB7, Rust images without
the legacy payload, needs the bundled hooks and the library to stop depending on those files.

**Measurement.** ADR 0009 measures the hooks copied from `zbm.hookroot` into PCR 9; the image's own hooks are measured
with the image.

## Decision

**The built-in hooks become boot steps in Rust, named and ordered as the files were; every hook runs as the kernel
would run it; each stage's environment is one function, with `ZBM_HOOK_API=2` and a JSON context; and the three
library files become a generated shell library over a `zbm zfunc` applet.**

### Boot steps

1. **`trait BootStep`** in `zbm-bootenv`'s `hooks` module, beside `run_stage`: a name, a stage (`HookStage`) and
   `run(&self, &Context, &HookEnv) -> Result<(), BootStepError>`. `run_stage` takes the stage's steps with the hook
   directory and runs both in the order of their names in the C locale, so a step runs where its file ran and a user's
   `85-x.sh` still runs between the LUKS close and the USB unbind. A step's error is logged with its code and the stage
   goes on, as a hook's exit status is ignored today.
2. **One table, `zbm_app::steps::BUILT_IN`,** since both boots reach `zbm-app` (the menu's and the countdown's through
   `zbm-startup`, which depends on it), and the LUKS close needs `zbm-luks`, which `zbm-boot` may not use. The callers
   pass the table down: start-up to its early stage, both boots to `kexec_kernel` through `BootSettings`. The console
   autosize's choice moves from `zbm-startup` to `zbm-app` with it, and the `console-autosize` applet calls it there.
3. **The six built-ins become steps,** and Rust images stop installing their scripts; the legacy runtime keeps the
   scripts, unchanged:
   - `05-screen-readers.sh` starts `brltty` and loads the `speakup` modules through the module loader (CD4), as the
     image's configuration asks;
   - `10-generic-efi-init.sh` writes the escape sequence to the console;
   - `20-dracut-console-init.sh` runs `/lib/udev/console_init` on the control terminal in dracut images, reading the
     build style from the image's configuration, not from the hook's environment;
   - `30-console-autosize.sh` is the autosize's choice;
   - `80-luks-close.sh` is `zbm-luks`'s `close_all`, after the pools are exported;
   - `90-xhci-unbind.sh` writes each `xhci_hcd` device to `unbind` through the files port.
4. **Names, skipping and replacing.** A step keeps its file name, `.sh` included, so `zbm.skip_hooks=80-luks-close.sh`
   and the man page keep their meaning; `zbm.skip_hooks` also accepts the name without `.sh`. A skipped step does not
   run, and the log says so. A user hook with a step's name, in the image or in the hook root, replaces the step, as
   it replaced the file; a hook-root hook stays measured (ADR 0009). But the LUKS close can be neither skipped nor
   replaced in a locked image (SE9) or under lockdown or Secure Boot (SE10): the runtime warns, and the step runs.
5. **Run once.** Early-setup steps run once, as the stage's hooks do, recorded in the state directory with the
   stages; a restarted start-up program runs neither again.

### Running a hook

6. **A hook runs as the kernel would run it** (fixes 21 and 22). `run_hook` executes the file. When its `#!` line
   names a shell (`/bin/sh`, `/bin/bash`, or `env` with either), the runtime's chosen shell runs it with the options
   of that line (`brush -e HOOK`), unless the line asks for an option the shell does not have, which runs it with
   Bash, and logs why. A file the kernel refuses (`ENOEXEC`), a text without a `#!` line, runs on the chosen shell, as
   Bash ran it in the legacy runtime. An ELF hook is executed.

### The hook API version 2

7. **One environment per stage,** built by one function in `zbm-bootenv` from what the stage knows, and checked by a
   test for every stage: the configuration's variables in every stage, whichever process runs it (an SSH menu
   included); `ZFSBOOTMENU_INITIALIZATION=yes` in `early-setup.d`; `BOOTFS` once it is known, `load-key.d` included;
   the stage's own variables as today. The `fzf` variables go, with `fzf` (SB6), and `CHANGES-FROM-LEGACY.md` says so.
8. **`ZBM_HOOK_API=2`** in every stage, and **`ZBM_HOOK_CONTEXT`,** the path of a JSON file the runtime writes before
   the stage and removes after it: `/run/zbm/hooks/<stage>.json`, mode 0444, owned by root. It holds the API version,
   the stage, the runtime, the build style, whether the image is a release build, locked, under lockdown or Secure
   Boot, and what the stage knows: the imported pools and the boot environment (`setup.d` on), the locked dataset and
   its encryption root (`load-key.d`), the selected environment, kernel, initramfs and mount point (`boot-sel.d`), and
   the command line, the device tree and how the kernel is loaded (`teardown.d`). It holds no secret and no key
   location's contents. Its schema is documented in `zfsbootmenu.7` and versioned with the API; fields may be added,
   never changed. It is not measured: the runtime writes it, and the hooks that read it are.

### The library

9. **SB5 owns the three files of the hook contract;** SB6 owns the rest of the legacy library (the menu's UI, `fzf`,
   the completions, the LUKS libraries) with the recovery tools. `crates/DESIGN.md` changes to say so.
10. **`zbm zfunc FUNCTION [ARGS]`,** an applet with a table of the 69 functions of `zfsbootmenu-core.sh`,
    `kmsg-log-lib.sh` and `zfsbootmenu-kcl.sh`. Each runs the Rust code where it exists (`import_pool`,
    `set_rw_pool`, `mount_zfs`, `load_key`, `find_be_kernels`, ...), and for the others the legacy function, until
    SB7 needs the list empty; a test lists which are still legacy. `emergency_shell` goes through the `ShellGate`
    (ADR 0010). The applet does not ask for the PIN: hooks are the image's or measured, and the recovery shell is
    behind the gate already.
11. **The generated library.** `zbm image plan` writes the three files at their paths from the applet's table: each
    function a wrapper, `f() { /libexec/zbm zfunc f "$@"; }`, with the implicit inputs and outputs `crates/DESIGN.md`
    lists (prefix assignments, `BOOTFS`, `spl_hostid`, `HEIGHT` and `WIDTH`); the functions that only transform text
    and the logging functions (`zdebug` is also a test of the log level) stay shell code, copied from the legacy files,
    which spares a process per log line. A test compares the generated functions with the legacy files' functions.
    `contrib/`'s hooks and `/root/.bashrc` keep sourcing the same paths. `/libexec/zfunc` becomes a link to the
    applet, so `grub-migration.rst`'s `/libexec/zfunc import_pool` keeps working.

### Steps

12. SB5a, running a hook (point 6) and one environment per stage (point 7); SB5b, the boot steps and their table
    (points 1 to 5), and the image plan without the built-ins' scripts; SB5c, the hook API version 2 (point 8); SB5d,
    the `zfunc` applet and the generated library (points 9 to 11); SB5e, docs, checks and the VM handoff.

## Consequences

- Rust images run no shell for the built-ins, and the LUKS close no longer depends on a script that a typo in
  `zbm.skip_hooks` or a hook root can remove where the image is locked or measured.
- Hooks run as on any Linux system: `#!/bin/sh -e` stops at the first error, a compiled hook runs, `#!/usr/bin/python3`
  runs on Python where the image has it.
- `early-setup.d` hooks get the configuration's variables, which they had in the legacy runtime; `20-dracut-console-init`
  does what it says on dracut images again.
- Hooks not written in a shell get one file to read; shell hooks get nothing new to learn.
- The library's functions keep their names and paths, so `contrib/` and users' hooks keep working through SB7, and
  each function moves to Rust on its own, with the test saying which are left.
- A process per call for the library's functions but the text and log ones: a hook that calls `import_pool` in a loop
  is slower than with the legacy library.
- The man page's User Hooks section grows: the API version, the context's schema, the steps and their names, and that
  sourcing the library is supported.

## Alternatives considered

- **Keep the built-ins as scripts and only fix their sources.** Less code, but SB7 still needs them free of the
  legacy files, and a shell per built-in stays at every boot.
- **Run the steps at fixed points** (before the user's early-setup hooks, after the user's teardown hooks). Simpler to
  explain, but a user's `85-x.sh` would no longer run before the USB unbind, which is why the names are numbered.
- **New names for the steps** (`luks-close`). Cleaner, but every `zbm.skip_hooks` on a kernel command line and the man
  page would change; accepting both spellings costs one comparison.
- **Let a hook root replace the LUKS close everywhere,** as today. Compatible, but on a locked or measured machine the
  hook root is the one input an attacker with the boot device can change, and leaving the keys in memory across
  `kexec` is what it would do.
- **Run every hook with brush,** as today, but pass the `#!` options. Fixes 21 only; an ELF or Python hook still goes
  to brush.
- **Every function of the library in Rust before SB5 ends.** The library is 69 functions, some (`zreport`,
  `zfs_chroot`) with no Rust yet and in SB6's scope; falling back to the legacy function keeps SB5 the size of SB5.
- **The context as environment variables only.** Lists (pools, kernels) do not fit a variable, and a program not
  written in a shell parses JSON more easily than quoted lists.

## How it is checked

- Unit tests for each step on `FakeSystem`, for the table (unique names, one stage each, sorted with hooks as the
  files were), for `zbm.skip_hooks` with and without `.sh`, for replacing a step, and for the LUKS close that cannot be
  skipped when locked or measured.
- `run_hook`'s tests: `#!/bin/sh -e` stops at the first error, `#!/usr/bin/env bash -x` keeps its option, an ELF
  hook runs, a text without `#!` runs on the shell.
- The environment function's tests for every stage, and a parity test of each stage's variables against the legacy
  runtime's, but for the `fzf` ones.
- The context's schema test, a test that it holds no secret, and that it is removed after its stage.
- The library's test against the legacy files' functions, a test of each wrapper's implicit outputs, and the list of
  functions still legacy.
- VM tests: `runtime-rust-hooks` with a `#!/bin/sh -e` hook, a hook without `#!`, a hook that reads its context,
  `zbm.skip_hooks` of a step; the LUKS close's test in `secrets-*`, whose secret search before `kexec` would find a
  key left in a mapping; `firmware-usb*` for the USB unbind; `runtime-rust-autosize` and `runtime-rust-console-lines`
  for the early-setup steps; `contrib/keycache.sh` sourcing the generated library.

## Questions for the maintainer

1. **Which built-ins become steps.** Proposed: all six, the screen readers' and dracut's console init included. The
   alternative keeps `05-screen-readers.sh` and `20-dracut-console-init.sh` as scripts, since they run programs.
2. **Names.** Proposed: the file names, `.sh` included, with the names without `.sh` accepted. The alternative is new
   names, with the old ones accepted for one release.
3. **Skipping and replacing the LUKS close.** Proposed: allowed as today, but refused, with a warning, in a locked image
   or under lockdown or Secure Boot. The alternatives allow it everywhere, as today, or never.
4. **Running a hook.** Proposed: as the kernel would, the shell's `#!` lines on the chosen shell with their options,
   texts without `#!` on the shell, and Bash for an option brush lacks. The alternative runs every shell `#!` line on
   Bash, and only hooks without `#!` on brush.
5. **The environment.** Proposed: the legacy runtime's per stage, the configuration everywhere and `BOOTFS` in
   `load-key.d`, without the `fzf` variables. The alternative keeps today's.
6. **The context file.** Proposed: `/run/zbm/hooks/<stage>.json` for the stage's length, read-only, no secrets, to
   every hook, the hook root's included. The alternative writes one file for the whole boot, updated per stage.
7. **The library.** Proposed: SB5 owns the three files of the hook contract, `zbm zfunc` runs Rust where it exists
   and the legacy function otherwise until SB7, and the text and log functions stay shell. The alternative leaves the
   library to SB6, so SB5 is the steps, hook execution and the API alone.
8. **`/libexec/zfunc`.** Proposed: a link to the applet. The alternative drops it with the legacy payload in SB7.
