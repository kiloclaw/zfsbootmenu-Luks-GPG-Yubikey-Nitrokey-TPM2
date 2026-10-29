# Usability sessions

The design is checked with people who use ZFSBootMenu, on the prototype, before UI10 builds the workspace, and again on
the UI10 build (UI11 in ROADMAP.md). They decide [decision 8](keymap.md): the workspace's key map against the
current one.

## Who

Five or more people who boot with ZFSBootMenu today, covering the [users](users-and-tasks.md#users): at least one who
uses LUKS or native encryption with a key device, one administrator of a server who works on a **serial console**,
one who types on a **non-US keyboard layout** (German, French, or one that is not Latin), and one who knows the `fzf`
menu well. Recruited from the issue tracker and the discussion channels; no one who worked on the design.

## Setup

- The prototype on the participant's own machine or over a screen share, in both maps, at the terminal size they use:

  ```sh
  cargo run -p zbm-tui -- --demo --workspace --log-keys workspace.log   # the workspace's map
  cargo run -p zbm-tui -- --demo --log-keys current.log                 # the current map
  ```

  Half the participants start with the workspace and half with the current map, and each does the script in both, so
  that neither map gains from being second.
- **The serial console session**: the demo at 80×24 in ASCII, as over IPMI (`--theme ascii --size 80x24`, inside
  `zbm-vmtest`'s guest or `screen /dev/ttyS0 115200`), in both maps.
- **The non-US layout session**: on the display, as at boot (`--drm --font …`, as root on a virtual console), with the
  participant's layout loaded on the console. Keys by position (rule 3 of [the key map](keymap.md)) come with UI15:
  until then, note which keys the participant reaches for and which do nothing.
- The demo machine: two pools (`zroot`, `tank` degraded), four boot environments, three kernels each, 400 snapshots on
  one, three LUKS volumes (passphrase `demo`, a TPM2 PIN `1234`); `--foreign` adds a pool of another host, `--locked`
  locks the volumes.
- Sessions take 45 to 60 minutes with both maps, recorded with consent (the terminal only).

## Script

The facilitator reads each task, then stays silent unless the participant is stuck for two minutes. The tasks follow
the [scenarios](users-and-tasks.md#scenarios), with the demo's names:

1. Let the machine boot as it would every morning. *(T1)*
2. Something on this machine needs attention: say what, and where you would look at it. *(the sidebar and the
   attention line; in the current map, the navigation bar's status)*
3. Boot `tank/ROOT/arch` instead. *(T2)*
4. Boot the default environment with its older kernel 6.12.8_1. *(T3)*
5. Yesterday's upgrade broke something: boot the state of `zroot/ROOT/void` from before it, to check it; then make it
   the current state. *(T4)*
6. Start again with the volumes locked: unlock them; the security key is not plugged in. *(T5, `--locked`)*
7. Boot once with `systemd.unit=rescue.target`. *(T6)*
8. Start again with a foreign pool: get it imported and boot from it. *(T7, `--foreign`)*
9. Find the snapshot taken before `2026-09` among the 400 of `zroot/ROOT/void`.
10. Look at the pools, then at the log, then go back to the boot environments. *(the tabs; in the current map, `p`,
    `l` and `Esc`)*

After the tasks: what was hard, what was surprising, which map they would keep and why, whether they used `j` `k` `h`
`l` or the arrows, the digits or `Tab`, what they missed from the `fzf` menu, and whether they would use the classic
bindings ([decision 8](keymap.md#the-classic-bindings)).

## What to record

For each task and map: completed or not, time, key presses (`--log-keys <file>` logs them with their times), wrong
actions started, keys that did nothing (a key the participant expected to work), dialogs dismissed without reading,
whether help (`?`) was opened, and what the participant said. Findings go into
[findings.md](findings.md), each with the task, the map, what happened, its severity (blocks the task, slows it,
cosmetic) and what changes in the design, or why nothing does.
