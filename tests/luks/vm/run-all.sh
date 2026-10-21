#!/bin/bash
# Runs the LUKS VM tests in order, one virtual machine at a time, and prints a
# summary of the results. See docs/luks/testing.rst.
#
# Usage: run-all.sh [test ...]
#   Without arguments, every test runs. Tests are named as in the summary,
#   e.g. layouts-main or complex-hotplug; setups run automatically before the
#   scenarios that need them.
#
# Environment: see zbmtest.py (ZBMTEST_WORK, ZBMTEST_IMAGE, ZBMTEST_ACCEL ...).
# Set ZBMTEST_OLD_KERNEL and ZBMTEST_OLD_INITRD to an image with an older ZFS to
# run complex-oldzfs.

here="$( cd "$( dirname "$0" )" && pwd )"
work="${ZBMTEST_WORK:-/var/tmp/zbm-luks-test}"
mkdir -p "${work}"

ALL=(
  layouts-main layouts-parity layouts-parity-oldzfs
  complex-setup complex-healthy complex-crypttab complex-degraded complex-hotplug
  complex-hotplug-auto complex-badkey complex-skip-recover complex-oldzfs
  keys-setup keys-plugin keys-native keys-lockout keys-notpm
  firmware-setup firmware-boot firmware-usbkey firmware-usblate firmware-nostick
  procedures
  runtime-setup runtime-legacy runtime-rust
)

tests=( "$@" )
[ "${#tests[@]}" -gt 0 ] || tests=( "${ALL[@]}" )

# Build the boot environment and the ZFSBootMenu images once
python3 -c "import sys; sys.path.insert(0, '${here}'); import zbmtest; zbmtest.build_be(); zbmtest.payload_disk()" \
  || exit 1

rc=0
for test in "${tests[@]}"; do
  script="${test%%-*}.py"
  args=()
  [ "${script}" = "procedures.py" ] || args=( "${test#*-}" )
  echo "=== ${test} ($( date +%T ))"
  ( cd "${here}" && python3 "${script}" "${args[@]}" ) > "${work}/run-${test}.out" 2>&1
  r=$?
  tail -n 1 "${work}/run-${test}.out"
  [ "${r}" -eq 0 ] || rc=1
done

echo
echo "Summary (${work}/tests/*/result.json):"
python3 - "${work}" "${tests[@]}" <<'EOF'
import json, sys
from pathlib import Path
work = Path(sys.argv[1])
total_p = total_f = 0
for name in sys.argv[2:]:
    p = work / "tests" / name / "result.json"
    if not p.exists():
        print(f"  {name:22} no result")
        continue
    r = json.loads(p.read_text())
    total_p += r["passed"]; total_f += r["failed"]
    print(f"  {name:22} {'PASS' if not r['failed'] else 'FAIL'}  {r['passed']:3} passed  {r['failed']:2} failed  {r['seconds']:5} s")
print(f"  {'total':22}       {total_p:3} passed  {total_f:2} failed")
EOF
exit "${rc}"
