# Recorded results of the legacy code

Each file here holds what the legacy Bash and Perl code returned for the cases of one Rust parity test:
`<crate>/<module>/<test>.json`, or `<crate>/<test file>/<test>.json` for an integration test. The tests read these
files instead of running the legacy code, so `cargo xtask test` needs neither Bash, Perl nor `legacy/`
(`zbm_testkit::parity`, described in `docs/dev/testing.md`).

`cargo xtask record-parity` writes them by running the legacy code that is still in the tree; the recordings of the
Perl `generate-zbm` and the Bash `zbm-kcl`, which are gone, are kept as they are. Record them as root: a few cases
compare with the legacy code only as root.

The port no longer aims at doing what the legacy code does, so these files are the expected results, not a contract.
A deliberate change of behaviour changes the test: its expected result, or the case, goes, and the pull request and
`CHANGES-FROM-LEGACY.md` say so.
