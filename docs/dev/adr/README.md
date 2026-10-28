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
| [0003](0003-crate-map.md) | The crate map: 24 crates in 11 layers, and the rules `check-deps` enforces (decision 14) | Proposed |
| [0004](0004-client-channel.md) | The menu draws natively on the console, over a client channel and a control socket (decision 15) | Accepted, 28 October 2026 |
| [0005](0005-c-dependencies.md) | C dependencies: no C library linked, programs replaced behind their ports (decision 12) | Proposed |
| [0006](0006-image-plan.md) | The image plan and its backends, and a native backend after the cutover (SB8, decision 16) | Proposed |

The record of the menu's workspace and its keys (UI13, decisions 8 and 18) comes with the workspace design.

A new record takes the next number, the same headings, and a line in this table.
