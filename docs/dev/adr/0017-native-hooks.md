# ADR 0017: Built-in hooks as boot steps, the hook API version 2, and the generated `zfunc` library

- **Status:** accepted, 4 November 2026 (proposed on 3 November); point 10 amended by [ADR 0018](0018-recovery-tools.md) on 9 November 2026
  (`zbm zfunc` asks for the PIN for `zfs_chroot` too)
- **Decides:** item SB5 of [ROADMAP.md](../../../ROADMAP.md), with fixes 21 and 22 of the review of 27 October 2026,
  and which of SB5 and SB6 owns the compatibility library that `crates/DESIGN.md` gives to "package F"
- **Governs:** `zbm-bootenv`'s `hooks` module, the stages that run hooks (`zbm-startup`'s `40-early-hooks`, the
  menu's `setup.d`, key loading, the boot sequence's `boot-sel.d` and `teardown.d`), `zbm_tools::programs::run_hook`
  and `zbm_platform::shell`, `zbm-host`'s image plan (`image/core.rs`, `image/shell.rs`, `image/graphics.rs`), a new
  `zbm zfunc` applet, `docs/man/zfsbootmenu.7.rst`'s User Hooks, and `CHANGES-FROM-LEGACY.md`

## Context

**The built-in hooks.** Every image installs five hooks from `legacy/zfsbootmenu/hooks/`; `zbm-host` rewrites two of
them in Rust images, and writes a sixth in any image that asks for a screen reader:

| Hook | What it does | In Rust images today |
| --- | --- | --- |
| `early-setup.d/05-screen-readers.sh` | starts `brltty`, loads `speakup` and starts `espeakup` for its soft synthesiser (ADR 0011) | written by `zbm-host` from the build options, after the user's hooks, so it replaces a user hook of its name |
| `early-setup.d/10-generic-efi-init.sh` | prints an escape sequence so the EFI framebuffer recomputes its size | the legacy script, on brush |
| `early-setup.d/20-dracut-console-init.sh` | dracut only: runs `/lib/udev/console_init` on the control terminal | the legacy script, on brush |
| `early-setup.d/30-console-autosize.sh` | picks a console font size | rewritten as `exec /libexec/zbm console-autosize` |
| `teardown.d/80-luks-close.sh` | closes the LUKS mappings before `kexec`, so the kernel wipes their keys | rewritten as `/bin/zluks close` |
| `teardown.d/90-xhci-unbind.sh` | unbinds the USB controllers before `kexec` | the legacy `#!/bin/sh` script |

