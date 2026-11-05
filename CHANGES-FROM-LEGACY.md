# Changes from the legacy runtime

What behaves differently in images with the Rust runtime (`zfsbootmenu_runtime=rust`) than in legacy images, and why.
Images with the legacy runtime, the default, are unchanged. The list grows with each step of R5 in
[ROADMAP.md](ROADMAP.md); step 7 completes it as the migration guide for the switch.

The last section, [The host tools](#the-host-tools-generate-zbm-and-zbm-kcl), is different: it lists what changed in
`generate-zbm` and `zbm-kcl`, the programs that build images and change command lines on the host, whatever runtime
the images have.

## The shell: brush

Rust images include [brush](https://github.com/reubeno/brush), a shell compatible with Bash and written in Rust, and
the runtime uses it wherever it used Bash. GNU Bash stays in the image as dracut's `/bin/sh` and `/bin/bash`.
`zbm.shell=bash` on the command line brings Bash back for everything below.

### The recovery shell is brush

- **Before:** `bash -i`, with readline.
- **Now:** `brush -i`, which reads the same `/root/.bashrc`: the prompt, the aliases, `help`, `PROMPT_COMMAND` and
  the completions stay. On the console on the display and over SSH, its line editor is reedline, with history hints
  and syntax colours; on a serial line, which may not answer the terminal queries of reedline, it is brush's basic
  line editor. `BASH_VERSION` is still set, for scripts that check it, and `BRUSH_VERSION` names brush's version. The
  history goes to `~/.brush_history`. When brush cannot start, or ends within five seconds, the supervisor opens
  `bash -i` instead; `bash` typed in the shell starts Bash too.
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
  environment chosen in the menu is `BOOTFS` for the hooks that run after it, as the legacy menu's was. The variables
  of the `fzf` menu (`FUZZYSEL`, `PREVIEW_HEIGHT`, `FZF_DEFAULT_OPTS`) are not set: Rust images have no `fzf` menu.
  Nor are `NO_CACHE` and `CLEAR_SCREEN`, which the legacy runtime's key loading set for its own functions and its
  `load-key.d` hooks inherited: the Rust runtime passes them as arguments.
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
  environment, and exits 0 when a hook or step ran and 1 otherwise, as the script did. A boot with `zkexec` closes the
  LUKS mappings and unbinds the USB controllers, as a boot from the menu does.
- **Why:** with the scripts gone, `zkexec` would otherwise boot with the mappings open.

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
  for the legacy `zkexec`. It holds no secret and no key location's contents. Later versions may add fields, never
  change or remove one. When the file cannot be written, `ZBM_HOOK_CONTEXT` is empty and the log says why.
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
  the legacy functions left set, such as their loop variables, are not. `emergency_shell` and `is_degraded` are Rust
  code; any other function runs as the legacy `zfunc` ran it: `zbm` replaces itself with Bash, which sources the
  legacy files, kept below `/usr/share/zfsbootmenu/legacy/lib`, and runs the function with the caller's streams and
  signals. `emergency_shell` opens the recovery shell through its gate, whichever function calls it: a locked image
  asks for the PIN first. Each wrapped call is a process, so a hook that calls `import_pool` in a loop is slower; its
  arguments must be valid UTF-8 (`zbm` refuses others, with status 2); and the function does not see the functions
  the hook defined.
- **Why:** the hook contract no longer needs the legacy files, and each function can move to Rust on its own; a test
  lists those still legacy.

### `/libexec/zfunc` is `zbm`

- **Before:** a Bash script that sourced `/etc/zfsbootmenu.conf` and four files of the legacy library and called a
  function by name.
- **Now:** a link to `zbm`: `/libexec/zfunc FUNCTION [ARGS]` is `zbm zfunc`, which runs the function as the
  library's wrappers do, with the variables of `/etc/zfsbootmenu.conf` the environment does not set, as the script
  sourced the file. `/libexec/zfunc import_pool`, which the GRUB migration guide names, keeps working.
- **Why:** one way to call a function of the library, with the recovery shell behind its gate.

### `zreport` does not list the built-in steps

- **Before:** the hook lists of `zreport` in the recovery shell named the built-in hooks with the user's.
- **Now:** `zreport` still lists the hooks by their files, so in Rust images the built-in steps are not in its lists.
  The log says which steps ran.
- **Why:** `zreport` is still the legacy function, and a step has no file.

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

## Snapshots and changes (R5 step 5)

The snapshots view and the main view make the changes of the legacy menu: a new default, a snapshot, a rollback, a
clone (promoted or not), an independent copy, and the differences since a snapshot. Each imports its pool read/write
first and loads the environment's key, as before, with the same commands.

### The changes run in the menu

- **Before:** each change cleared the screen, printed what it did, and asked on the console: the name, the
  hibernation question of a read/write import, a passphrase.
- **Now:** a change on a pool imported read/write, in an environment whose key is loaded, runs inside the menu, which
  says how it went in its notification line, or in an error dialog with the reasons. Otherwise the menu hands the
  console over, as for a boot, and the hibernation question and passphrase prompts are those of before.
- **Why:** the usual change needs no question, and the menu shows the result where it shows everything else.

### New names are typed in a dialog

- **Before:** `New boot environment name` or `New snapshot name` on the console, filled with `<environment>_NEW` or
  the time (`%Y-%m-%d-%H%M%S`); characters outside `[a-zA-Z0-9-_.:]` were removed after Enter, with a message.
- **Now:** a dialog with the same name filled in, which only takes those characters, and refuses a name the menu knows
  already. The time of a snapshot's name is in UTC, as the image has no time zone.
- **Why:** the name is part of the action, and what can be typed is what can be used.

### Rollback has no safety snapshot

- **Before:** typing `ROLLBACK` confirmed the rollback, which destroys the newer snapshots.
- **Now:** typing the environment's name confirms it, and the confirmation counts the snapshots destroyed and their
  size. The menu's design had a safety snapshot taken first; ZFS destroys it with the other newer snapshots, so there
  is none, and the confirmation says to clone the snapshot instead to keep things as they are.
- **Why:** the name says which environment loses its changes.

### A pool that may not be written

- **Before:** `org.zfsbootmenu:readonly` or a degraded pool showed `Operation prohibited` for ten seconds.
- **Now:** the change fails with an error dialog that says why: the property's value, or that the pool is degraded.
- **Why:** the reason stays on the screen until it is read.

### Clones leave encryption properties out

- **Before:** a clone was given every property set on its environment, `keylocation` and `keyformat` included, which
  ZFS refuses on a clone when the environment is its own encryption root.
- **Now:** the clone gets the other properties; it shares the key of its origin's encryption root, as every clone does.
- **Why:** cloning an environment that is its own encryption root failed.

### A new default needs no key

- **Before:** setting the default loaded the key of the pool's root dataset first, which could ask for a passphrase.
- **Now:** only the pool is imported read/write; `zpool set bootfs` needs no key.
- **Why:** the passphrase was asked for nothing.

## Start-up (R5 step 4)

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

## The menu (R5 step 3)

The `fzf` menu is replaced by the new menu, designed in [docs/dev/menu-design](docs/dev/menu-design/README.md). It
runs as `/bin/zfsbootmenu` and `zbm`, as the old one did, so it also opens over SSH and from the recovery shell, and
takes over from a menu already running.

### The countdown shows the menu

- **Before:** one line, `Booting <environment> in <n> seconds`, with Enter to boot at once and Escape for the menu.
- **Now:** the menu itself, with the environment to boot selected and `Booting <environment> in <n> s. Press any key
  to choose another.` above the list. Any key stops the countdown; Enter boots the selected environment, which is the
  one the countdown would have booted, and Escape only stops the countdown. `zbm.timeout`, `zbm.show` and `zbm.skip`
  mean what they meant.
- **Why:** what is about to boot, and what else there is, can be seen without a key press.

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
  `No Boot Environments` and how to get to the pools (`p`) and the recovery tools (`!`), and waits.
- **Why:** an environment that is there but cannot boot is a problem to see, not to hide, and the menu is a better
  place to recover from than a shell that opens by itself.

### A key that cannot be loaded at the countdown

- **Before:** the recovery shell, with `unable to load key for <environment>`.
- **Now:** the menu says `unable to load key for <environment>` and why; another environment can be chosen, or the
  recovery shell opened from the recovery tools.
- **Why:** the same as above; the shell is one key away.

### The recovery shell

- **Before:** Ctrl-R in the menu.
- **Now:** Ctrl-R and Alt-R still open it, as the old `MOD+key` bindings stay as aliases; the menu's own way is `!`,
  the recovery tools, of which the shell is the first. As before, leaving the shell imports the pools that became
  importable in it and opens the menu again.

### An edited command line is for one boot

- **Before:** the edited line was saved in `/zfsbootmenu/cmdline` and used for every later boot of any environment
  until Ctrl-T cleared it.
- **Now:** the edited line boots once, in place of the environment's `org.zfsbootmenu:commandline`: `%{parent}` in it
  stands for the parent's line, `root=` is ZFSBootMenu's, and `noresume` and the host ID are added when they apply,
  as for the environment's own line. Nothing is saved, so there is nothing to clear.
- **Why:** the menu says the edit is for this boot only, and a line saved after a failed boot should not decide the
  next one.

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

- **Before:** `zbm` was the menu's other name.
- **Now:** it still is, and `zbm <command>` runs that command of the Rust program (`zbm debug crash`, `zbm report`),
  as the messages of the runtime say.

### `fzf`

The menu no longer uses `fzf`. Rust images still install it, for the tools of the recovery shell that are not ported
yet (`logs`, `zsnapshots`, the help pages); it leaves the images when R5 and R6 port them.

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
