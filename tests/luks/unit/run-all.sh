#!/bin/bash
# Runs every LUKS unit suite and prints a summary; exits non-zero on failure.
#
# Usage: run-all.sh [ZFSBootMenu source tree]
#
# Output of each suite goes to ${ZBMTEST_UNIT_WORK:-/var/tmp/zbm-luks-unit}.
# The suites need root; see docs/luks/testing.rst. Run one at a time: runs share
# the work directory and the ports of their software TPMs.

here="$( cd "$( dirname "$0" )" && pwd )"
repo="${1:-$( cd "${here}/../../.." && pwd )}"
out="${ZBMTEST_UNIT_WORK:-/var/tmp/zbm-luks-unit}"
mkdir -p "${out}"

{
  echo "tools:"
  echo "  $( cryptsetup --version 2>&1 | head -n1 )"
  echo "  $( systemd-cryptenroll --version 2>&1 | head -n1 )"
  echo "  $( gpg --version 2>&1 | head -n1 )"
  echo "  $( tpm2_getcap --version 2>&1 | head -n1 )"
  echo "  clevis $( dpkg-query -W -f '${Version}' clevis 2>/dev/null || echo '?' )"
  echo "  $( swtpm --version 2>&1 | head -n1 )"
} | tee "${out}/tools.txt"

rc=0
for suite in unlock tokens edge config; do
  "${here}/${suite}.sh" "${repo}" > "${out}/${suite}.out" 2>&1
  r=$?
  if [ "${r}" -eq 77 ]; then
    printf '%-8s skipped (%s)\n' "${suite}" "$( tail -n1 "${out}/${suite}.out" )"
    continue
  fi
  printf '%-8s %s\n' "${suite}" "$( tail -n1 "${out}/${suite}.out" )"
  grep -a -h "^  FAIL" "${out}/${suite}.out"
  [ "${r}" -eq 0 ] || rc=1
done
exit "${rc}"