They run among the user's hooks, in the order of their names (`zbm_bootenv::hooks::run_stage`). `zbm.skip_hooks`
names hooks by file name, `80-luks-close.sh`, and removes their permissions; a user hook with a built-in's name, in
the image (but for the screen readers') or in `zbm.hookroot`, replaces it (`zfsbootmenu.7`). A locked image ignores
`zbm.hookroot` (ADR 0010) but not `zbm.skip_hooks`. Nothing in the boot sequence closes LUKS but that hook, and Rust
images still install the legacy `zkexec`, whose legacy `kexec_kernel` runs the teardown hooks through
`/libexec/zfsbootmenu-run-hooks`.
Start-up has a `Step` trait for its stages (`zbm-startup/src/step.rs`); there is no `BootStep` and no table of named
built-ins.

**Running a hook.** With a shell chosen (`ZBM_SHELL`, brush in Rust images), `run_hook` reads the first line: a `#!`
line naming a shell, or no `#!` line at all, runs `brush HOOK`; any other `#!` line is executed. So the options of
`#!/bin/sh -e` are dropped (fix 21), and an ELF hook, which has no `#!` line, goes to brush (fix 22). Hooks run
through `Command` with a `pre_exec`, so the C library's `execvp`, which runs a file the kernel refuses on `/bin/sh`,
hides `ENOEXEC` from the runtime.

**The environment.** The supervisor starts with `/etc/zfsbootmenu.conf` exported, so the start-up program and the
menu's hooks inherit it, `early-setup.d` included; each stage adds its own variables. Against the legacy runtime,
`early-setup.d` gets no `ZFSBOOTMENU_INITIALIZATION`, `load-key.d` no `BOOTFS`, a menu started over SSH no configuration for its key hooks,
and no hook gets the `fzf` variables (`FUZZYSEL`, `PREVIEW_HEIGHT`, `FZF_DEFAULT_OPTS`). The environment is assembled
in four crates, though `crates/DESIGN.md` promises "a pure function, so unit tests check it for every stage". No
hook gets anything a program not written in a shell can read easily.

**The library.** Rust images still install `legacy/zfsbootmenu/{lib,libexec,bin}`. Hooks may source
`/lib/zfsbootmenu-core.sh`, which sources `/lib/zfsbootmenu-kcl.sh`, and `/lib/kmsg-log-lib.sh`: `contrib/`'s hooks do,
the recovery shell's `/root/.bashrc` does, and `crates/DESIGN.md` makes their 69 functions a contract ("Compatibility
library"), but gives it to "package F", which is SB6, while ROADMAP gives "the generated `zfunc` library" to SB5.
`/libexec/zfunc` is a Bash script that sources five files and calls any function by name; the legacy menu's `F12`,
`zfsbootmenu-help` and `docs/general/grub-migration.rst` use it. There is no `zfunc` applet. SB7, Rust images without
the legacy payload, needs the bundled hooks and the library to stop depending on those files.

**Measurement.** ADR 0009 measures the hooks copied from `zbm.hookroot` into PCR 9; the image's own hooks are measured
with the image. securityfs, which says whether the kernel is locked down, is mounted only in a locked image at start
and when the kernel is loaded.

## Decision

**The built-in hooks become boot steps in Rust, named and ordered as the files were; every hook runs as the kernel
would run it; each stage's environment is one function, with `ZBM_HOOK_API=2` and a JSON context; and the three
library files become a generated shell library over a `zbm zfunc` applet.**

### Boot steps

1. **`trait BootStep`** in `zbm-bootenv`'s `hooks` module, beside `run_stage`: a name, a stage (`HookStage`) and
   `run(&self, &Context, &HookEnv) -> Result<(), BootStepError>`; a step that needs what a `Context` does not hold runs
   the `zbm` applet that has it, as a program, without a shell. `run_stage` takes the stage's steps with the hook
   directory and runs both in the order of their names in the C locale, so a step runs where its file ran and a user's
   `85-x.sh` still runs between the LUKS close and the USB unbind. A step's error is logged with its code and the stage
   goes on, as a hook's exit status is ignored today.
2. **One table, `zbm_app::steps::built_in()`,** since both boots reach `zbm-app` (the menu's and the countdown's through
   `zbm-startup`, which depends on it), and the LUKS close needs `zbm-luks`, which `zbm-boot` may not use. The callers
   pass the table down: start-up to its early stage, both boots to `kexec_kernel` through `BootSettings`, whose
   `BootSteps` field compares and prints by the steps' names; `zbm-app`'s `Runtime` sets it, and a test checks that
   both callers of `kexec_kernel` do. The callers also mark the steps that may not be skipped or replaced (point 4),
   since they know the lock. In Rust images `/libexec/zfsbootmenu-run-hooks` becomes the applet `zbm run-hooks`, which
   runs the same steps and hooks, so the legacy `zkexec` still closes LUKS until SB6 replaces it.
3. **The six built-ins become steps,** and Rust images stop installing their scripts; the legacy runtime keeps the
   scripts, unchanged:
   - `05-screen-readers.sh` starts `brltty`, loads the `speakup` modules through the module loader (CD4) and starts
     `espeakup` for the soft synthesiser, as the build options ask, which `zbm-host` now writes to the image's
     configuration instead of into a script;
   - `10-generic-efi-init.sh` writes the escape sequence to the console;
   - `20-dracut-console-init.sh` runs `/lib/udev/console_init` on the control terminal in dracut images, reading the
     build style from the image's configuration, not from the hook's environment;
   - `30-console-autosize.sh` runs the `console-autosize` applet, which holds the console's fonts;
   - `80-luks-close.sh` is `zbm-luks`'s `close_all`, which exports the pools on the mappings first;
   - `90-xhci-unbind.sh` writes each `xhci_hcd` device to `unbind` through the files port.
4. **Names, skipping and replacing.** A step keeps its file name, `.sh` included, so `zbm.skip_hooks=80-luks-close.sh`
   and the man page keep their meaning; `zbm.skip_hooks` also accepts the name without `.sh`. A skipped step does not
   run, and the log says so. A user hook with a step's name, in the image or in the hook root, replaces the step, as
   it replaced the file; a hook-root hook stays measured (ADR 0009). But the LUKS close is kept in a locked image
   (SE9) and under lockdown or Secure Boot (SE10): `zbm.skip_hooks` cannot skip it, which is the one way left in a
   locked image, and a hook root cannot replace it under lockdown or Secure Boot; the runtime warns, and the step runs.
   An image's own hook of its name still replaces it, as the image is the owner's. Start-up mounts securityfs
   read-only before the early stage, as it mounts `efivarfs`, so lockdown is known from the start.
5. **Run once.** Early-setup steps run once, as the stage's hooks do: each is recorded in the state directory apart
   from the stages' records, so that neither a restarted start-up program nor `ZFSBOOTMENU_FORCE_INIT` runs it again,
   as neither runs a renamed hook again.

### Running a hook

6. **A hook runs as the kernel would run it** (fixes 21 and 22), decided from its first bytes before it starts,
   since `execvp` hides `ENOEXEC`. A `#!` line naming `/bin/sh` or `/bin/bash` runs on the runtime's chosen shell with
   that line's argument, as the kernel passes it, one argument (`#!/bin/sh -e` is `brush -e HOOK`), unless the
   argument asks for an option the shell does not have, which runs it with Bash, and logs why. Any other `#!` line,
   `env` included, and an ELF file are executed. A file with neither runs on the chosen shell, as Bash ran it in the
   legacy runtime.

### The hook API version 2

7. **One environment per stage,** built by one function in `zbm-bootenv` from what the stage knows, and checked by a
   test for every stage: the configuration's variables in every stage, whichever process runs it (an SSH menu
   included); `ZFSBOOTMENU_INITIALIZATION=yes` in `early-setup.d`; `BOOTFS` once it is known, `load-key.d` included;
   the stage's own variables as today. The `fzf` variables go, with `fzf` (SB6), and `CHANGES-FROM-LEGACY.md` says so.
