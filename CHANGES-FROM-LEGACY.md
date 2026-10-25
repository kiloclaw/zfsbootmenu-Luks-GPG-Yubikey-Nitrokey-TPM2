# Changes from the legacy runtime

What behaves differently in images with the Rust runtime (`zfsbootmenu_runtime=rust`) than in legacy images, and why.
Images with the legacy runtime, the default, are unchanged. The list grows with each step of R5 in
[REFACTOR-ROADMAP.md](REFACTOR-ROADMAP.md); step 7 completes it as the migration guide for the switch.

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

### `fzf`

The menu no longer uses `fzf`. Rust images still install it, for the tools of the recovery shell that are not ported
yet (`logs`, `zsnapshots`, the help pages); it leaves the images when R5 and R6 port them.
