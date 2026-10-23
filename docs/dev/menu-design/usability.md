# Usability sessions

The design is checked with people who use ZFSBootMenu, on the prototype (`cargo run -p zbm-tui -- --demo`), before R5
builds it, and again on the R5 build. Phase D needs at least five sessions, one of them on a serial console.

## Who

Five or more people who boot with ZFSBootMenu today, covering the [users](users-and-tasks.md#users): at least one who
uses LUKS or native encryption with a key device, one administrator of a server, and one who knows the `fzf` menu
well. Recruited from the issue tracker and the discussion channels; no one who worked on the design.

## Setup

- The prototype on the participant's own machine or over a screen share: `cargo run -p zbm-tui -- --demo`, at the
  terminal size they use. For the serial console session, the demo runs in a VM's serial console at 80×24 with
  `TERM=vt100` (`cargo run -p zbm-tui -- --demo` inside `zbm-vmtest`'s guest, or `screen /dev/ttyS0 115200`).
- The demo machine: two pools (`zroot`, `tank`), four boot environments, three kernels each, 400 snapshots on one,
  three LUKS volumes (passphrase `demo`, a TPM2 PIN `1234`), a foreign pool, and a pool that is `DEGRADED`.
- Sessions take 30 to 45 minutes, recorded with consent (the terminal only).

## Script

The facilitator reads each task, then stays silent unless the participant is stuck for two minutes. The tasks follow
the [scenarios](users-and-tasks.md#scenarios), with the demo's names:

1. Let the machine boot as it would every morning. *(T1)*
2. Boot `tank/ROOT/arch` instead. *(T2)*
3. Boot the default environment with its older kernel 6.8.0. *(T3)*
4. Yesterday's upgrade broke something: boot the state of `zroot/ROOT/void` from before it, to check it; then make it
   the current state. *(T4)*
5. Start again with the volumes locked: unlock them; the security key is not plugged in. *(T5, demo option
   `--locked`)*
6. Boot once with `systemd.unit=rescue.target`. *(T6)*
7. Start again with a foreign pool: get it imported and boot from it. *(T7, demo option `--foreign`)*
8. Find the snapshot taken before `2026-09` among the 400 of `zroot/ROOT/void`.

After the tasks: what was hard, what was surprising, what they missed from the `fzf` menu, and whether they would
use the classic bindings ([decision 8](keymap.md#the-classic-bindings)).

## What to record

For each task: completed or not, time, key presses (the demo logs them with `--log-keys <file>`), wrong actions
started, dialogs dismissed without reading, whether help (`?`) was opened, and what the participant said. Findings go
into `docs/dev/menu-design/findings.md`, each with the task, what happened, its severity (blocks the task, slows it,
cosmetic) and what changes in the design, or why nothing does.
