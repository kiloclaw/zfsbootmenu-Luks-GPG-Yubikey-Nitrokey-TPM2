# Architecture decision records

Each record states one decision of [ROADMAP.md](../../../ROADMAP.md#decisions): the problem, what is decided, what
follows from it, what else was weighed, how the decision is checked, and the questions left for the maintainer. A
record is **proposed** until the maintainer accepts it; it is then **accepted**, with the date, and the decision in
ROADMAP.md moves from "Open" to "Decided". A later record that changes a decision **supersedes** the earlier one, which
stays, marked so.

| Record | Decision | Status |
| --- | --- | --- |
| [0001](0001-process-model.md) | The process model and concurrency: one reactor per process, no async runtime (decision 10, PM1) | Accepted, 28 October 2026 |
| [0002](0002-trust-zones.md) | Trust zones: the supervisor, the keyholder, the session, the unprivileged front ends (decision 13) | Accepted, 28 October 2026 |
| [0003](0003-crate-map.md) | The crate map: 24 crates in 11 layers, and the rules `check-deps` enforces (decision 14) | Accepted, 29 October 2026 |
| [0004](0004-client-channel.md) | The menu draws natively on the console, over a client channel and a control socket (decision 15) | Accepted, 28 October 2026 |
| [0005](0005-c-dependencies.md) | C dependencies: no C library linked, programs replaced behind their ports (decision 12) | Accepted, 29 October 2026 |
| [0006](0006-image-plan.md) | The image plan and its backends, and a native backend after the cutover (SB8, decision 16) | Accepted, 29 October 2026 |
| [0007](0007-menu-workspace.md) | The menu as a workspace of tabs and columns, and keys that work on every keyboard (UI13, decision 18; proposes decision 8's key map) | Accepted, 29 October 2026 |
| [0008](0008-measured-boot.md) | Measured boot: PCRs 8 and 9, a CEL-JSON log, a separator after the last unseal, `zbm measure` (#10, SE10) | Accepted, 30 October 2026 |
| [0009](0009-hook-root.md) | The hook root on machines that unlock themselves: measured into PCR 9 before its hooks run, and the value tokens bind to (SE10, R1) | Accepted, 31 October 2026 |
| [0010](0010-locked-menu.md) | The locked menu and the one gate to a shell: what the lock covers, where the PIN is checked, who else is let in (#8, SE9, R8) | Proposed |

A new record takes the next number, the same headings, and a line in this table.
