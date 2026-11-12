# Changes from the legacy runtime

The migration guide for the switch to the Rust runtime (RL1 in [ROADMAP.md](ROADMAP.md)): what behaves differently
in images with the Rust runtime (`zfsbootmenu_runtime=rust`) than in legacy images, and why. Images with the legacy
runtime, the default until that switch, are unchanged. The guide stays when the legacy runtime goes (RL2), for those
who come from it. [The menu](#the-menu) begins with its keys, a table from each key of the `fzf` menu to the new ones;
then come what each tab of the menu does differently, and what the menu as a whole does, and then the tools of the
recovery shell.

The last section, [The host tools](#the-host-tools-generate-zbm-and-zbm-kcl), is different: it lists what changed in
`generate-zbm` and `zbm-kcl`, the programs that build images and change command lines on the host, whatever runtime
the images have.

## The shell: brush

Rust images include [brush](https://github.com/reubeno/brush), a shell compatible with Bash and written in Rust, and
the runtime uses it wherever it used Bash. GNU Bash stays in the image as dracut's `/bin/sh` and `/bin/bash`.
`zbm.shell=bash` on the command line brings Bash back for everything below.

### The recovery shell is brush

- **Before:** `bash -i`, with readline.
- **Now:** `brush -i`, which reads the same `/root/.bashrc`: the prompt, the aliases, `help` and `PROMPT_COMMAND`
  stay. On the console on the display and over SSH, its line editor is reedline, with history hints and syntax
  colours; on a serial line, which may not answer the terminal queries of reedline, it is brush's basic line editor.
  `BASH_VERSION` is still set, for scripts that check it, and `BRUSH_VERSION` names brush's version. The history goes
  to `~/.brush_history`. When brush cannot start, or ends within five seconds, the supervisor opens `bash -i`
  instead; `bash` typed in the shell starts Bash too.
- **Why:** one shell, in Rust like the rest of the runtime, with a better line editor.

### Hooks run on brush

- **Before:** hooks ran as programs; a script without a `#!` line ran on Bash.
- **Now:** a hook whose `#!` line names `sh` or `bash` directly (`#!/bin/bash`, `#!/bin/sh -e`), or that has no `#!`
  line, runs on brush, with the option of its `#!` line (`brush -e`). A hook whose line asks for an option brush does
  not have (`-p`) runs as the kernel runs it, on the interpreter it names, and the log says so; as on any Linux system,
  the kernel passes what follows the interpreter as one argument, so `#!/bin/bash -o pipefail` fails there too. Other
  hooks, `#!/usr/bin/env bash`, Python and compiled programs among them, run as the kernel runs them. ZFSBootMenu
  tells which from the hook's first bytes, so a compiled hook is never handed to a shell (ADR 0017).
- **Why:** hooks see the shell the recovery shell has, and run as they would on any Linux system. A hook that does not
  work on brush can name another interpreter, through `env`, or the whole image can use Bash with `zbm.shell=bash`.

### Hooks get the legacy runtime's variables

- **Before (in earlier Rust images):** `load-key.d` hooks got no `BOOTFS`, and `early-setup.d` hooks no
  `ZFSBOOTMENU_INITIALIZATION`.
- **Now:** every stage's hooks get the variables of `/etc/zfsbootmenu.conf`, `BOOTFS` once it is known, and their
  stage's own, as in the legacy runtime: `ZFSBOOTMENU_INITIALIZATION=yes` in `early-setup.d`, `ZBM_LOCKED_FS` and
  `ZBM_ENCRYPTION_ROOT` in `load-key.d`, `ZBM_SELECTED_*` in `boot-sel.d` and `teardown.d`. A new default boot
  environment chosen in the menu is `BOOTFS` for the hooks that run after it, as the legacy menu's was. The variables of
  the `fzf` menu (`FUZZYSEL`, `PREVIEW_HEIGHT`, `FZF_DEFAULT_OPTS`) are not set: Rust images have no `fzf` menu. Nor are
  the flags of the `fzf` menu (`HAS_*`), which Rust images' `/etc/zfsbootmenu.conf` does not have
  ([`fzf`](#fzf)). Nor are `NO_CACHE` and `CLEAR_SCREEN`, which the legacy runtime's key loading set for its own
  functions and its `load-key.d` hooks inherited: the Rust runtime passes them as arguments.
- **Why:** one environment per stage, built in one place and tested for each stage.

### SSH logins get brush

- **Before:** the login shell of an SSH session was Bash.
- **Now:** an interactive SSH login hands over to brush, when the runtime chose it. Commands run over SSH
  (`ssh host zfsbootmenu`) are unchanged.
- **Why:** the same shell wherever the user types.

### What brush does not do yet

brush 0.4 runs ZFSBootMenu's own shell libraries, which the recovery shell and hooks source. It does not have every
feature of Bash: a script given on standard input fails where a `$(` substitution spans lines (a script file works).
Its parser had a bug with `case` patterns inside `$( )`, and its `kill` did not take signal numbers (`kill -9`); the
image's brush has fixes for both (`patches/README.md`). A command started with `&` runs inside brush rather than as a
process of its own: `jobs` lists it, but `$!` is empty and `kill %1` does not stop it.

## Hooks (SB5)

Rust images run the hooks as [ADR 0017](docs/dev/adr/0017-native-hooks.md) decides. How a hook runs and what its
environment holds are above, with the shell: [Hooks run on brush](#hooks-run-on-brush) and [Hooks get the legacy
runtime's variables](#hooks-get-the-legacy-runtimes-variables). `zfsbootmenu(7)`, User Hooks, documents the rest.

### The built-in hooks are steps of the runtime

- **Before:** images installed the hooks ZFSBootMenu ships as scripts: `early-setup.d/10-generic-efi-init.sh`,
  `20-dracut-console-init.sh` (dracut images), `30-console-autosize.sh` and, with a screen reader,
  `05-screen-readers.sh`; `teardown.d/80-luks-close.sh` and `90-xhci-unbind.sh`. They ran on a shell, among the
  user's hooks.
- **Now:** Rust images do not install these scripts. The runtime does the same things itself, as built-in steps with
  the same names: it writes the escape sequence that makes the EFI framebuffer find its size, runs
  `/lib/udev/console_init` on the control terminal in dracut images, runs `/libexec/zbm console-autosize`, starts the
  screen readers, runs `/bin/zluks close` when `cryptsetup` is in the image, and unbinds every `xhci_hcd` USB
  controller (it tries them all, and fails at the end naming those left bound). A step runs where its file ran: among
  the stage's hooks, in the order of their names, so a user's `85-x.sh` still runs between the LUKS close and the USB
  unbind. The log says `running the built-in step <name>`; a step that fails is logged with its code, `ZBM-E0611`, and
  the stage goes on, as a hook's exit status was ignored. A hook with a step's name, in the image or in
  `zbm.hookroot`, replaces the step, as it replaced the file, and the log says `the hook <name> replaces its built-in
  step`. The legacy runtime keeps the scripts, unchanged.
- **Why:** no shell runs for each built-in at every boot, and the hooks ZFSBootMenu ships no longer need the legacy
  files, which Rust images are to lose (SB7).

### `zbm.skip_hooks` takes names without `.sh`

- **Before:** `zbm.skip_hooks` named hooks by their file names, `80-luks-close.sh`, and removed the permissions of the
  files of those exact names.
- **Now:** a name skips a built-in step with or without `.sh` (`zbm.skip_hooks=80-luks-close`), and a hook file
  named `<name>.sh` too. A skipped step does not run, and the log says so (`Disabling built-in step`). The steps
  skipped are recorded in the state directory, so the boots from the menu and `zbm run-hooks` leave them out too.
- **Why:** the steps keep the names of their files, so command lines and the manual keep their meaning; accepting
  the name without `.sh` costs one comparison.

### The LUKS close is kept on a locked or measured machine

- **Before:** `zbm.skip_hooks=80-luks-close.sh` removed the LUKS close on any machine, and a hook root's
  `teardown.d/80-luks-close.sh` took its place; the mappings, and their keys, then stayed open across `kexec`.
- **Now:** in a locked image (ADR 0010), and under lockdown or Secure Boot (ADR 0008), the LUKS close runs anyway.
  `zbm.skip_hooks` does not skip it, and warns: `zbm.skip_hooks names 80-luks-close.sh, which this locked or measured
  machine runs anyway`. Under lockdown or Secure Boot, a hook root's `teardown.d/80-luks-close.sh` is disabled (mode
  0), with a warning, so the step runs; a locked image ignores `zbm.hookroot` altogether (ADR 0010). A hook of
  that name in the image itself still replaces the step: the image is the owner's. Elsewhere, both work as before.
  Start-up mounts securityfs read-only before the `early-setup.d` stage, so lockdown is known from the start, and a
  locked image without lockdown or Secure Boot is warned about: a changed kernel command line gets past the lock.
- **Why:** on such a machine the kernel command line and the hook root are what an attacker with the boot device can
  change, and keys left in memory across `kexec` are what they would be after.

### Early steps run once, recorded in the state directory

- **Before:** the built-in `early-setup.d` hooks ran once as every hook of the stage did: afterwards they lost their
  permissions and were renamed to `*.completed`.
- **Now:** the user's `early-setup.d` hooks still are. The built-in steps of the stage are recorded by start-up in the
  state directory, `/zfsbootmenu/steps`, as `ran <name>` lines (the steps `zbm.skip_hooks` names as `skipped <name>`),
  apart from the records of the stages, so neither a start-up program started again nor `ZFSBOOTMENU_FORCE_INIT` runs
  one again, as neither runs a renamed hook again.
- **Why:** a step has no file to rename.

### `zfsbootmenu-run-hooks` runs the steps too

- **Before:** `/libexec/zfsbootmenu-run-hooks [-once] STAGE` ran the executable files of the stage; the legacy
  `zkexec` of the recovery shell runs the `teardown.d` hooks with it.
- **Now:** in Rust images it is a script that execs `zbm run-hooks [-once] STAGE`, which runs the stage's hooks and
  its built-in steps (but those start-up left out), with `/etc/zfsbootmenu.conf` and the stage's variables from its
  environment, and exits 0 when a hook or step ran and 1 otherwise, as the script did. A boot by the library's
  `kexec_kernel`, still the legacy function, closes the LUKS mappings and unbinds the USB controllers, as a boot from
  the menu does. The recovery shell's `zkexec` no longer runs the stage this way: it is `zbm`'s and boots as the menu
  does ([`zkexec` boots](#zkexec-is-zbm-and-boots)).
- **Why:** with the scripts gone, `kexec_kernel` would otherwise boot with the mappings open.

### Hooks get `ZBM_HOOK_API` and `ZBM_HOOK_CONTEXT`

- **Before:** hooks got their stage's variables, and nothing a program not written in a shell reads easily.
- **Now:** every stage's hooks get `ZBM_HOOK_API=2`. When a stage runs at least one hook file, the runtime writes
  `/run/zbm/hooks/<stage>.json` before the stage (mode 0444, root's), passes its path as `ZBM_HOOK_CONTEXT`, and
  removes it after the stage; a stage of built-in steps alone writes none. The file is one line of JSON: the API
  version, the stage, the runtime, the build style, whether the image is a release build, locked, under lockdown
  (and which) or Secure Boot; from `setup.d` on, the imported pools and the boot environment; and what the stage
  knows: the locked dataset and its encryption root in `load-key.d`, the selected environment, kernel, initramfs and
  mount point in `boot-sel.d`, and in `teardown.d` the selection, the command line, the device tree and how the
  kernel was loaded. A value that is not known is `null`, as are the last three when `zbm run-hooks` runs the stage
  for the library's legacy `kexec_kernel`; `zkexec` loads the kernel itself and sets them. It holds no secret and no key
  location's contents. Later versions may add fields, never change or remove one. When the file cannot be written,
  `ZBM_HOOK_CONTEXT` is empty and the log says why.
- **Why:** hooks written in Python or compiled get one file to read, and shell hooks get nothing new to learn.

### The hook library runs in `zbm`

- **Before:** `/lib/zfsbootmenu-core.sh`, `/lib/kmsg-log-lib.sh` and `/lib/zfsbootmenu-kcl.sh` were the legacy shell
  code, and a hook that sourced them ran their functions in its own shell.
- **Now:** Rust images install the three files generated from the legacy ones. Their 69 functions keep their names;
  56 are wrappers that run `/libexec/zbm zfunc <name> "$@"` in a subshell that exports the variables the legacy
  functions read, so a variable a hook set without `export` still reaches the function. The functions that only
  transform text (`colorize`, `decolorize`, `center_string`, `is_snapshot`, `kcl_tokenize`, `kcl_suppress`,
  `kcl_append`, `kcl_assemble`) and the logging functions (`zdebug`, `zinfo`, `znotice`, `zwarn`, `zerror`) stay the
  legacy shell code. The wrappers leave in the caller's shell what callers read of the legacy functions: `BOOTFS`
  after a `set_default_env` that succeeded; `ret` after `import_pool`, and `spl_hostid` after a failed import; `ret`
  after `export_pool`; `HEIGHT` and `WIDTH` after `timed_prompt`; `mount_dev` after `is_mounted`. The other variables
  the legacy functions left set, such as their loop variables, are not. `emergency_shell`, `zfs_chroot` and
  `is_degraded` are Rust code; any other function runs as the legacy `zfunc` ran it: `zbm` replaces itself with Bash,
  which sources the legacy files, kept below `/usr/share/zfsbootmenu/legacy/lib`, and runs the function with the
  caller's streams and signals. `emergency_shell` opens the recovery shell, and `zfs_chroot` its chroot, through the
  shell's gate, whichever function calls them: a locked image asks for the PIN first. Each wrapped call is a process, so
  a hook that calls `import_pool` in a loop is slower; its arguments must be valid UTF-8 (`zbm` refuses others, with
  status 2); and the function does not see the functions the hook defined.
- **Why:** the hook contract no longer needs the legacy files, and each function can move to Rust on its own; a test
  lists those still legacy.

### `/libexec/zfunc` is `zbm`

- **Before:** a Bash script that sourced `/etc/zfsbootmenu.conf` and four files of the legacy library and called a
  function by name.
- **Now:** a link to `zbm`: `/libexec/zfunc FUNCTION [ARGS]` is `zbm zfunc`, which runs the function as the
  library's wrappers do, with the variables of `/etc/zfsbootmenu.conf` the environment does not set, as the script
  sourced the file. `/libexec/zfunc import_pool`, which the GRUB migration guide names, keeps working.
- **Why:** one way to call a function of the library, with the recovery shell behind its gate.

### `zreport` lists the built-in steps and escapes names

- **Before:** `zreport` listed the hooks by their files, so its lists named the built-in hooks with the user's. It
  printed the hooks' paths through `echo -e`, which acted on backslashes in them, and the command line and what
  `zfs`, `zpool` and `modinfo` printed as they were; `zbmcmdline` printed the command line as it was. At debug level
  start-up logged the report with its colours.
- **Now:** `zreport` and `zbmcmdline` are `zbm`'s. `zreport` lists the built-in steps after the hooks, each with its
  state: to run, run once, replaced by a hook of its name (one that ran once and became `NAME.completed` too), or
  skipped by `zbm.skip_hooks`. Both write the control characters of names, paths, the command line and the programs'
  output as escapes (`\x1b`), backslashes as they are, and bytes that are not UTF-8 as U+FFFD. The architecture
  `uname -m` printed comes from `/proc/sys/kernel/arch`. Start-up logs the report without its colours. `zbmcmdline`
  saves a command line it had to build in the state directory of `/etc/zfsbootmenu.conf` when `BASE` is not set,
  where the legacy function saved none.
- **Why:** a step has no file, so only Rust code lists it; and a pool's, a dataset's or a hook's name must not act on
  the terminal (P13 of the threat model).

### Screen readers are started from the configuration

- **Before:** images built with `zfsbootmenu_brltty=yes` or `zfsbootmenu_speakup=<synth>` got an
  `early-setup.d/05-screen-readers.sh` written by the image build after the user's hooks, so it took the place of a
  user hook of that name.
- **Now:** Rust images get `ZBM_BRLTTY`, `ZBM_SPEAKUP` and `ZBM_ESPEAKUP` lines in `/etc/zfsbootmenu.conf` instead,
  and the built-in step `05-screen-readers.sh` starts `brltty`, loads `speakup_<synth>` through the runtime's module
  loader and starts `espeakup` for the soft synthesiser, as they ask. As for every step, a user hook of that name
  replaces it. Legacy images keep the script.
- **Why:** the build options are values the runtime reads, not a script written for them.

## The console on the display

Only Rust images have the console of `zbm.console=drm`, which draws the terminal through DRM in place of the kernel
console.

### Programs see an xterm

- **Before:** programs ran with `TERM=linux`, the kernel console's description: 8 colours, and no Ctrl or Alt with the
  cursor keys.
- **Now:** on the console on the display, programs run with `TERM=xterm-256color` and `COLORTERM=truecolor` when the
  image has that terminfo entry, which Rust images include, else with `TERM=linux`. `ZBM_CONSOLE=drm` tells them
  they run there; the menu reads it to keep the console's look. The kernel console, serial lines and SSH keep their
  `TERM`.
- **Why:** the console draws 256 colours and true colour, and sends xterm's key sequences, as programs then expect;
  the recovery tools' chroot environments have the same entry.

## Start-up

Start-up runs on the menu's start-up view when the image has the menu and the terminal can run it (`TERM` set, and not
`dumb`) and is 52 columns by 20 lines at least; otherwise it keeps the console of the legacy program, prompts
included. `zluks` in the recovery shell and over SSH is unchanged.

### Start-up shows its stages

- **Before:** the stages' messages and the output of the programs they run scrolled by on the console.
- **Now:** the start-up view lists the stages, the one running and the ones done, and what takes a while
  (`Unlocking /dev/sda2 ...`) below them. What the stages' programs print goes to the log (`zlogtail`, saved logs,
  `zbm report`) instead of the screen. `early-setup.d` hooks and the recovery shell get the console as before; the view
  comes back after them.
- **Why:** start-up and the menu are one screen, and a dialog can ask without the text of other programs around it.

### Unlocking asks in a dialog

- **Before:** console prompts, such as `Passphrase (leave empty to skip):`, `TPM2 PIN (leave empty to use another
  method):` and `Key file passphrase (leave empty to use another method):`, with what went wrong in red lines above
  them, and `Insert the key device ...` while waiting for a key device.
- **Now:** the unlock dialog of the volume, which names its device, the tries left and what went wrong last (a wrong
  passphrase, a TPM that locked itself against PIN guessing), and the keyboard layout. Enter with nothing typed skips
  the volume's passphrase or moves on from a PIN, as before; Escape does the same. What is typed is never shown during
  start-up. While a key device or security key is awaited, the dialog shows the seconds left: Enter looks again at once,
  Escape stops waiting. A crypttab key script and `fido2-assert` asking for a FIDO2 PIN get the console for the time
  they run, as hooks do, so they can ask there.
- **Why:** the dialog says which volume asks, and why it asks again, where the console said it in lines that scrolled.

### The timed prompts are questions

- **Before:** red lines that counted down, with RETURN and ESCAPE: `Unable to import <pool>, retrying in <n>
  seconds`, `LUKS device <device> is missing`, `One or more required devices are missing`, and the warning about
  unsupported pool features.
- **Now:** questions with a title, what happened, the countdown and two buttons: Enter does what RETURN did (try or
  look again, go on), Escape what ESCAPE did (the recovery shell, skip, stop waiting). They go on by themselves after
  the same delays, `zbm.retry_delay` included.
- **Why:** the choice and its time limit are on the screen, not in the help.

## The menu

The `fzf` menu is replaced by the new menu, designed in [docs/dev/menu-design](docs/dev/menu-design/README.md). It
runs as `/bin/zfsbootmenu` and `zbm`, as the old one did, so it also opens over SSH and from the recovery shell, and
takes over from a menu already running.

### Keys

The tables give each key of the `fzf` menu, screen by screen, what it did, and the keys that do it now: in the
workspace, the menu, and in the classic menu, which `zbm.menu=classic` keeps for this release. `MOD` is Alt, Ctrl or
Ctrl-Alt, as in the `fzf` menu, whose `MOD+key` bindings both menus keep as aliases in the view where they had a
meaning, but for those under **Gone**. `Alt+C`, `Alt+L`, `Alt+N` and `Alt+P` mark the aliases that do not work with Ctrl
alone, as the menu binds those letters with Ctrl itself. On a serial line or over SSH, Ctrl-J arrives as `Enter`, so
`MOD+J` takes Alt there; and on a serial line, flow control may keep Ctrl-S from the menu, so `MOD+S` takes Alt. The new
letters take either case: Shift and Caps Lock change no action. On the console on the display (`zbm.console=drm`), other
keyboard layouts reach the keys too: a key whose character means nothing to the menu counts as the key in its place on a
US keyboard (on a Russian layout, `й` is `q`); on a serial line and over SSH, and on the kernel console until `Ctrl-K`
chose a layout, characters count as typed, and the arrows, `Enter` and the command line (`:`), which lists what only a
character reaches, reach every action. [zfsbootmenu(7)](docs/man/zfsbootmenu.7.rst), The Menu's Keys, lists every key of
the workspace and every alias, and `?` the keys of the view it is pressed in. The recovery shell's `help`, which opened
the legacy menu's help pages with the `fzf` keys, shows `zbm help`'s three topics, written from the code: the shell's
commands, the menu's keys (`help keys`, in the key map that `zbm.menu` sets) and the kernel command line's options.

Every list:

| `fzf` key | What it did | Workspace | `zbm.menu=classic` |
| --- | --- | --- | --- |
| `MOD+H` | the help pages of the screen | gone; `?` or `F1` in a view: the keys of the view | the same |
| `MOD+L` | the log (`zlogtail`) | `4`, the Log tab; `Alt+L` in the four lists below | `l`; `Alt+L` |
| `MOD+T`, at loglevel 7 | `ztrace` | gone | gone |
| `F12`, at loglevel 7 | a debugging shell | gone; `!` opens the recovery tools, the shell first | the same |
| typing | filtered the list | `/`, then typing; `Enter` acts on the selection, `Esc` clears the filter | the same |
| `Enter` | chose the selection | `Enter` | `Enter` |
| `Esc`, `Ctrl-C`, `Ctrl-G`, `Ctrl-Q` | back to the boot environments, whose list was read again | `Esc`, `Ctrl-C` or `h`: back to the Boot tab, where they clear the filter | `Esc`, `Ctrl-C`: back to the boot environments |
| `Up`, `Down`, `Ctrl-P`, `Ctrl-N`, `Ctrl-K`, `Ctrl-J`, where the screen did not take them | moved, from the last row round to the first | `Up`, `Down`, `Ctrl-P`, `Ctrl-N`, `k`, `j`, which stop at the first and last rows | `Up`, `Down`, `Ctrl-P`, `Ctrl-N` |
| `PgUp`, `PgDn` | a page | `PgUp`, `PgDn`; `Home`, `End` | the same |
| `Backspace`, `Ctrl-U`, `Ctrl-W`, `Ctrl-A`, `Ctrl-E` and fzf's other editing keys, where the screen did not take them | edited the filter | `Backspace` and `Ctrl-U` in a field | the same |
| `Shift-Up`, `Shift-Down`, the mouse | scrolled the preview, chose | gone: the inspector shows the selection | gone |

The boot environments:

| `fzf` key | What it did | Workspace | `zbm.menu=classic` |
| --- | --- | --- | --- |
| `Enter` | booted the environment, with its default kernel | `Enter`, `b` | the same |
| `Right` | the snapshots | the kernels; the snapshots are `s` or `2` | nothing; `s` |
| `MOD+K` | the kernels | `l`, `Right`; `MOD+K` | `k`; `MOD+K` |
| `MOD+S` | the snapshots | `s`, `2`; `MOD+S` | `s`; `MOD+S` |
| `MOD+D` | made it its pool's default (`bootfs`) | `d`; `MOD+D` | the same |
| `MOD+E` | edited the command line, kept for every later boot | `e`, whose `Enter` boots with the line, kept for the environment's later boots until `MOD+T`; `MOD+E`; saved for every boot by `Set the Command Line of …` on `:`, after its dry run | `e`, whose `Enter` boots with the line, kept for the environment's later boots until `MOD+T`; `MOD+E`; no save |
| `MOD+T`, after an edit | dropped the edited line | `MOD+T` drops the environment's edit; with none, it does nothing and says nothing | the same, and `t` |
| `MOD+P` | the pools' status | `3`, the Pools tab; `Alt+P` | `p`; `Alt+P` |
| `MOD+R` | the recovery shell | `!`, the recovery tools, the shell first; `MOD+R` | the same |
| `MOD+J` | a chroot into the environment | `!`, then Chroot; `MOD+J` | `j`; `MOD+J` |
| `MOD+W` | imported its pool read/write, or read-only again | `w`: read/write; read-only again by `i` or `Enter` on the pool in the Pools tab; `MOD+W` | `w`; read-only again by `i` or `Enter` in the pools; `MOD+W` |
| `MOD+O` | the next sort order of the list | gone | gone |
| `MOD+X` | the power menu | `x`; `MOD+X` | the same |

The kernels of an environment:

| `fzf` key | What it did | Workspace | `zbm.menu=classic` |
| --- | --- | --- | --- |
| `Enter` | booted the environment with this kernel | `Enter`, `b` | the same |
| `MOD+D` | made it the default kernel | `d`; `MOD+D` | the same |
| `MOD+U` | inherited the default kernel again | `d` on the default kernel; `MOD+U` | the same |
| `MOD+E` | edited the command line, with this kernel | `e`; `MOD+E` | the same |
| `Left` | the snapshots | back to the boot environments; the snapshots are `2` | nothing; `Esc`, then `s` |
| `Right` | the pools' status | nothing; `3` | nothing; `Esc`, then `p` |
| `Esc` | back to the boot environments | `Esc`, `h`, `Left` | `Esc` |

The snapshots of an environment:

| `fzf` key | What it did | Workspace | `zbm.menu=classic` |
| --- | --- | --- | --- |
| `Enter` | duplicated the snapshot into a new environment, named on the console | `Enter` opens the snapshot's actions, Duplicate among them; `y` | the same |
| `MOD+X` | cloned and promoted it | `m`; `MOD+X` | the same |
| `MOD+C` | cloned it | `c`; `Alt+C` | the same |
| `MOD+N` | a new snapshot | `n`; `Alt+N` | the same |
| `MOD+D` | the differences since the snapshot, or between the two selected | `f`; `MOD+D` | the same |
| `MOD+R` | rolled back, once `ROLLBACK` was typed | `r`, then its review page, confirmed by the environment's name; `MOD+R` | `r`, then an alert, confirmed by the environment's name; `MOD+R` |
| `MOD+J` | a read-only chroot into the snapshot | `Enter`, then Chroot; `MOD+J` | `j`; `MOD+J` |
| `MOD+O` | the next sort order | `o`; `MOD+O` | the same |
| `Tab`, `Shift-Tab` | selected or deselected, up to two, for the differences | `Space` marks, as many as wanted; `Tab` is the next tab | `Space` or `Tab` marks |
| `Left`, `Esc` | back to the boot environments | `Esc`, `h`, `1` | `Esc` |
| `Right` | the kernels | `1`, then `l` | `Esc`, then `k` |
| none | nothing destroyed a snapshot, or booted one | `Delete` destroys the marked or the selected; `b` boots a clone; `z` folds | `b` boots a clone |

The pools' status:

| `fzf` key | What it did | Workspace | `zbm.menu=classic` |
| --- | --- | --- | --- |
| `MOD+R` | rewound the pool to its checkpoint, once `REWIND` was typed | `i`, then the rewind, on its review page; `MOD+R` | `h`, or `i` then the rewind; `MOD+R` |
| `Enter` | back to the boot environments | the pool's details; on a device, its kernel messages; on a volume, its unlock | the pool's details; on a volume, its unlock |
| `Left` | the kernels | nothing; `1`, then `l` | nothing; `Esc`, then `k` |
| `Esc` | back to the boot environments | `Esc`, `h` | `Esc` |
| none | only the imported pools were listed | `i` imports, `u` unlocks a volume, `w` imports read/write, and on a pool imported read/write says so and does nothing | the same |

The other screens:

| Key | What it did | Workspace | `zbm.menu=classic` |
| --- | --- | --- | --- |
| `Enter` | the countdown: booted at once | stops the countdown and boots the selected environment, the one it would boot | the same |
| `Esc` | the countdown: the menu | stops the countdown; the menu shows already | the same |
| other keys | the countdown: nothing | stop the countdown | the same |
| typing, `Up`, `Down`, `Enter` | the help viewer: chose a topic | gone: help shows the keys of the view it is opened from | the same |
| `PgUp`, `PgDn` | the help viewer: scrolled the page | `PgUp`, `PgDn`, `Up`, `Down`, `k`, `j` | `PgUp`, `PgDn`, `Up`, `Down` |
| `Esc` | the help viewer: back | `Esc`, `h` | `Esc` |
| typing | the differences: filtered the paths | `/`, then typing | the same |
| `Enter`, `Esc` | the differences: back | `Esc`; `Enter` restores the marked files, or the selected one | the same |
| none | the differences did nothing else | `Space` marks, `Left` and `Right` show the files or the packages | the same, and `Tab` marks |
| `Up`, `Down`, `Enter`, `Esc` | the power menu | `x`, then the same | the same |
| `Enter`, `Ctrl-C` | a name typed on the console: took it, gave up | the name's dialog: `Enter`; `Esc` or `Ctrl-C`; `Backspace`, `Ctrl-U` | the same |
| typing `ROLLBACK`, `REWIND` | confirmed a rollback, a rewind | the target's name, on the review page | the target's name, in an alert |
| typing `NORESUME`, `DANGEROUS` | allowed a read/write import while a suspend partition was active | the same, on the console the change takes | the same |

**Gone:**

- `MOD+H`, the help: `?` and `F1` open it, and `Ctrl-H` arrives as `Backspace` on a serial line and over SSH.
- `MOD+O` in the list of boot environments: they carry no creation time or size to sort by; `o` and `MOD+O` still
  sort the snapshots.
- `MOD+T` (`ztrace`) and `F12` (a debugging shell) at loglevel 7; `ztrace` stays in the recovery shell.
- `Ctrl-G` and `Ctrl-Q`, which left a list: `Esc` and `Ctrl-C` do.
- `Ctrl-K` and `Ctrl-J` as moves: `k` and `j` move in the workspace.
- The filter's editing keys but `Backspace` and `Ctrl-U`.
- The preview's scrolling, and the mouse.
- The help viewer's topics: help lists the keys of the view it is opened from.
- The arrows' jumps from screen to screen (`Right` from the environments to the snapshots, from the kernels to the
  pools and from the snapshots to the kernels; `Left` back): the tabs and their digits replace them.
- The lists' wrapping from the last row to the first (`--cycle`).

**Changed meaning:**

- `Right` on the boot environments: the snapshots before; now, in the workspace, the kernels, and nothing in the
  classic menu.
- `Left` on the kernels: the snapshots before; now, in the workspace, back to the boot environments, and nothing in
  the classic menu.
- `Ctrl-L`, in every list: the log before; now Redraw, and the log is `Alt+L`.
- `Ctrl-C` on the snapshots: a clone before; now back.
- `Ctrl-N` on the snapshots: a new snapshot before; now the row below.
- `Ctrl-P` on the boot environments: the pools' status before; now the row above.
- `Ctrl-J` on a serial line or over SSH: a chroot on the boot environments and the snapshots, the row below on the
  kernels and the pools' status, before; now `Enter`, which boots on the boot environments and the kernels, opens the
  snapshot's actions on the snapshots, and the pool's details on the pools' status.
- `Tab` on the snapshots: a selection before; now, in the workspace, the next tab.
- `Enter` on the snapshots: a duplicate before; now the snapshot's actions.
- `Enter` on the pools' status: back to the boot environments before; now the pool's details.
- Typing: a filter before; now letters are keys, and `/` starts the filter.

## The menu's Boot tab

The boot environments and their kernels: the main view and the kernels of the classic menu.

### The countdown shows the menu

- **Before:** one line, `Booting <environment> in <n> seconds`, with Enter to boot at once and Escape for the menu.
- **Now:** the menu itself, with the environment to boot selected and `Booting <environment> in <n> s. Press any key
  to choose another.` above the list. Any key stops the countdown; Enter boots the selected environment, which is the
  one the countdown would have booted, and Escape only stops the countdown. `zbm.timeout`, `zbm.show` and `zbm.skip`
  mean what they meant.
- **Why:** what is about to boot, and what else there is, can be seen without a key press.

### No countdown to a snapshot

- **Before:** there was no boot counting, so nothing booted in place of a default that failed. The menu's design had
  the countdown boot a clone of the default's newest snapshot when no environment had marked a boot good.
- **Now:** the countdown boots only environments. When the default used up its boot tries, it boots the last
  environment that marked a boot good; with none, nothing boots on its own, and the banner says only what failed, as
  it does with the countdown off. A snapshot is booted only when someone chooses it, under the name they give its
  clone.
- **Why:** the countdown cannot ask for the clone's name, and nobody may be there to find and destroy a clone it made;
  the environment the list selects first is the default, which must not boot again.

### Encrypted environments are listed locked

- **Before:** the menu asked for the key of every locked encryption root before it showed the list, one prompt after
  another.
- **Now:** a locked environment is listed as `locked`, and its key is asked for when it boots, from its key source
  (`org.zfsbootmenu:keysource`) first, as boots chosen in the menu did. Its kernels are listed once its key is loaded.
- **Why:** the menu shows at once, and only the environment that boots needs its key.

### Environments without kernels are listed

- **Before:** an environment without a kernel was left out of the list; with none at all, the menu printed
  `No boot environments with kernels found` and opened the recovery shell after 10 seconds.
- **Now:** an environment without a kernel is listed, with `0 kernels`; with none at all, the menu says
  `No Boot Environments` and how to get to the pools (the Pools tab, `3`; `p` in the classic menu) and the recovery
  tools (`!`), and waits.
- **Why:** an environment that is there but cannot boot is a problem to see, not to hide, and the menu is a better
  place to recover from than a shell that opens by itself.

### A key that cannot be loaded at the countdown

- **Before:** the recovery shell, with `unable to load key for <environment>`.
- **Now:** the menu says `unable to load key for <environment>` and why; another environment can be chosen, or the
  recovery shell opened from the recovery tools.
- **Why:** the same as above; the shell is one key away.

### An edited command line is for one environment

- **Before:** the edited line was saved in `/zfsbootmenu/cmdline` and used for every later boot of any environment
  until Ctrl-T cleared it.
- **Now:** the edited line boots in place of the environment's `org.zfsbootmenu:commandline`: `%{parent}` in it stands
  for the parent's line, `root=` is ZFSBootMenu's, and `noresume` and the host ID are added when they apply, as for the
  environment's own line. A line the environment inherits opens with its parent's arguments in place of `%{parent}`,
  once, so they do not repeat. Nothing is saved by `e`: the menu keeps the line for that environment alone, so a boot
  that failed can be tried again with it, until `MOD+T` drops it. In the workspace a line is saved for every boot only
  by a command: `Set the Command Line of …` on `:` opens the editor for every boot, and its `Enter` shows the dry run
  (the pool imported read/write, the `zfs set org.zfsbootmenu:commandline=…`, the line it replaces, and the undo) before
  anything is written; `Let … Inherit Its Command Line` drops a line set on the environment. A kernel's `.kcl` file and
  `zbm.kcl_override` still come before the property. The locked menu asks for the PIN first; the classic menu does not
  save.
- **Why:** the edit was made for one environment, and should not decide how the others boot; a line kept for every boot
  belongs on the environment, where `zfs get` shows it, and is written only once what it changes was read.

### A new default needs no key

- **Before:** setting the default loaded the key of the pool's root dataset first, which could ask for a passphrase.
- **Now:** only the pool is imported read/write; `zpool set bootfs` needs no key.
- **Why:** the passphrase was asked for nothing.

### Inheriting the default kernel asks for no key

- **Before:** `set_default_kernel` loaded the environment's key, and went on when it did not load, also to inherit
  the default kernel again.
- **Now:** inheriting imports the pool read/write only; `zfs inherit` needs no key. Setting a default kernel still
  loads the key, as finding the kernel's file mounts the environment, and fails when it does not load.
- **Why:** the passphrase was asked for nothing, as for a new default environment.

### The default kernel names its kernel alone

- **Before:** `org.zfsbootmenu:kernel` was set to the kernel's file name followed by `$`, read as an extended regular
  expression: the dots of `vmlinuz-6.1.10` matched any character, a `+` in a version repeated what came before it, and
  `linux-6.1$` also matched `vmlinux-6.1`.
- **Now:** the value is `/`, the file name with the metacharacters escaped, then `$`: `/vmlinuz-6\.12\.9\+$`. Bash's
  `=~` reads it the same way, so the legacy runtime boots the same kernel from it.
- **Why:** the kernel chosen is the one that boots, whatever its version holds.

## The menu's Snapshots tab

The snapshots view and the main view make the changes of the legacy menu: a new default and a default kernel (in the
Boot tab above), a snapshot, a rollback, a clone (promoted or not), an independent copy, and the differences since a
snapshot. Each that writes its pool imports it read/write first and loads the environment's key, as before, with the
same commands. The menu also restores files from a snapshot, which the legacy menu did not.

### The changes run in the menu

- **Before:** each change cleared the screen, printed what it did, and asked on the console: the name, the
  hibernation question of a read/write import, a passphrase.
- **Now:** a change on a pool imported read/write, in an environment whose key is loaded, runs inside the menu, which
  says how it went in its notification line, or in an error dialog with the reasons. Otherwise the menu hands the
  console over, as for a boot, and the hibernation question and passphrase prompts are those of before.
- **Why:** the usual change needs no question, and the menu shows the result where it shows everything else.

### Snapshots are read when their view opens

- **Before:** the snapshot screen ran `zfs list -t snapshot -H -o name -S <sort>` for its environment each time it
  opened.
- **Now:** at start, and after each change, the menu reads only each environment's count of snapshots and its newest,
  which the details show, from one `zfs list -H -p -t snapshot -o name,creation,used -d 1 -s creation` of the
  environment (its descendants' snapshots left out). The snapshots view asks for every snapshot of its environment as it
  opens, and says "Reading the Snapshots..." until they come; after Esc stops the read, it says the snapshots are not
  read, and opening the view again, or the next change, reads them. A read that fails says why once: only opening the
  view again reads them then. While the countdown runs nothing is read for it. Once read, they are read again with each
  change, so that the rollback's count of the snapshots it destroys is never from an older read. The command line finds
  the snapshots read so far, and a new name is known to be taken only among them: a snapshot of a view never opened is
  refused by `zfs`, whose message the error shows. An environment with more snapshots than a menu over SSH can be sent
  (some 70,000) ends in an error that says so, on the console too, so that both menus show the same.
- **Why:** with thousands of snapshots, reading them all for every environment at start and after every change
  slowed the menu down; ten thousand now open, filter and scroll within a frame.

### New names are typed in a dialog

- **Before:** `New boot environment name` or `New snapshot name` on the console, filled with `<environment>_NEW` or
  the time (`%Y-%m-%d-%H%M%S`); characters outside `[a-zA-Z0-9-_.:]` were removed after Enter, with a message.
- **Now:** a dialog with the same name filled in, which only takes those characters, and refuses a name the menu knows
  already: an environment's, or a snapshot's of those read. The time of a snapshot's name is in UTC, as the image has
  no time zone.
- **Why:** the name is part of the action, and what can be typed is what can be used.

### A snapshot boots as a new clone under a chosen name

- **Before:** `Enter` on a snapshot asked for the name of a new environment, filled with `<environment>_NEW`, and
  copied the snapshot into it; nothing booted a snapshot, and the new environment was booted by hand.
- **Now:** booting a snapshot (`b`, the snapshot's `Enter` menu or the command line) asks for the name the same way,
  clones the snapshot into it with `zfs clone`, as a clone without promotion does, and boots it, with its own kernel and
  command line, through the menu's boot: the hooks, the key and `kexec`. From the workspace's command line, its plan,
  the clone and its command, shows first, as for any change. It takes the console, as a boot does, and the locked menu
  keeps it. A clone that fails boots nothing; a boot that fails leaves the clone, and says so.
- **Why:** booting a snapshot is one step, and what boots is an environment with a name that stays, not a temporary
  clone that has to be found and destroyed later.

### The chroot goes through the lock

- **Before:** `mod-j` ran `zfs-chroot` on the console for the environment or snapshot selected; nothing stood in the
  way of the shell it opened.
- **Now:** the chroot goes through the one gate to a shell (ADR 0010): a locked menu asks for the PIN first, and
  imports, mounts and runs nothing without it; the proof is forgotten once the shell ends. No `/bin/zfs-chroot`
  script runs: ZFSBootMenu prepares the chroot itself, in the script's order, and the menu offers it on every image.
  Its question, read/write or read-only, is asked on the console, as before, and over SSH on the client's terminal.
  When no shell opens, as when no shell runs in the filesystem, the menu says it failed and why, where it used to
  come back as if a shell had run.
- **Why:** a shell in a chroot is a shell, and the lock keeps every shell.

### A chroot loads the environment's key first

- **Before:** `zfs-chroot` loaded the key of the filesystem's encryption root only after the question was answered
  read/write; a read-only chroot of a locked environment failed to mount it.
- **Now:** a chroot loads the environment's key first, read-only or read/write, as a boot chosen in the menu loads
  it: the `load-key.d` hooks, then the prompt. A key refused, or none given, ends the chroot with nothing mounted.
  This holds for the menu's chroot, the recovery shell's `zfs-chroot` and a hook's `zfs_chroot` alike.
- **Why:** the menu's command line already marks a chroot of a locked environment with the key it costs; now the
  chroot does what it says.

### A hook's `zfs_chroot` asks for the PIN in a locked image

- **Before:** the library's `zfs_chroot` ran the `/bin/zfs-chroot` script on Bash, which opened the chroot's shell
  at once.
- **Now:** it runs `zbm`'s `zfs-chroot` on the console, after the same log lines, clear screen and cursor, and
  returns its status: in a locked image that asks for the PIN first, and a blank line or three wrong PINs open
  nothing. A hook that calls it waits there for the PIN. Without a filesystem it still logs "fs is undefined" and
  returns 1.
- **Why:** a hook is no way past the lock: every shell opens through its gate.

### The shell's `zfs-chroot` asks for the PIN again

- **Before:** `/bin/zfs-chroot`, a Bash script, opened the chroot from the recovery shell at once.
- **Now:** `/bin/zfs-chroot` is a link to `zbm`, and prepares the chroot as the menu's does. In a locked image it asks
  for the PIN as `zbm lock shell` does, even in a recovery shell that was opened with the PIN; a blank line or three
  wrong PINs open nothing. It exits 0 once a shell ran, 1 when none could be opened, saying why on standard error, and
  0 without a filesystem, as before. A second argument is a usage error (status 2), and `--help` prints its usage.
  `SIGHUP`, `SIGINT`, `SIGQUIT` and `SIGTERM` wait until it has unmounted the chroot, as the script's trap undid the
  mounts: the chroot's shell gets them, and a dropped SSH session leaves nothing mounted. So Ctrl-C no longer ends it
  at the PIN or at the question, where the script ended at once; a blank line at the PIN goes back.
- **Why:** each process keeps its own proof: the recovery shell's ended with it, and a chroot is a shell of its own.

### A filesystem name with a space is one argument

- **Before:** `zfs_chroot` ran `bash -c "zfs-chroot ${fs}"`, which split a name with a space into words: the chroot
  was of the name's first word.
- **Now:** the name reaches `zfs-chroot` as one argument.
- **Why:** ZFS names may hold spaces, and the chroot is of the filesystem named.

### Rollback has no safety snapshot

- **Before:** typing `ROLLBACK` confirmed the rollback, which destroys the newer snapshots.
- **Now:** typing the environment's name confirms it. In the workspace, the rollback's review page (see "Changes are
  reviewed before they are made") lists the newer snapshots and bookmarks it destroys, with their sizes, and offers
  to clone the snapshot instead; in the classic menu, the confirmation counts the snapshots destroyed and their size.
  The menu's design had a safety snapshot taken first; ZFS destroys it with the other newer snapshots, so there is
  none, and both say to clone the snapshot instead to keep things as they are. A clone or a hold of a newer snapshot,
  which makes `zfs rollback -r` refuse, is found before the pool is imported read/write: the rollback fails at once
  and names it, where a hold used to stop `zfs` only after it had destroyed the other newer snapshots.
- **Why:** the name says which environment loses its changes.

### Clones leave encryption properties out

- **Before:** a clone was given every property set on its environment, `keylocation` and `keyformat` included, which
  ZFS refuses on a clone when the environment is its own encryption root.
- **Now:** the clone gets the other properties; it shares the key of its origin's encryption root, as every clone does.
- **Why:** cloning an environment that is its own encryption root failed.

### The differences of two snapshots leave the pool read-only

- **Before:** the differences always imported the pool read/write first (`zfsbootmenu-diff`), which could ask the
  hibernation question, also to compare two snapshots.
- **Now:** two snapshots are compared on the pool as it is imported, the environment mounted read-only once its key
  is loaded; the menu takes the console only when that key must be asked for. The differences against the live
  filesystem still import the pool read/write, as `zfs diff` takes a snapshot of the filesystem for them.
- **Why:** comparing two snapshots writes nothing, and a pool imported read-only should stay so for it.

### Files are restored from a snapshot

- **Before:** the differences since a snapshot could only be read; a file was put back by hand from the recovery
  shell.
- **Now:** in the differences against the live filesystem, `Enter` restores the marked paths, or the selected one,
  after the environment's name is typed. The pool is imported read/write, the key is loaded, and a safety snapshot
  `<environment>@before-restore-<UTC time>` (`%Y-%m-%d-%H%M%S`, `-2` and on for another in the same second) is taken
  first; each path is copied back with `cp -a` from the snapshot's read-only `.zfs/snapshot/<name>` into the
  environment, mounted read/write, keeping owner, mode and times. Nothing is deleted: a file created since the
  snapshot stays, a directory the environment has is left as it is, and the partial copy of a directory that failed
  is left, named in the failure. A path that is not absolute and normalised, that has a symbolic link above it in the
  environment or the snapshot, or that is a directory in only one of them, is refused before anything is written; the
  path itself is replaced, never written through. The differences of two snapshots restore nothing, as they did not
  look at the live files.
- **Why:** undoing a bad edit or upgrade of a few files should not need a rollback, which destroys the newer
  snapshots, and rolling back to the safety snapshot undoes the restore.

### Snapshots are marked as a set

- **Before:** `Tab` selected up to two snapshots for the differences (`fzf --multi 2`); the Rust runtime's menu,
  the workspace and the classic menu alike, dropped the first mark when a third was set.
- **Now:** in the workspace and the classic menu alike, `Space` marks as many snapshots as the user likes, and drops
  none; in the classic menu `Tab` marks too, as before. `f` compares the two marked, the older against the newer
  whichever was marked first (two taken in the same second in the order ZFS took them), or the one marked with now;
  with three or more, or while their list is not read, it compares nothing and says so. A list of snapshots read
  again, after a change, takes the marks of those it no longer has. The line view says `marked` after each.
- **Why:** a mark that goes by itself, unseen, is a surprise; and a comparison of three snapshots has no meaning, so
  the menu says so rather than choose two of them.

### Changes are reviewed before they are made

- **Before:** the legacy menu asked to type `ROLLBACK` before a rollback and `REWIND` before a rewind, and, while a
  suspend partition was active, `NORESUME` or `DANGEROUS` before the first change that imported a pool read/write, none
  listing what the change would lose; the Rust runtime's menu asked for the target's name in an alert, which counted
  what a rollback destroys from the snapshots it had read.
- **Now:** in the workspace a rollback, a destroy, a forced import and a rewind open a review page, in red, which asks
  the machine for the change's plan and shows it: what is lost (the snapshots and bookmarks with their sizes and the
  space they free, what was written since the snapshot, or for a rewind every write since the checkpoint, with the
  filesystems and snapshots made since), what is kept, the steps with their commands, the undo or that there is none,
  a safer action (a clone of the snapshot instead of a rollback, a read-only import instead of a rewind), and, for an
  import, who else may use the pool: this host, or the host ID and name it last had and when it was last written. A
  clone or a hold of a snapshot the change destroys, or another system that has the pool imported now, makes the page
  refuse the change and name what stops it. Otherwise the change is made once the name is typed: the environment's
  last component for a rollback, the snapshot's short name for the destroy of one, the environment's last component
  for several, and the pool's name for an import. `Tab` goes to the safer action, `Escape` closes the page and changes
  nothing. In the line view the page is lines, which cannot scroll or take the safer action. The classic menu keeps
  its confirmations, and is otherwise unchanged.
- **Why:** what a change destroys should be read before its name is typed, from the machine as it is, not from what
  the menu happened to read; and where ZFS would refuse, the menu says so before it imports anything read/write.

### Snapshots are destroyed from the menu

- **Before:** the legacy menu destroyed no snapshot: that took `zfs destroy` in the recovery shell.
- **Now:** in the workspace, `Delete` on the Snapshots tab destroys the marked snapshots, or the selected one, after
  their review page; `Destroy It…` in a snapshot's `Enter` menu destroys that snapshot, and the command line has an
  entry for the selected snapshot and one for the marked ones. Only snapshots of one boot environment are destroyed, in
  one `zfs destroy <environment>@a,b,c` that destroys all or none, and their bookmarks stay; a clone or a hold of one
  stops it before the pool is imported read/write. The locked menu asks for the PIN first. Over SSH only the snapshots
  of the machine's boot environments are destroyed. The classic menu does not destroy.
- **Why:** reclaiming the space of old snapshots, or removing a bad one, needs no shell, and the review page says
  what it frees before anything is destroyed.

## The menu's Pools tab

The pools, their devices and the LUKS volumes: the pools view of the classic menu. The forced import and the rewind
are reviewed as the rollback is, under the Snapshots tab:
[Changes are reviewed before they are made](#changes-are-reviewed-before-they-are-made).

### A pool that may not be written

- **Before:** `org.zfsbootmenu:readonly` or a degraded pool showed `Operation prohibited` for ten seconds.
- **Now:** the change fails with an error dialog that says why: the property's value, or that the pool is degraded.
- **Why:** the reason stays on the screen until it is read.

### One import for every way to import a pool

- **Before:** `mod-w` exported a pool and imported it read/write (`set_rw_pool`), or read-only again
  (`set_ro_pool`); the rewind to a checkpoint (`rewind_checkpoint`) asked for `REWIND`, then exported the pool and
  imported it read/write with `--rewind-to-checkpoint`. An import that failed after the export left the pool exported.
  The menu could not import a pool that was not imported.
- **Now:** the menu's import of an imported pool is read-only or read/write again, or the rewind, confirmed by the
  pool's name, on its review page in the workspace, and refused when the pool has no checkpoint; each exports the pool
  and imports it under `zbm.import_policy`, with the property, degraded and hibernation checks of before for
  read/write, and a failed import imports the pool read-only again. The import after the export names the pool by its
  `guid`, read before the export, so that a pool that is not imported and has its name is not in the way;
  `set_rw_pool` of the hook library does the same. A pool that is not imported is imported read-only as this host, as
  the host that last imported it, whose host ID is then recorded for the boot as start-up records it, or forced,
  confirmed by the pool's name and refused when another system has the pool imported now; a read/write import or a
  rewind then exports it and imports it again, with the same checks. Every import takes the console, and the
  locked menu keeps every one but a read-only import as this host.
- **Why:** one action says how a pool is imported, and a failed import tries to import the pool read-only again
  instead of leaving it exported; the pool stays exported only when that import fails too.

### The menu lists the pools that could be imported

- **Before:** the pool screen listed the imported pools (`zpool list`) and previewed `zpool status -v`; a pool that
  was not imported was not shown, nor the host that used it last, and two pools of one name could not be told apart.
- **Now:** once its first frame is drawn, the menu scans for the pools that could be imported (`zpool import`, which
  looks at every device) and lists them after the imported ones, each with its identifier; for one that another system
  used last, `zdb -e -C` reads that host's ID and name, and only for it. The scan waits for the countdown to be stopped,
  so that the boot the countdown ends with never waits for it; it runs again after an import, opening a volume or a
  chroot, and from the command line (Rescan the Pools), and the snapshots read in between list what it found. Each
  imported pool carries its devices from one `zpool status`, their states kept apart (UNAVAIL, REMOVED, FAULTED,
  OFFLINE), with their error counts. A pool that shares its name with another is imported by its identifier, which its
  row shows, and, as the host that last imported it, with the host ID its configuration records; one named like an
  imported pool is shown but not offered for import. `zbm debug pools` scans too, and lists the pools that could be
  imported after the imported ones.
- **Why:** a pool that did not import, or a missing disk, is what the menu is opened for; the scan reads the labels of
  every disk, which takes seconds, so it runs once the menu shows and not with every snapshot.

### The menu exports a pool, and shows a disk's kernel messages

- **Before:** the menu exported a pool only on its way to something else: before `kexec`, around `set_rw_pool` and
  `set_ro_pool`, before a reboot and when LUKS volumes were closed (`export_pool`); a user could not export one. The
  kernel log was the recovery shell's (`dmesg`).
- **Now:** the dialog of an imported pool (`Enter` on it) and the command line offer Export: `zpool export` through the
  ZFS port, with nothing written on the console; a busy pool fails with the reason `zpool` gives. The pool of the
  default environment may be exported too, after which the menu has no default. The locked menu asks for the PIN first.
  The pools are scanned again after it, so the pool is listed to import. In the workspace, `Enter` on a disk in the
  Pools tab's tree shows below the pools the kernel's messages that name it, the disk of a partition or the devices
  below a mapping, or that the kernel tagged with one of them, without a PIN under the lock. Messages that name only a
  port, as `ata3.00:` does, are not among them, and the panel says so.
- **Why:** a pool imported by mistake, or one to hand to another machine, can be let go without a shell; and why a
  disk went missing is in the kernel's messages about it.

### Volumes open from the menu

- **Before:** the menu opened no LUKS volume. A volume that start-up skipped was opened with `zluks` in the recovery
  shell or over SSH.
- **Now:** a locked volume opens from the pools view (`u` or `Enter` on its row), on the console and in the menu over
  SSH, in an image with `cryptsetup`. A keyholder opens it (`zbm unlock serve`, SB10), as at start-up, and asks in the
  menu's unlock dialog: the passphrase or PIN, a key device or security key to wait for with the seconds left, a
  volume to retry. Escape skips the method, as at start-up. Cancelling the action, or another menu taking over, closes
  the dialog and ends the keyholder at once, also while it waits for a TPM2 or a security key; a volume it opened by
  then stays open, and the menu says so. The keyholder's programs leave the menu's terminal alone, so a crypttab key script and
  `fido2-assert` asking for a FIDO2 PIN cannot ask there: from the menu they fail, and start-up or `zluks` opens such
  a volume.
- **Why:** the volume and its key methods are on the screen, and opening it is one key, not a shell.

### A volume opened over SSH does not open the lock

- **Before:** the legacy runtime had no lock. In the Rust runtime, a locked menu (ADR 0010) lets a person who opened
  a volume with a passphrase, a PIN or a key device, at start-up or with `zluks`, do what it keeps without the PIN;
  the menu over SSH opened no volume.
- **Now:** a volume opened over SSH is recorded as opened remotely (`<name>\tremote` in
  `/zfsbootmenu/luks-unlocked`): in the menu over SSH, by start-up run from it, or with `zluks` in an SSH login
  (`SSH_CONNECTION`). It counts neither as attended, so the locked menu and the recovery shell still ask for the PIN,
  nor as unattended, so the boot is not taken for one that unlocked itself. One opened in the menu on the console is
  attended, as at start-up.
- **Why:** a passphrase typed over SSH shows who knows it, not who is at the machine; the PIN keeps the shell.

## The menu's Log tab

The log since start-up, and the boots saved before: the log view of the classic menu.

### Saved logs record what was booted

- **Before:** nothing of a boot survived `kexec`: the kernel log that `logs` and `zlogtail` showed was gone with the
  memory of the runtime.
- **Now:** with `zbm.save_logs`, each boot's directory on the ESP (`EFI/zfsbootmenu/logs/<time>`) holds, beside its
  journal and kernel log, `boot.json`: one line of JSON with `"schema": 1`, the time of the save, the boot
  environment, the kernel and initramfs, the command line as it was loaded and whether it was typed for this boot,
  the device tree, what loaded the kernel (`kexec_file_load` or `kexec`) and whether the files loaded were the copies
  measured into the TPM (the event log beside says what the TPM took). It is written last, to `boot.json.tmp` and
  then renamed, so a directory with a record holds its boot's logs whole, and the logs written again before `kexec`
  go through a `.tmp` file too, so that a write that fails leaves the saved ones as they were;
  a record that cannot be written fails the save as a full partition does, with a warning, and the boot goes on. The
  saves after a crash and those of `zbm debug save` have no record.
- **Why:** the boot history (ADR 0016, point 7) needs to know what each saved boot booted, and the logs alone do
  not say; fields may be added, never changed, and a reader takes the record as text from a disk anyone can write.

### The boot history

- **Before:** the menu showed the kernel's warnings and errors of the boot running (`logs`, `zlogtail`); nothing of
  earlier boots.
- **Now:** with `zbm.save_logs`, the workspace's Log tab lists the boots saved on the ESP, newest first, above the
  log since start: when each loaded which environment and kernel, and whether what was loaded was measured, or that
  it was saved without a record (from the shell, after a crash) or with one this runtime does not read. The
  inspector says when the selected environment was last loaded, and with which kernel. The ESP is mounted read-only,
  at `/run/zbm-saved-logs-read`, for the read alone, and unmounted even when Escape cancels it; at most the newest 16
  boots, 32 files of each and records of 64 KiB are read. Without the option the ESP is not read, and the tab says
  how to keep a history. The read needs no PIN under the locked menu, and is never made while the countdown runs.
  The classic menu shows nothing new.
- **Why:** after a boot that went wrong, the menu should say what was booted last and how (ADR 0016, point 7);
  reading a partition the user did not name for logs would mount it for nothing.

## The menu as a whole

### The menu is the workspace

- **Before:** the `fzf` menu, one list at a time; then, in the Rust runtime, the classic menu: a screen per view,
  each opened by a letter from the boot environments (`k` the kernels, `s` the snapshots, `p` the pools, `l` the
  log).
- **Now:** the workspace (ADR 0007, ADR 0016 points 9 to 11): boot environments, snapshots, pools and the log are the
  tabs of one screen, `1` to `4`, and `Tab` and `Shift-Tab` go from tab to tab. Its top line names the tabs, the worst
  pool, the volumes open and what needs attention, and from 120 columns the host ID and the time. The Boot tab groups
  the boot environments by pool, with the kernels of the one selected beside them (`l` or Right, back with `h`, Left
  or Escape) and an inspector; `j` and `k` move as the arrows do. The Snapshots tab is a timeline grouped by day, week
  and month, newest first, which `z` folds; the Pools tab a tree of the pools, their vdevs and devices, and the
  volumes, with a device's kernel messages on `Enter`. Start-up shows as the Boot tab where the start-up view runs,
  and the help names the ZFS module's version. Keys that moved: the kernels are `l`, not `k`, which moves up; the log
  is `4`, not `l`; the pools `3`, not `p`; the chroot is among the recovery tools (`!`), not `j`; a rewind among the
  imports (`i`), not `h`; marking a snapshot is Space alone, `Tab` choosing tabs. `!` still opens the recovery tools,
  the shell first, and the `MOD+key` aliases stay.
- **Why:** what needs attention, and the parts of the machine that hold it, show on one screen, and the keys move
  as other terminal programs' do (ADR 0007).

### The command line shows a change before it runs

- **Before:** the `fzf` menu had no command line: each screen's keys were the only way to its actions.
- **Now:** `:` opens the command line, in both menus. Its entries come under Recent (the last 20 chosen, kept until the
  menu ends and never written to disk), Actions, Items and Help, each with the keys that do it and, on the right, what
  it costs: `PIN` (the locked menu asks for it), `read/write` (a pool imported read-only is imported read/write first),
  `key` (the environment's key must be loaded) or `review` (a page asks for the target's name). It finds an entry by
  its label or by its command's name and target, such as `rollback zroot/ROOT/void@pre-upgrade`, in the menu's
  language or in English, and ignores case, so Caps Lock changes nothing it finds. `Tab` completes the name, `Ctrl-W`
  deletes a word, `Ctrl-R` shows the entries chosen before, and `F1` lists the keys and the names, which
  [zfsbootmenu(7)](docs/man/zfsbootmenu.7.rst) lists too. In the workspace, a change chosen there first shows its
  plan, on a page like a review page without the name to type: the steps, the command each runs, what is lost, and
  how to undo it. `Enter` runs it, `Esc` leaves it, and nothing has changed. A boot and a chroot run at once, as do
  the keys of the views; the classic menu shows no plan.
- **Why:** a change made from a list of names should say what it will run before it runs, and a command's English
  name finds it whatever language the menu speaks.

### The locked menu marks what it keeps

- **Before:** the legacy runtime had no lock. In the Rust runtime, the locked menu (ADR 0010) asked for the PIN when a
  kept action was chosen, with nothing to say so beforehand.
- **Now:** what the lock keeps carries the word `PIN` after its name: on the toolbar, in the next steps, in the
  command line, in a sheet's entries and on a review page's button, in both menus and in the lines. The countdown and
  the default boot never carry it. Once the PIN was typed the marks go, and they come back when its proof lapses,
  after 5 idle minutes, when the machine would ask again.
- **Why:** nobody should wonder where an action went, or be asked for a PIN they did not expect.

### The recovery shell

- **Before:** Ctrl-R in the menu.
- **Now:** Ctrl-R and Alt-R still open it, as the old `MOD+key` bindings stay as aliases; the menu's own way is `!`,
  the recovery tools, of which the shell is the first. As before, leaving the shell imports the pools that became
  importable in it and opens the menu again.

### Firmware setup is offered only where the firmware can open it

- **Before:** the Rust menu offered Reboot into Firmware Setup on every machine: in its power sheet, its command line
  and its recovery tools, where on a machine that boots from a BIOS it could only fail. The legacy power menu
  (`zpowermenu`) offered it only where the firmware said it could open its setup.
- **Now:** the menu offers it only where the firmware says so (`OsIndicationsSupported`). `zpowermenu` also wanted
  the `OsIndications` variable to exist; where it does not, the menu still offers Firmware Setup, and choosing it says
  that the firmware has no `OsIndications` variable.
- **Why:** the menu offers what it can do.

### `zbm.menu`: lines, a screen, or the classic menu

- **Before:** the menu was `fzf`'s, drawn on the terminal, with nothing to choose.
- **Now:** `zbm.menu=lines` shows the menu as numbered lines a screen reader reads in order, and `zbm.menu=screen`
  as a screen; without either, the lines show by themselves when `TERM` is dumb or unset, or `speakup` is loaded.
  In the lines, the names of the tabs (`boot`, `snapshots`, `pools`, `log`) choose them. `zbm.menu=classic` keeps
  the classic menu, its screens and keys, for this release only, and the lines still show by themselves where they
  would, with the classic keys.
- **Why:** the lines serve screen readers and terminals that cannot draw a screen (ADR 0011); `classic` leaves a
  release to learn the workspace's keys, for users and for scripts that type into the menu (ADR 0016, point 10).

### When the menu cannot start

- **Before:** the countdown of the start-up program booted the default environment before the menu ran, so a menu
  that could not read the machine did not stop the automatic boot.
- **Now:** the menu counts down itself; if it cannot start (it cannot read the pools, or use the terminal), the
  start-up program counts down without it, as before the menu existed. After it found another instance running, it
  never boots by itself, as the legacy program did not.
- **Why:** the automatic boot must not depend on the menu.

### A menu killed by a hang-up

- **Before:** the EXIT trap removed the lock of the running menu (`/zfsbootmenu/active`) on any exit.
- **Now:** a hang-up (`SIGHUP`, a dropped SSH session) ends the menu as `SIGTERM` does, and it removes its lock; a
  lock left by a process that no longer exists (it was killed outright) is removed by the next instance instead of
  waited for.

### `zbm` with a command

- **Before:** `zbm` was the menu's other name, and ignored its arguments: `zbm --help` opened the menu.
- **Now:** it still is, and `zbm <command>` runs that command of the Rust program (`zbm debug crash`, `zbm report`),
  as the messages of the runtime say. The program's own options are its own: `zbm -h` and `zbm --help` print its
  usage, `zbm --list` lists its commands and `zbm -V` and `zbm --version` print its version; any other option goes to
  the menu, as before.

### `fzf`

- **Before:** every image had `fzf`, which drew the legacy menu and the screens of its tools, and the build stopped on a
  host without it. `/etc/zfsbootmenu.conf` said what the build host's `fzf` could do, and whether the image had
  `column`, in six flags (`HAS_REFRESH`, `HAS_DISABLED`, `HAS_BORDER`, `HAS_COLUMN`, `HAS_COLORS`, `HAS_RAW`), which
  hooks got with the file's other variables.
- **Now:** the menu no longer uses `fzf`, nor does any tool of the recovery shell of Rust images, and Rust images have
  no `fzf`: they build on a host without it. Their `/etc/zfsbootmenu.conf`, and so the hooks' environment, has none of
  the six flags, and their build runs no `fzf` and looks for no `column` for them (`column` itself is still taken
  where the host has it). The legacy menu's files that run `fzf` go with it, with the other legacy files but the
  library and `zfsbootmenu-input`
  ([below](#rust-images-keep-only-the-library-and-zfsbootmenu-input-of-the-legacy-files)); the menu's power actions
  and Diff do what they did. `/libexec/zfunc` still runs the functions of the hook contract's library, but no longer
  those of the menu's library (`draw_be`, `draw_snapshots`, `draw_modal_prompt` and the rest), whose screens ran
  `fzf`. Legacy images keep `fzf`, those files and the flags.
- **Why:** ADR 0018, point 21: only the legacy runtime runs `fzf`, about 3.7 MB of the image.

## The recovery shell's tools

The tools of the recovery shell that Rust images take from `zbm` (ADR 0018), in place of the legacy runtime's
scripts.

### `reboot`, `poweroff`, `shutdown` and `firmware-setup` are `zbm`

- **Before:** one Bash script, `/bin/reboot`, with the other three names as links to it, which sourced three files of
  the legacy library and set the firmware's variable with `od` and `tr`.
- **Now:** links to `zbm`, which exports the writable pools of `zpool list` and resets the machine as the script did;
  `shutdown` is another name of `poweroff`. Where the machine cannot reboot into its setup, as one that boots from a
  BIOS cannot, `firmware-setup` exits 1 as before, and says why in its own words. That message, and the one when the
  machine does not reset, go to standard error, where the script printed its warnings on standard output. An argument
  is a usage error (exit status 2), and `--help` prints the usage, where the script ignored its arguments.
- **Why:** the recovery shell's tools in Rust (ADR 0018, point 11), with what the menu's power actions do.

### `zkexec` is `zbm`, and boots

- **Before:** `/bin/zkexec FILESYSTEM KERNEL INITRAMFS`, a Bash script, joined its three arguments with spaces into the
  one argument of the library's `kexec_kernel`, which splits it on tabs: the filesystem's name got all three words,
  its mount failed, and the script opened another recovery shell ("unable to mount"), then exited 1. It never booted.
  A wrong number of arguments printed the usage on standard output and exited 0.
- **Now:** a link to `zbm`, which keeps the three arguments apart and boots `/boot/KERNEL` and `/boot/INITRAMFS` of
  the filesystem as a boot chosen in the menu boots: the key of a locked encryption root is loaded first, with the
  `load-key.d` hooks, then the menu's settings, the hooks and the built-in steps, so the LUKS mappings are closed and
  the `teardown.d` context says what loaded the kernel. When the boot fails, it opens no other recovery shell: it says
  why on standard error and exits 1. A wrong number of arguments prints the usage on standard error and exits 2, and
  `--help` prints it on standard output. It searches the boot environment for kernels first, as the menu does before
  a boot, so the kernel's `.kcl` file and the `org.zfsbootmenu:commandline` of now make its command line, not one
  cached by an earlier boot. FILESYSTEM is a boot environment: a snapshot's name is refused (exit status 1), as the
  menu boots a snapshot only through a clone. As before, it exits 1 when it cannot read `/etc/zfsbootmenu.conf`.
- **Why:** the recovery shell's tools in Rust (ADR 0018, point 9); the shell it runs in is the recovery shell already.

### `mount_esp` is `zbm`, and finds the ESP itself

- **Before:** `/bin/mount_esp DEVICE`, a Bash script, ran the library's `mount_block`: it mounted the block device at
  `/mnt/NAME` with `mount DEVICE /mnt/NAME`, printed the mount point and left it mounted, or printed where the device
  was mounted already, the first line of `/proc/self/mounts` whose source was DEVICE as typed. Without DEVICE it only
  logged "device is undefined" and exited 1; the shell's completion offered the EFI system partitions not mounted yet.
  Its refusals (no block device, `/mnt/NAME` already in use, a failed mount with `mount`'s output) went only to the
  kernel log (`zerror`), with the exit status 1, and nothing to standard error. A second argument was ignored.
- **Now:** a link to `zbm`, which mounts as the script did, and without DEVICE finds the EFI system partition itself:
  the one the boot loader loaded ZFSBootMenu from (`LoaderDevicePartUUID`), or else the machine's only one, as
  `zbm.save_logs` takes it. Its refusals go to standard error as well as to the kernel log, the names in them escaped;
  a failed mount leaves `mount`'s own message on standard error and logs the device, the mount point and `mount`'s
  exit status. A device named by a link, such as `/dev/disk/by-id/...`, is found mounted by the device the link names,
  where the script compared the name as typed with the kernel's table, which names the device itself, so a second
  `mount_esp` of the same link found its own mount point in use. Two arguments or more are a usage error (exit status
  2), and `--help` prints the usage. A partition it left mounted read/write cannot be mounted read-only elsewhere, so
  the Log tab's boot history cannot read the boots saved on it until it is unmounted (`umount /mnt/NAME`); saving the
  logs on it still works.
- **Why:** the recovery shell's tools in Rust (ADR 0018, point 10).

### `logs`, `zlogtail` and `ztrace` are `zbm`, and print escaped

- **Before:** `/bin/zlogtail`, and `/bin/logs`, a link to it, removed `have_errors` and `have_warnings` and showed
  ZFSBootMenu's errors and warnings in the kernel log in `fzf`. `/bin/ztrace` showed the debug lines that shell code
  writes at `loglevel=7`, each with its calls, in `less`, with the relative time of util-linux `dmesg`, and showed
  nothing where `dmesg` was BusyBox's. Both showed the kernel's text raw.
- **Now:** links to `zbm`'s `logs`. `logs` and `zlogtail` print the errors and warnings, or the levels that
  `--level` names as `dmesg -l` takes them; an unknown level is a usage error (exit status 2). They remove
  `have_errors` when they showed the errors and `have_warnings` when they showed the warnings, so
  `logs --level info` leaves both. Where they cannot show them, as when the terminal cannot be set up or standard
  output cannot be written, they say why, exit with status 1 and remove neither. `ztrace` is `logs --trace`: it prints the debug lines in the legacy layout, those
  of the Rust runtime (`ZBM:[PID]|FILE,LINE|TEXT`) too, without the script's colours, and says that debug lines need
  `loglevel=7` when there are none. Its time is the record's, since the boot (`[   12.345678]`), where
  the script printed `dmesg --time-format reltime`'s, a clock time, then the time since the line before. They read
  `/dev/kmsg`. Where it cannot be read, `logs` runs `dmesg`, as the scripts did, and `ztrace` says why and exits
  with status 1, where the script showed nothing and exited with 0. On a terminal they show their lines in the pager
  of `zbm help`, which `/` filters and `q` leaves, where `zlogtail` showed a list in `fzf` and `ztrace` a page in
  `less`; through a pipe, or with `TERM=dumb`, they print them. Nothing to show opens no pager. Every line is escaped
  (control characters and the characters that reorder text), as the recovery shell's banner shows the errors.
- **Why:** the recovery shell's tools in Rust (ADR 0018, point 12); what the kernel log holds reaches no terminal
  raw (threat model, P13).

### `/bin/debug` is gone

- **Before:** `/bin/debug` was another name of `ztrace`.
- **Now:** Rust images have no `/bin/debug`. A link of that name to `zbm` would run `zbm debug` (`zbm debug crash`,
  `zbm debug journal`), another program; `ztrace` and its alias `trace` stay.
- **Why:** ADR 0018, point 4.

### The prompt's errors are escaped and follow a record number

- **Before:** the shell's `PROMPT_COMMAND`, `/libexec/recovery-error-printer`, printed between blank lines the errors
  logged since the time it wrote at every prompt, read with `dmesg --since` and printed raw, while `have_errors`
  existed, then removed it. Where `dmesg` had no `--since`, as BusyBox's has not, it printed every error since the
  boot.
- **Now:** Rust images run `/libexec/zbm logs --prompt`. Without `have_errors` it reads nothing; with it, it prints
  between blank lines the errors after the last record shown, each line escaped, removes `have_errors` alone and
  keeps the number of the last record of the kernel log in `errors_since`, where the script kept the time of every
  prompt. The recovery shell's banner, which shows every error since the boot as before, and `logs`, when it prints
  the errors, keep that number too, so the errors they showed are not shown again at a prompt. Two records never share a number, where several
  share a second.
- **Why:** ADR 0018, point 13; the threat model's P13.

### `zbm help` lists the commands, the keys and the options

- **Before:** the shell's `help`, an alias, opened the help pages in `fzf`: `.ansi` renderings of the documentation,
  with a page of the recovery shell's commands written by hand. There was no `/bin/help`.
- **Now:** `zbm help [TOPIC]` shows text from the tables the man page is written from:
  `commands`, each command of the recovery shell with its usage, the six functions of the shell library the shell
  offers and a line that points to `zbm --list`; `keys`, the menu's keys in the key map that `zbm.menu` sets; and
  `options`, the kernel command line's options. Without a topic it shows the commands, then names the other topics;
  an unknown topic is a usage error (exit status 2). On a terminal it shows the topic in the pager, which `q` leaves;
  through a pipe, or with `TERM=dumb`, it prints its lines as plain text, and a reader that stops early, as `head`,
  ends it with exit status 0. The shell's `help` is an alias of `zbm help`, so `help keys` shows the keys. The image
  links `/bin/help` to it too, but Bash runs its alias `help`, or else its own builtin `help`, before any program:
  the link runs only by that path.
- **Why:** ADR 0018, points 14 and 16: one source for the help, the man page and the code.

### No help pages and no help viewer in Rust images

- **Before:** images had `/libexec/zfsbootmenu-help`, the help viewer, and the documentation rendered as `.ansi`
  pages in `/usr/share/docs/help-files`, ten pages at each of three widths: the five screens of the `fzf` menu, the
  recovery shell's page and four manual pages. The shell's `help` opened the recovery shell's page, and `MOD+H` in an
  `fzf` screen the page of that screen.
- **Now:** Rust images install neither: the shell's `help` is `zbm help` (above). Legacy images keep both.
- **Why:** ADR 0018, point 17: the help comes from the code's own tables, and the `fzf` screens have gone from the
  menu.

### `zsnapshots` is the menu on its Snapshots tab

- **Before:** `/bin/zsnapshots [FILESYSTEM]`, a Bash script, showed the snapshots of any ZFS filesystem in `fzf`, in
  the snapshot screen of the `fzf` menu; without FILESYSTEM it first offered the boot environments in an `fzf` picker.
  A name that was no filesystem's, a snapshot's among them, was refused in the kernel log, which the next prompt
  showed ("'NAME' is not a ZFS filesystem"), with the exit status 1. The script ran in the shell's own process, beside a menu running elsewhere, as
  over SSH. `zfsbootmenu` ignored its arguments.
- **Now:** a link to `zbm`, which runs the menu as `zfsbootmenu --snapshots [ENVIRONMENT]`: it opens on the Snapshots
  tab, of the boot environment named, else of the one the menu selects, and from there it is the whole menu, with the
  Snapshots tab's keys (see Keys) and its other tabs. It takes boot environments only, as the menu lists their
  snapshots alone: another filesystem is refused on standard error, which names `zfs list -t snapshot -r` to list its
  snapshots, with the exit status 1; a name that is no filesystem's, a snapshot's among them, and more than one
  argument are usage errors (exit status 2). As `zfsbootmenu` does, it ends a menu running elsewhere, as over SSH, and
  the session serves it, so its changes keep the lock of a locked menu; its recovery shell ends it, back to the shell
  it ran in. The classic menu (`zbm.menu=classic`) has no Snapshots tab: there both are refused (exit status 2).
  `zfsbootmenu` refuses any other argument (exit status 2).
- **Why:** ADR 0018, point 18: the menu's Snapshots tab and its actions replace the `fzf` screen, and the menu holds
  the snapshots of boot environments only.

### Rust images keep only the library and `zfsbootmenu-input` of the legacy files

- **Before:** Rust images took every file of the legacy runtime's `lib`, `libexec` and `bin` that `zbm` did not
  replace. Besides the hook contract's library and `/libexec/zfsbootmenu-input`, they were the LUKS libraries
  (`/lib/zfsbootmenu-luks.sh`, `/lib/zfsbootmenu-luks-keys.sh`); logging functions to standard error, which nothing
  in the image sourced (`/lib/echo-log-lib.sh`); the firmware variables' functions of the legacy `firmware-setup`
  (`/lib/efi-firmware-lib.sh`); the shell's completions (`/lib/zfsbootmenu-completions.sh`), which the recovery
  shell's `.bashrc` sourced; and the `fzf` menu's library and options (`/lib/zfsbootmenu-ui.sh`,
  `/lib/fzf-defaults.sh`), its power menu (`/libexec/zpowermenu`), its differences screen
  (`/libexec/zfsbootmenu-diff`), its command-line editor (`/libexec/zfsbootmenu-edit-kcl`) and its preview
  (`/libexec/zfsbootmenu-preview`).
- **Now:** Rust images take of those files only the hook contract's library (`/lib/zfsbootmenu-core.sh`,
  `/lib/kmsg-log-lib.sh`, `/lib/zfsbootmenu-kcl.sh`, generated, with the legacy files below
  `/usr/share/zfsbootmenu/legacy`) and `/libexec/zfsbootmenu-input`, which three of the library's functions run. The
  others are gone: `zbm` unlocks and closes LUKS volumes itself, the menu replaces the `fzf` screens, the tools of the
  recovery shell are `zbm`, and no function that `/libexec/zfunc` runs used them. A hook that sources one of them,
  such as `/lib/zfsbootmenu-luks.sh` for `luks_close_all`, no longer finds it: `zluks close` closes the volumes, as
  the built-in step `80-luks-close.sh` does. A copy of the legacy hook `teardown.d/80-luks-close.sh` among an image's
  hooks takes the place of that step, as any hook of its name does, but now closes nothing: its `source` of the LUKS
  library fails and it exits 0, so the volumes stay open across `kexec`. Delete such a copy, or make it run
  `zluks close`. The recovery shell's `.bashrc` sources no completions, so Tab completes the names of commands, files and
  variables alone. Legacy images keep every file.
- **Why:** ADR 0018, points 20 and 22: Rust images take only what hooks and the library's legacy functions need, which
  SB7 removes with them.

## The host tools: `generate-zbm` and `zbm-kcl`

`generate-zbm` and `zbm-kcl` were ported from Perl and Bash step by step, quirks included. Since DM10 ([ADR
0013](docs/dev/adr/0013-host-tools.md)) they keep every key of `config.yaml`, every documented option and every hook
directory, and do what their manual pages say where Perl or Bash did something else. These are the differences.

### Values keep their text

- **Before:** `config.yaml` was read as Perl's YAML::PP read it: `Global.Version: 3.10` was the number 3.1, and
  `false` became `0` in names and messages.
- **Now:** a value is the text the file has (`3.10` stays `3.10`); a key that wants a number reads one, and one that
  wants a yes or no takes `false`, `no`, `off`, `0` and an empty value as no.
- **Why:** the manual never promised Perl's typing, and a version that loses its digits names the wrong image.

### Globs match case-sensitively

- **Before:** the globs that find kernels in the boot directory (by prefix, or by a `Kernel.Version` such as `6.1*`)
  and the hooks in their directories matched with case ignored, through File::Glob.
- **Now:** they match as the file system names files: `vmlinuz*` does not match `VMLINUZ-6.1`. Only the last part of
  a path may have `*`, `?` and `[...]`; the directory part is taken as it is.
- **Why:** two files whose names differ by case are two files.

### Versions sort as the menu sorts kernels, and all are pruned

- **Before:** image versions sorted as Perl's Sort::Versions sorts them, and a version such as `git-abc` was never
  pruned.
- **Now:** versions sort as the runtime sorts kernels, and a version such as `git-abc` is pruned with the others
  (fix 25 of the review). A name that is a word, a dash and a number alone (`foo-2`) is taken as another prefix's.
- **Why:** one order everywhere, and no image kept by accident.

### The builders run without a shell

- **Before:** dracut, mkinitcpio, `objcopy`, `objdump`, `mount` and the hooks ran through `/bin/sh -c`, and
  `Global.DracutFlags` and `Global.InitCPIOFlags` were shell text: `$HOME` or `$(...)` in them was expanded.
- **Now:** every program runs from a list of arguments. The flags are split into words as a shell splits them, quotes
  and backslashes honoured, but nothing is expanded: a flag with `$`, a backquote, a glob character or other shell
  syntax stops `generate-zbm` with the reason. A program that cannot be started gets status 127 or 126, and one a
  signal ends is described as the shell described it (`Killed`).
- **Why:** a path with a space or a quote cannot turn into another command, and a flag means what it says.

### `--enable` and `--disable` change one line

- **Before:** `--enable` and `--disable` wrote the whole of `config.yaml` again, without its comments and in another
  order.
- **Now:** they change the one value of `Global.ManageImages` in place, keep everything else as it was, and replace
  the file whole (a new file beside it, renamed over it). When that value is not a plain value on its own line, they
  refuse and say to set it by hand; a missing key or `Global` section is added.
- **Why:** comments and layout are the user's (fix 25 of the review).

### Options are spelled as the manual spells them

- **Before:** `generate-zbm` took any spelling Perl's Getopt::Long took (`--ver`, `+debug`, `-kernel`, `--noinitcpio`),
  and `POSIXLY_CORRECT` changed how it read its arguments.
- **Now:** the documented spellings, and for one release the others too, each with a warning that names the spelling
  to use; after it they are errors. `POSIXLY_CORRECT` changes nothing. `zbm-kcl` reads its options as before.
- **Why:** one parser for both tools, and options that are the same in every release.

### `generate-zbm --dry-run`

- **Before:** there was no way to see what a run would do without doing it.
- **Now:** `--dry-run` (`-n`) prints each thing a run would do (mount, run a hook, build, copy, rename, remove, change
  `Global.ManageImages`) on a line that starts with `Would`, and does none of it.
- **Why:** a configuration can be checked before it changes the boot partition.

### Images are installed whole

- **Before:** images were copied over the old ones in place, so a full disk or a power cut could leave a half-written
  image where a good one was.
- **Now:** each image is written beside its place under another name, written to the disk and renamed over the old
  one: the old image or the new one is there, never part of either.
- **Why:** the boot partition always has an image that boots.

### EFI images are built without `objcopy`

- **Before:** `generate-zbm` read the stub's alignment and sections from `objdump` and built the EFI image with
  `objcopy`, which also rewrote the stub's own headers: binutils' DOS program in front, binutils' linker version, and
  the build's time as the time stamp unless `SOURCE_DATE_EPOCH` was set.
- **Now:** `generate-zbm` builds the image itself (ADR 0014): the same sections at the same addresses, the stub's
  headers as the stub has them, and the stub's time stamp unless `SOURCE_DATE_EPOCH` sets one, so that two builds
  from the same files give the same bytes. A stub it cannot use stops the build with the reason, and a signed stub
  loses its signature with a warning, as it did before without one. The debug log names each section with its
  address; `--dry-run` lists the sections and where they come from. `objcopy` and `objdump` are no longer needed;
  `strings`, which finds kernel versions, still is.
- **Why:** no text of `objdump` to parse, no program to install, and no rewrite of the stub nothing asks for.

### Images of another prefix are named for it

- **Before:** with kernels of a prefix other than `vmlinuz`, the initramfs changed name without a word.
- **Now:** `generate-zbm` warns where images of the old name are left beside the new ones, which it neither uses nor
  prunes, and `contrib/syslinux-update.sh` finds both names (fix 23 of the review).
- **Why:** a boot loader entry that names the old file would boot an old image.

### `zbm-kcl` reads command lines as the kernel does

- **Before:** `zbm-kcl` split a command line with Bash's word splitting and `echo`: a property `-n` printed as an
  empty line, and text the kernel cannot take as an argument was kept.
- **Now:** it splits the command line as the kernel splits it, and writes each argument so that the kernel reads it
  back the same. Text that is not a kernel argument, such as a name with spaces (`"a b"`), is left out, and
  `zbm-kcl` says so on standard error. `-r` removes an argument by its name or as the whole argument; `-a` takes its
  value as arguments. A `.cmdline` section is read up to its first NUL byte, as the EFI stub reads it.
- **Why:** what `zbm-kcl` prints and saves is what the kernel will get.

### `zbm-kcl` replaces EFI images whole

- **Before:** `zbm-kcl` wrote a changed EFI image over the old one in place, the image the firmware boots.
- **Now:** it writes the image to a new file beside the destination and renames it over the old one, which keeps its
  permissions (fix 24 of the review). When that fails, it says why, and keeps the changed image in the temporary
  directory as before.
- **Why:** an interrupted write cannot leave a machine without an image that boots.

### `zbm-kcl` changes EFI images without `objcopy`

- **Before:** `zbm-kcl` read `.cmdline` with `objcopy` and `objdump`, and put the new line at the old one's address:
  a longer line than the room before the next section was refused, an image without `.cmdline` could not get one,
  and `-d` left a `.cmdline` section with a NUL byte in it.
- **Now:** `zbm-kcl` changes the section itself (ADR 0014). A line that fits takes the old one's place; a longer
  one moves the sections after it; an image without `.cmdline` gets one before its kernel; `-d` removes the section,
  as the manual says. The image keeps its time stamp; a signature it carried is dropped with a warning.
- **Why:** a command line is as long as it needs to be, on any image the stub can boot.
