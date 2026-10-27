#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# Lists the commits that changed the frozen legacy runtime since the freeze at
# the start of Phase 4 of the port plan (docs/dev/history/port-roadmap.md),
# with the files each one changed.
# Every one of them must be listed in legacy/RUNTIME-CHANGES.md and ported to the Rust
# runtime before the beta ends (rule 4 of the port plan).
#
# Usage: scripts/dev/legacy-runtime-changes.sh [revision]   (default: HEAD)

set -euo pipefail

# The last commit before the freeze
FREEZE=661e9f8894219826981e24e53739a32662bb258f

# The legacy runtime, and the dracut and mkinitcpio files that install it
FROZEN=(legacy/zfsbootmenu packaging/dracut packaging/initcpio)

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  sed -n '4,9s/^# \{0,1\}//p' "$0"
  exit 0
fi

cd "$(git rev-parse --show-toplevel)"

if ! git cat-file -e "${FREEZE}^{commit}" 2>/dev/null; then
  echo "the freeze commit ${FREEZE} is not in this clone; fetch more history" >&2
  exit 1
fi

git log --reverse --date=short --format='%h %ad %s' --name-only \
  "${FREEZE}..${1:-HEAD}" -- "${FROZEN[@]}"