8. **`ZBM_HOOK_API=2`** in every stage, and **`ZBM_HOOK_CONTEXT`,** the path of a JSON file the runtime writes before
   the stage and removes after it: `/run/zbm/hooks/<stage>.json`, mode 0444, owned by root. It holds the API version,
   the stage, the runtime, the build style, whether the image is a release build, locked, under lockdown (securityfs,
   point 4) or Secure Boot, and what the stage knows: the imported pools and the boot environment (`setup.d` on), the locked dataset and
   its encryption root (`load-key.d`), the selected environment, kernel, initramfs and mount point (`boot-sel.d`), and
   the command line, the device tree and how the kernel is loaded (`teardown.d`). It holds no secret and no key
   location's contents. Its schema is documented in `zfsbootmenu.7` and versioned with the API; fields may be added,
   never changed. It is not measured: the runtime writes it, and the hooks that read it are.

### The library

9. **SB5 owns the three files of the hook contract;** SB6 owns the rest of the legacy library (the menu's UI, `fzf`,
   the completions, the LUKS libraries) with the recovery tools. `crates/DESIGN.md` changes to say so.
10. **`zbm zfunc FUNCTION [ARGS]`,** an applet with a table of the 69 functions of `zfsbootmenu-core.sh`,
    `kmsg-log-lib.sh` and `zfsbootmenu-kcl.sh`. Each runs the Rust code where it exists (`import_pool`,
    `set_rw_pool`, `mount_zfs`, `load_key`, `find_be_kernels`, ...), and for any other name, the five files the legacy
    `zfunc` sources included, the legacy function, which Rust images install under `/usr/share/zfsbootmenu/legacy/`
    until SB7 needs the list empty; a test lists which are still legacy. `emergency_shell` goes through the gate
    (ADR 0010: the shell, or the PIN first in a locked image). The applet asks for the PIN for nothing else: hooks are
    the image's or measured, and the recovery shell is behind the gate already.
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

- Rust images run no shell for the built-ins, and the LUKS close can no longer be removed by `zbm.skip_hooks` in a
  locked image, or by a hook root under lockdown or Secure Boot.
- Hooks run as on any Linux system: `#!/bin/sh -e` stops at the first error, and a compiled hook runs instead of
  going to brush; other interpreters' `#!` lines run as they do today.
- `load-key.d` hooks get `BOOTFS`, and a menu over SSH gives its hooks the configuration, as the legacy runtime did.
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
- **Let `zbm.skip_hooks` and a hook root remove the LUKS close everywhere,** as today. Compatible, but on a locked
  image the command line, and under lockdown the hook root, are inputs an attacker with the boot device can change,
  and leaving the keys in memory across `kexec` is what they would do.
- **Run every shell hook with brush,** as today, but pass the `#!` options. Fixes 21 only; an ELF hook still goes to
  brush.
