# Developer documentation

Pages for people who change the code of this repository. Users' documentation is the Sphinx site in `docs/` and the
manual pages in `docs/man/`.

| Page | What it covers |
| --- | --- |
| [architecture.md](architecture.md) | The crates, the layers they sit in, and the dependencies allowed between them |
| [testing.md](testing.md) | Every test suite, how to run it, and how new tests are written |
| [tooling.md](tooling.md) | `cargo xtask`: lints, tests, dependency checks and code metrics |
| [dependencies.md](dependencies.md) | Every third-party crate and why it is used |
| [adr/](adr/) | The architecture decision records of the target architecture: what is decided, why, and how it is checked |
| [history/](history/) | The four plans that [ROADMAP.md](../../ROADMAP.md) replaced, kept as the record of how the code got here |
| [threat-model.md](threat-model.md) | Attackers, assets, untrusted input, what must hold and what shows it, and the fuzz targets |

The plan behind the code is [ROADMAP.md](../../ROADMAP.md) in the repository root: the target architecture, the rules
for the code, and the work left, in order.
[CONTRIBUTING.md](../../CONTRIBUTING.md) has the rules for pull requests, and [crates/DESIGN.md](../../crates/DESIGN.md)
the design of the Rust runtime.
