# ADR 0003: The crate map

- **Status:** accepted, 29 October 2026 (proposed 28 October 2026)
- **Decides:** decision 14 of [ROADMAP.md](../../../ROADMAP.md#decisions)
- **Governs:** DM2, DM7, DM8, DM10, DM11, SB10, SE6; `xtask/src/deps.rs` and
  [architecture.md](../architecture.md)

## Context

The workspace has 25 crates ([architecture.md](../architecture.md) lists them by layer, generated from
`xtask/src/deps.rs`). DM1 removed `zbm-menu` and `zbm-coreutils` and moved the PSF parser. What is left to settle:

- `zbm-core` is the emulation of Bash and Perl (`getopts`, shell quoting, Perl's file tests) that the parity work
  needed; parity is no longer a goal (decision 17).
- `zbm-boot` holds boot environments, kernels, command lines and key sources as well as loading and booting, and it
  depends on `zbm-luks` and `zbm-pool` in its own layer.
- `generate-zbm` and `zbm-kcl` are two host tools in two crates, sharing the Perl and Bash emulation.
- The domain takes `Runner` and the `System` bundle, which carries a console, so domain code can run any program and
  draw on the screen.
- There is no crate for the keyholder of [ADR 0002](0002-trust-zones.md).

## Decision

**Twenty-four crates in eleven layers; dependencies point down only, with two new rules.**

| Layer | Crates | Responsibility |
| --- | --- | --- |
| unsafe | `zbm-sys` | The system calls `rustix` lacks (`signalfd` and the signal mask, `memfd_secret`, seccomp, landlock, `kexec_file_load`, the virtual terminal's ioctls); the rest moves onto `rustix` (SE6) |
| types | `zbm-types` | Domain values with no I/O: names, `KernelSelector`, `KernelCommandLine`, `HostId`, `Timeout`, `LogLevel`, questions and answers |
| foundation | `zbm-trace` | Structured events, the journal, the kernel log, crash reports |
| platform | `zbm-platform`, `zbm-display` | The ports and their real implementations, one `Context` without a console, the reactor's parts ([ADR 0001](0001-process-model.md)); the display, fonts and keyboards |
| state | `zbm-config`, `zbm-state` | The option table and typed options; the state directory with its locks |
| adapters | `zbm-zfs`, `zbm-crypt`, `zbm-tools` | One port trait per capability (`Zfs`, `LuksVolumes`, `Tpm2`, `Fido2`, `KernelLoader`, `Mounts`, `Modules`, `Devices`), with in-memory fakes behind a `fake` feature |
| domain | `zbm-luks`, `zbm-pool`, `zbm-bootenv`, `zbm-boot` | Unlocking, with the registry of key methods; pool import as a state machine; boot environments, kernels, command lines and key sources (`zbm-bootenv`, split from `zbm-boot`); loading and booting with teardown |
| application | `zbm-app` | The `Application` trait, `App`, the `Action`, `Event` and `Snapshot` vocabulary, and the messages between zones |
| orchestration | `zbm-startup` | Start-up as a pipeline of steps over `zbm-app` |
| front ends | `zbm`, `zbm-tui`, `zbm-console`, `zbm-brush`, `zbm-host` | The multi-call runtime binary; the menu; the console; the shell; the host tools, `generate-zbm` and `zbm-kcl` as two binaries of one crate |
| tools | `xtask`, `zbm-vmtest`, `zbm-testkit` | Developer automation, the VM harness, the test kit |

Gone: `zbm-core` (DM11: its useful parts move into `zbm-platform` and `zbm-config`, the emulation is deleted),
`generate-zbm` and `zbm-kcl` (into `zbm-host`, DM10). New: `zbm-bootenv` (DM7) and `zbm-host`.

The rules `cargo xtask check-deps` enforces, the existing ones and two new:

1. A crate depends only on crates of lower layers, with exceptions listed by name in `deps.rs`. The exceptions of
   today go: `zbm-trace` → `zbm-core` with `zbm-core`, `zbm-boot` → `zbm-luks` with DM2; `zbm-boot` → `zbm-pool`
   becomes `zbm-boot` → `zbm-bootenv` → `zbm-pool` through ports; `zbm` → `zbm-tui` and `zbm` → `zbm-console` stay,
   as the binary that holds the front ends.
2. **New: orchestration does not use adapters.** `zbm-startup` reaches the system through `zbm-app` and the domain's
   services, never `zbm-zfs`, `zbm-crypt` or `zbm-tools` directly.
3. **New: front ends reach the system only through `zbm-app`.** `zbm-tui` and `zbm-console` depend on no domain or
   adapter crate.
4. Domain crates never take `Runner`; they reach programs through adapter ports.
5. Every third-party crate is allowed per crate, in `deps.rs`, with its reason in
   [dependencies.md](../dependencies.md).

The keyholder of Z1 is a process, not a crate: an applet of `zbm` (`zbm unlock serve`) built from `zbm-luks` and the
adapters it needs, so the image keeps one Rust binary. A separate binary follows only if SE11's sandbox profiles need
the smaller attack surface of a binary without the menu.

## Consequences

- One place to look for each kind of code, and a dependency graph a reviewer can read from the table.
- The domain can be tested with fakes only: no program runs and nothing draws in a domain test.
- DM2 and DM8 are large: every service that takes `&System` or a `Runner` changes its signature, and the tests with it.
- `zbm-host` merges two crates' tests and golden files; the host tools' command lines and outputs do not change.
- Some code moves twice (`zbm-core`'s parts to `zbm-platform`, then into the ports that need them); each move is its
  own commit, so history stays readable.

## Alternatives considered

- **Fewer, larger crates** (one for the domain, one for the adapters). Fewer manifests, but the layer rules would
  have to be checked inside a crate, where `check-deps` cannot see them.
- **More crates** (one per adapter or per key method). Faster incremental builds, but every port change would touch
  several manifests, and the table would no longer fit in a reader's head.
- **Keeping `zbm-core`.** Its emulation exists for parity, which decision 17 dropped; keeping it keeps every caller of
  it on the Bash model.
- **A separate binary for the keyholder from the start.** Cleaner isolation, but a second Rust binary in the image,
  with its own copy of the standard library and of the crates it shares with `zbm`, before the sandbox that would
  justify it exists.

## How it is checked

- `cargo xtask check-deps` with the table above in `deps.rs`, and `--bless` regenerating
  [architecture.md](../architecture.md), in CI.
- The metric columns (`cargo xtask metrics`): ZFS command lines outside `zbm-zfs` and string parameters in domain
  crates at zero by the end of phase 1.

## Where the workspace stands

On 29 October 2026, when the map was accepted:

- Done: `zbm-core` is gone (DM11), and so is the `zbm-boot` → `zbm-luks` exception (DM2); `zbm-sys` is on `rustix`
  (SE6); no domain crate takes a `Runner`. `zbm-tui` and `zbm-console` already depend on no domain or adapter crate
  (rule 3), and `zbm-app` uses `zbm-sys` directly for the session's socket (PM10), a lower layer.
- Left: `zbm-bootenv` (DM7), so `zbm-boot` → `zbm-pool` stays in `SAME_LAYER` until then; `zbm-host` (DM10), so
  `generate-zbm` and `zbm-kcl` stay two crates until then; and `zbm-startup` still uses `zbm-zfs` and `zbm-tools`
  directly, so rule 2 joins `check-deps` once it no longer does, with start-up's steps over `zbm-app`. Rule 3 is
  enforced since DM8 (29 October 2026): `zbm-tui` and `zbm-console` may use no crate of the adapters, the domain or
  orchestration but in tests (`ONLY_THROUGH_APP` in `deps.rs`).

## Answers of the maintainer

Accepted as proposed, on 29 October 2026:

1. **The keyholder:** an applet of `zbm` (`zbm unlock serve`), so the image keeps one Rust binary; a binary of its own
   only if SE11's sandbox profiles need it.
2. **The host tools' crate:** `zbm-host`.
3. **`zbm-bootenv`:** a crate of its own, so that `zbm-boot` (loading and booting) cannot reach pool import except
   through ports.