- **Every function of the library in Rust before SB5 ends.** The library is 69 functions, some (`zreport`,
  `zfs_chroot`) with no Rust yet and in SB6's scope; falling back to the legacy function keeps SB5 the size of SB5.
- **The context as environment variables only.** Lists (pools, kernels) do not fit a variable, and a program not
  written in a shell parses JSON more easily than quoted lists.

## How it is checked

- Unit tests for each step on `FakeSystem`, for the table (unique names, one stage each, sorted with hooks as the
  files were), for `zbm.skip_hooks` with and without `.sh`, for replacing a step, and for the LUKS close that cannot be
  skipped when locked or measured.
- `run_hook`'s tests: `#!/bin/sh -e` stops at the first error, `#!/bin/bash -eu` keeps its options, an ELF hook
  runs, a text without `#!` runs on the shell and not on `/bin/sh`.
- The environment function's tests for every stage, and a parity test of each stage's variables against the legacy
  runtime's, but for the `fzf` ones.
- The context's schema test, a test that it holds no secret, and that it is removed after its stage.
- The library's test against the legacy files' functions, a test of each wrapper's implicit outputs, and the list of
  functions still legacy.
- VM tests: `runtime-rust-hooks` with a `#!/bin/sh -e` hook, a hook without `#!`, a hook that reads its context,
  `zbm.skip_hooks` of a step; the LUKS close's test in the LUKS VM tests (`keys-*`, `layouts-*`, `complex-*`), whose
  teardown report after the close fails the boot when a mapping is left open; `firmware-usb*` for the USB unbind; `runtime-rust-autosize` and `runtime-rust-console-lines`
  for the early-setup steps; `contrib/keycache.sh` sourcing the generated library.

## Questions for the maintainer

1. **Which built-ins become steps.** Proposed: all six, the screen readers' and dracut's console init included. The
   alternative keeps `05-screen-readers.sh` and `20-dracut-console-init.sh` as scripts, since they run programs.
2. **Names.** Proposed: the file names, `.sh` included, with the names without `.sh` accepted. The alternative is new
   names, with the old ones accepted for one release.
3. **Skipping and replacing the LUKS close.** Proposed: allowed as today, but `zbm.skip_hooks` refused in a locked
   image and under lockdown or Secure Boot, and a hook root's replacement refused under lockdown or Secure Boot, each
   with a warning. The alternatives allow it everywhere, as today, or never.
4. **Running a hook.** Proposed: as the kernel would, decided from the first bytes: `/bin/sh` and `/bin/bash` lines
   on the chosen shell with their argument, other `#!` lines and ELF files executed, texts without `#!` on the shell,
   and Bash for an option brush lacks. The alternative runs every shell `#!` line on
   Bash, and only hooks without `#!` on brush.
5. **The environment.** Proposed: the legacy runtime's per stage, the configuration everywhere and `BOOTFS` in
   `load-key.d`, without the `fzf` variables. The alternative keeps today's.
6. **The context file.** Proposed: `/run/zbm/hooks/<stage>.json` for the stage's length, read-only, no secrets, to
   every hook, the hook root's included. The alternative writes one file for the whole boot, updated per stage.
7. **The library.** Proposed: SB5 owns the three files of the hook contract, `zbm zfunc` runs Rust where it exists
   and the legacy function otherwise until SB7, and the text and log functions stay shell. The alternative leaves the
   library to SB6, so SB5 is the steps, hook execution and the API alone.
8. **`/libexec/zfunc`.** Proposed: a link to the applet. The alternative drops it with the legacy payload in SB7.

## Where the work stands

- **SB5a** (4 November 2026), points 6 and 7, as decided but for one thing: a hook whose `#!` line asks for an option
  brush does not have runs as the kernel runs it, on the interpreter its line names, not on Bash, since Bash refuses
  what the kernel passes too (`-o pipefail` is one argument). A `#!` line ending in `\r\n` is read without its `\r`,
  as before. A new default boot environment chosen in the menu is recorded in the state directory, and the hooks that
  run after it get it as `BOOTFS`, as the legacy menu's did. `NO_CACHE` and `CLEAR_SCREEN`, which legacy `load-key.d`
  hooks inherited from the functions that ran them, are not set (`CHANGES-FROM-LEGACY.md`). zbm-bootenv's public items
  rose by three, `Variables`, `StageEnv` and `HookEnv`, the types this record calls for.
