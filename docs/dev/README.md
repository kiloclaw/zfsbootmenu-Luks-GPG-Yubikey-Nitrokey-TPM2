# Developer documentation

Pages for people who change the code of this repository. Users' documentation is the Sphinx site in `docs/` and the
manual pages in `docs/man/`.

| Page | What it covers |
| --- | --- |
| [architecture.md](architecture.md) | The crates, the layers they sit in, and the dependencies allowed between them |
| [testing.md](testing.md) | Every test suite, how to run it, and how new tests are written |
| [tooling.md](tooling.md) | `cargo xtask`: lints, tests, dependency checks and code metrics |

The plans behind the code are in the repository root: [PORT-ROADMAP.md](../../PORT-ROADMAP.md) ports ZFSBootMenu to
Rust, and [REFACTOR-ROADMAP.md](../../REFACTOR-ROADMAP.md) restructures the result.
[CONTRIBUTING.md](../../CONTRIBUTING.md) has the rules for pull requests, and [crates/DESIGN.md](../../crates/DESIGN.md)
the design of the Rust runtime.
