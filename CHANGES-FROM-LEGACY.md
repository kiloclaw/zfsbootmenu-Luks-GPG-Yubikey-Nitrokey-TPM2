# Changes from the legacy runtime

What behaves differently in images with the Rust runtime (`zfsbootmenu_runtime=rust`) than in legacy images, and why.
Images with the legacy runtime, the default, are unchanged. The list grows with each step of R5 in
[REFACTOR-ROADMAP.md](REFACTOR-ROADMAP.md); step 7 completes it as the migration guide for the switch.

## Start-up (R5 step 4)

Start-up runs on the menu's start-up view when the image has the menu and the terminal can run it (`TERM` set, and not
`dumb`); otherwise it keeps the console of the legacy program, prompts included. `zluks` in the recovery shell and
over SSH is unchanged.

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
  the volume's passphrase or moves on from a PIN, as before; Escape does the same; Ctrl-R shows what is typed. While a key device or security key is awaited, the dialog shows the
  seconds left: Enter looks again at once, Escape stops waiting.
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