- **SB5b** (4 November 2026), points 1 to 5, as decided but for the LUKS close and what a name without `.sh` skips. The
  six built-ins are `BootStep`s in one table, `zbm_app::steps` (`zbm_types::BUILT_IN_HOOKS` lists their names, and a
  test holds the two together); `run_stage` runs a stage's steps among its hook files in the order of their names, and a
  hook file of a step's name, in the image or in `zbm.hookroot`, replaces the step, which the log says. Rust images no
  longer install the built-ins' scripts; the legacy runtime keeps them, unchanged. `zbm-host` writes the screen readers'
  build options to `/etc/zfsbootmenu.conf` as `ZBM_BRLTTY`, `ZBM_SPEAKUP` and `ZBM_ESPEAKUP`. The LUKS close runs the
  `zluks close` applet, as a program, and not `zbm-luks`'s `close_all` in the boot process (point 3), since a LUKS
  session needs the crypttab configuration `zluks` reads; it still exports the pools on the mappings first, and does
  nothing in an image without `cryptsetup`, as the script did. The USB unbind tries every controller and fails at the
  end naming those left bound. `zbm.skip_hooks` skips a step by its name, with or without `.sh`, and a name without
  `.sh` now also skips the hook file of that name with `.sh`, so that a hook of a skipped step's name does not run in
  its place; the legacy runtime matched the exact file name only. In a locked image and under lockdown or Secure Boot,
  `zbm.skip_hooks` does not skip the LUKS close, with a warning; under lockdown or Secure Boot a hook root's
  `teardown.d/80-luks-close.sh` is disabled (mode 0), with a warning, so the step runs. When `cp` fails to copy a
  hook-root file, the file of its name in the image is disabled too, whether it holds part of the copy or the image's
  hook the copy overwrote, since it was not measured. Start-up mounts securityfs read-only before the early stage, and
  warns about a locked image without lockdown or Secure Boot, whose kernel command line can be changed past the lock.
  Early-setup steps are recorded in the state directory (`/zfsbootmenu/steps`, lines `ran NAME` and `skipped NAME`). A
  step's error is ZBM-E0611. In Rust images `/libexec/zfsbootmenu-run-hooks` is a script that execs
  `zbm run-hooks [-once] STAGE`, which runs the stage's hooks and steps with the image's configuration and the stage's
  variables from its environment, and succeeds when a hook or step ran, as the legacy script did; so the legacy `zkexec`
  of the recovery shell still closes the mappings. The recovery shell's `zreport` lists hooks by their files, so the
  steps do not appear in its lists. Public items rose for the table, the trait, `run_quietly` and the list of names:
  `zbm-app` from 153 to 157, `zbm-bootenv` from 35 to 37, `zbm-tools` from 43 to 44 and `zbm-types` from 43 to 44.
- **SB5c** (4 November 2026), point 8, as decided but for when the file is written. Every stage's hooks get
  `ZBM_HOOK_API=2`. A stage that runs at least one hook file gets `/run/zbm/hooks/<stage>.json`, written before it (mode
  0444, root) and removed after it, and `ZBM_HOOK_CONTEXT` with its path; a stage of built-in steps alone writes no
  file. When the file cannot be written, `ZBM_HOOK_CONTEXT` is set empty and the failure logged, so that no hook takes
  for its own the file of a stage it runs within, which its environment may name, and a write that fails part way
  removes what it left. The file is one line of JSON with the fields of point 8: `api`, `stage`, `runtime`,
  `build_style`, `release`, `locked`, `lockdown` and `secure_boot` in every stage; `pools` (from `/proc/spl/kstat/zfs`)
  and `boot_environment` from `setup.d` on; `locked_fs` and `encryption_root` in `load-key.d`; `selected` and
  `mountpoint` in `boot-sel.d`; `selected`, `command_line`, `devicetree` and `loaded_by` (`kexec_file_load` or `kexec`)
  in `teardown.d`. An unknown value is null, as are the last three when the process running the stage did not load the
  kernel (the legacy `zkexec`, through `zbm run-hooks`). `zbm_tools::kexec` reads lockdown and Secure Boot apart, and
  the lock's path is `zbm_config::conf::LOCK`. `zbm-bootenv` may use `serde_json`, for the file. Public items rose:
  `zbm-bootenv` from 37 to 38, `zbm-tools` from 44 to 46 and `zbm-config` from 217 to 218.
