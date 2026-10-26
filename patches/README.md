# Patched crates

Crates the workspace builds with a change of ours, until their authors release it. Each is the crate as crates.io
publishes it, with the change applied, and `[patch.crates-io]` in the root `Cargo.toml` points at it. `cargo vendor`
leaves this directory alone: it owns `vendor/` only ([docs/dev/dependencies.md](../docs/dev/dependencies.md)).

| Crate | Change | Offered upstream |
| --- | --- | --- |
| `brush-parser` 0.4.0 | [brush-parser-case-in-substitution.patch](brush-parser-case-in-substitution.patch): in a command substitution, the `)` that ends a `case` pattern (`a\|b)`) no longer closes the substitution, as in bash. Without it, brush cannot read `zfsbootmenu-core.sh`, which the recovery shell sources, nor any script with such a `case` inside `$( )`. The tokenizer tracks the `case` commands of the substitution, and the word parser asks the tokenizer where the substitution ends. With tests; brush's 2,067 compatibility cases give the same results with it | To be offered to [reubeno/brush](https://github.com/reubeno/brush) |

To refresh a patched crate for a new release: unpack the release's `.crate` file here, apply the patch (`patch -p1`
in the crate's directory), run its tests (`cargo test` there), and drop the patch once a release has the change.
