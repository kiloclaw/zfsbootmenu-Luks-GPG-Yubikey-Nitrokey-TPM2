# Shadow mode

Shadow mode checks the Rust code on machines that boot with the legacy runtime, without letting it act. The
legacy runtime still decides everything; just before each boot, `zbm shadow` works out what the Rust code would
have decided from the same inputs and reports where the two differ. The plan behind it is in
[REFACTOR-ROADMAP.md](../../REFACTOR-ROADMAP.md#shadow-mode-on-real-machines).

## Building an image with shadow mode

Set the option in the dracut or mkinitcpio configuration of a legacy runtime image:

```sh
zfsbootmenu_shadow="yes"
```

The image needs `zbm` (`zfsbootmenu_zbm`, by default `/usr/libexec/zfsbootmenu/zbm`); without it the build fails.
The option adds the hook `/libexec/hooks/boot-sel.d/99-zbm-shadow.sh`, which runs `/libexec/zbm shadow` after the
legacy runtime has chosen a boot environment and before it loads the kernel. Images of the Rust runtime
(`zfsbootmenu_runtime=rust`) ignore the option with a warning: they make these decisions themselves.

## What it compares

| Decision | Legacy runtime | Rust code |
| --- | --- | --- |
| host ID | `/etc/hostid`, as the start-up stage wrote it | the host ID chosen from `spl_hostid` |
| pools | the pools imported, read-only or not | what the start-up program imports: the preferred pool alone when `zbm.prefer=pool!!`, every pool read-only |
| LUKS mappings | the mappings in `luks-mappings` | the crypttab name, or `luks-<UUID>` for volumes discovery found |
| default boot environment | the `bootfs` the start-up stage chose | `zbm_boot::import::choose_bootfs` |
| kernel | `ZBM_SELECTED_KERNEL` and `ZBM_SELECTED_INITRAMFS` | the kernel `org.zfsbootmenu:kernel` and the version order choose |

A decision whose inputs are missing (no LUKS volume, no `bootfs`) is left out rather than counted. A kernel picked by
hand in the menu shows as a difference. Each phase of the refactoring adds the decisions it moves.

## What it may do

`zbm shadow` runs every program through `zbm_platform::ReadOnlyRunner`, which allows only `zfs list`, `zfs get`,
`zpool list`, `zpool get`, `zpool status`, `cryptsetup luksDump`, `cryptsetup status`, `cryptsetup luksUUID`,
`cryptsetup isLuks` and `blkid`, refuses commands with secret input, and refuses everything after a budget of five
seconds. It never prompts (`zbm_platform::NoConsole`), and it always exits with status 0, so it can neither stop nor
slow a boot by more than the budget.

## Where the result goes

Each difference is a line `ZBM-SHADOW: <decision>: legacy <what it did>, rust <what Rust would do>`, followed by the
summary `ZBM-SHADOW: <n> differences in <m> decisions (<names of the decisions>)`. The lines go to the console, the
summary also to the kernel log (level notice) and every comparison to the journal (`/zfsbootmenu/journal.jsonl`,
target `zbm::shadow`).

The ZFSBootMenu file system is gone after the boot, so testers on UEFI machines can add `zbm.shadow=efivar` to the
ZFSBootMenu command line: when there are differences, the lines are written once per boot to the EFI variable
`ZbmShadowSummary-5a2f3c1e-9b4d-4e7a-8c61-2d0f9b3e7a15`. On the booted system, `zbm shadow report` prints it and
removes it (as root, with `efivarfs` mounted read-write).

## In the tests

`crates/zbm-boot/src/shadow/tests.rs` covers each decision on machines where the runtimes agree and where they do
not, and that shadow mode only reads; `crates/zbm-platform/src/read_only.rs` pins the allowed commands;
`crates/zbm/tests/image.rs` checks the hook in built images. The VM harness builds every legacy image with shadow
mode (`ZBMTEST_SHADOW=0` turns it off), and a test fails when a summary on its serial consoles reports differences.
