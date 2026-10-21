#!/bin/sh
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# Checks the line widths that the Rust Style Guide sets and rustfmt does not
# enforce: no line is longer than 100 characters, and a line that is only a
# comment is no longer than 80 characters without its indentation.
#
# Usage: check-rust-width.sh [directory ...]   (default: crates)
#
# Prints each line that is too long and exits with 1 if there are any.

[ "$#" -gt 0 ] || set -- crates

find "$@" -name '*.rs' -type f | LC_ALL=C sort | xargs awk '
  {
    text = $0
    sub(/^ +/, "", text)
    if (length($0) > 100) {
      printf "%s:%d: %d characters (at most 100)\n", FILENAME, FNR, length($0)
      bad = 1
    } else if (text ~ /^\/\// && length(text) > 80) {
      printf "%s:%d: comment of %d characters without indentation (at most 80)\n", FILENAME, FNR, length(text)
      bad = 1
    }
  }
  END { exit bad }
' || exit 1
