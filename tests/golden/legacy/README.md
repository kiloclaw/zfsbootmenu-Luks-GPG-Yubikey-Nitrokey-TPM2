# Recorded results of the legacy code

Each file here holds what the legacy Bash and Perl code returned for the cases of one Rust parity test:
`<crate>/<module>/<test>.json`, or `<crate>/<test file>/<test>.json` for an integration test. The tests read these
files instead of running the legacy code, so `cargo xtask test` needs neither Bash, Perl nor `legacy/`
(`zbm_testkit::parity`, described in `docs/dev/testing.md`).

`cargo xtask record-parity` writes them by running the legacy code, and `cargo xtask record-parity --check` checks that
they still match it; CI's coverage job runs the tests that way. Record them as root, as CI checks them: a few cases
compare with the legacy code only as root. Never edit them by hand. A deliberate change of behaviour records the file
again and is listed in the pull request.