- **SB5d** (4 and 5 November 2026), points 9 to 11, as decided but for which functions run Rust. Rust images install the
  three files as `zbm-host` generates them from the module root's (`image/library.rs`): 13 functions stay the legacy
  shell code (`decolorize`, `colorize`, `center_string`, `is_snapshot`, `kcl_tokenize`, `kcl_suppress`, `kcl_append`,
  `kcl_assemble` and the five logging functions), and the other 56 are wrappers that run `/libexec/zbm zfunc NAME "$@"`.
  A wrapper runs in a subshell that exports the variables the legacy functions read, so a variable a hook set without
  `export` still reaches the function. It keeps the implicit outputs callers read: `BOOTFS` after `set_default_env`
  succeeds; `ret`, and `spl_hostid` after a failed import, after `import_pool`; `ret` after `export_pool`; `HEIGHT` and
  `WIDTH` after `timed_prompt`; `mount_dev` after `is_mounted`. The other variables the legacy functions left behind,
  their loop variables, are not kept. The files' guards, error trap and `source` of the kcl file stay. The legacy files
  go below `/usr/share/zfsbootmenu/legacy/lib`, and `/libexec/zfunc` is a link to `zbm`. `zbm zfunc`
  (`zbm_startup::zfunc`) runs Rust code for two functions only, `emergency_shell`, through the gate (a locked image asks
  for the PIN first), and `is_degraded`, not for each that has Rust code (point 10): a function moved needs its legacy
  output, status and log lines bridged and a parity test; the 27 functions that have a Rust equivalent were surveyed for
  SB6 and SB7. Any other function runs as the legacy `zfunc` ran it: `zbm` replaces itself with Bash, which sources the
  legacy files (they source each other there, not the generated library) and runs the function with its caller's streams
  and signals, with the configuration's variables where the environment sets none; inside it, `emergency_shell` is
  `zbm`'s gated one. A test lists the 54 functions still legacy (`crates/zbm/src/zfunc.rs`). Each wrapped call is a
  process, its arguments must be UTF-8 (`zbm` refuses others with status 2), and a wrapped function does not see
  functions its caller defined. Public items rose: `zbm-host` from 113 to 115, `zbm-startup` from 99 to 102 and
  `zbm-tools` from 46 to 48.
- **SB5e** (5 November 2026): the docs (`zfsbootmenu.7`'s User Hooks: the built-in steps, the hook API, the context's
  schema and the shell library; `CHANGES-FROM-LEGACY.md`, `crates/DESIGN.md`'s compatibility library, the threat model,
  the LUKS pages and `zfsbootmenu-luks.7`, the help of `zbm.skip_hooks`), and VM checks in `runtime-rust-hooks` and
  `runtime-legacy-hooks`: the hook root's hook reports `ZBM_HOOK_API=2`, the teardown's context file (its stage and what
  loaded the kernel), the library's `get_zbm_arg` through `zbm zfunc`, and no script for the LUKS close in the Rust
  runtime; no API version, the same `get_zbm_arg` and the LUKS close's script in the legacy runtime. The other VM checks
  of "How it is checked" are those of the scenarios that ran the built-ins' scripts before (the LUKS tests' teardown
  report, which fails a boot that leaves a mapping open, `firmware-usb*`, `runtime-rust-autosize`, `runtime-rust-console-lines`), which now run the steps; hooks with
  `#!/bin/sh -e` or without `#!` and `zbm.skip_hooks` of a step are checked by unit tests only, and no test sources the
  generated library from `contrib/keycache.sh`. The VM run of SB5, on rebuilt images, is the maintainer's and has not
  run yet.

## Answers of the maintainer

Accepted as proposed, on 4 November 2026:

1. **Which built-ins become steps:** all six, the screen readers' and dracut's console init included.
2. **Names:** the file names, `.sh` included, and the names without `.sh` accepted.
3. **Skipping and replacing the LUKS close:** allowed as today, but `zbm.skip_hooks` refused in a locked image and
   under lockdown or Secure Boot, and a hook root's replacement refused under lockdown or Secure Boot, each with a
   warning.
4. **Running a hook:** as the kernel would, decided from the first bytes, with Bash for an option brush lacks.
5. **The environment:** the legacy runtime's per stage, without the `fzf` variables.
6. **The context file:** `/run/zbm/hooks/<stage>.json` for the stage's length, read-only, no secrets, to every hook.
7. **The library:** SB5 owns the three files of the hook contract; `zbm zfunc` runs Rust where it exists and the
   legacy function otherwise until SB7; the text and log functions stay shell.
8. **`/libexec/zfunc`:** a link to the applet.
