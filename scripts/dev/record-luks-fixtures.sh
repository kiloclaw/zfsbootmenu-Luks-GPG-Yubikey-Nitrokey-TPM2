#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# Records the output of cryptsetup for LUKS volumes with tokens as fixtures for
# the unit tests of zbm-luks, so that the token parsers are tested against real
# output and not only against examples written by hand.
#
# The volumes are image files on loop devices, with tokens that
# systemd-cryptenroll, cryptsetup and clevis add against a software TPM
# (swtpm), as in the LUKS unit suites; see docs/luks/testing.rst for the tools.
#
# Each fixture in crates/zbm-luks/tests/fixtures holds the exact standard
# output of one command, and commands.txt lists the command (with the volume's
# name for its device) and exit status of each.
#
# Usage: scripts/dev/record-luks-fixtures.sh   (root)

set -u

repo="$( cd "$( dirname "$0" )/../.." && pwd )"
out="${repo}/crates/zbm-luks/tests/fixtures"
work="$( mktemp -d /tmp/zbm-luks-fixtures.XXXXXX )"
tcti="swtpm:host=127.0.0.1,port=2621"
loops=()

if [ "$( id -u )" -ne 0 ]; then
  echo "record-luks-fixtures.sh needs root (loop devices)" >&2
  exit 1
fi

cleanup() {
  local loop
  for loop in "${loops[@]}"; do
    losetup -d "${loop}" 2>/dev/null
  done
  [ -f "${work}/swtpm.pid" ] && kill "$( cat "${work}/swtpm.pid" )" 2>/dev/null
  rm -rf "${work}"
}
trap cleanup EXIT

mkdir -p "${work}/tpm" "${out}"
swtpm_setup --tpm2 --tpmstate "${work}/tpm" --overwrite >/dev/null 2>&1
swtpm socket --tpm2 --tpmstate dir="${work}/tpm" \
  --server type=tcp,port=2621 --ctrl type=tcp,port=2622 \
  --flags not-need-init,startup-clear -d --pid file="${work}/swtpm.pid"
sleep 1
export SYSTEMD_TPM2_DEVICE="${tcti}" TPM2TOOLS_TCTI="${tcti}"

# arg1: name, arg2: LUKS type
# prints: the loop device of a new volume with the passphrase "pw"
volume() {
  local loop
  truncate -s 32M "${work}/${1}.img"
  printf 'pw' | cryptsetup luksFormat -q --type "${2}" --pbkdf pbkdf2 \
    --pbkdf-force-iterations 1000 --key-file=- "${work}/${1}.img"
  loop="$( losetup -f --show "${work}/${1}.img" )"
  loops+=( "${loop}" )
  echo "${loop}"
}

# arg1: device, arg2: file with the new passphrase
add_key() {
  printf 'pw' | cryptsetup luksAddKey -q --pbkdf pbkdf2 --pbkdf-force-iterations 1000 \
    --key-file=- "${1}" "${2}"
}

# A FIDO2 token as systemd-cryptenroll writes it, for a security key that is
# not there; arg1: device, arg2: key slot
fido2_token() {
  printf '%s' "fido2-secret" > "${work}/fido2-key"
  add_key "${1}" "${work}/fido2-key"
  printf '{"type":"systemd-fido2","keyslots":["%s"],"fido2-credential":"%s",%s}' "${2}" \
    "$( printf 'credential' | base64 -w0 )" \
    "\"fido2-salt\":\"$( printf 'salt' | base64 -w0 )\",\"fido2-rp\":\"io.systemd.cryptsetup\",\"fido2-clientPin-required\":false,\"fido2-up-required\":true,\"fido2-uv-required\":false" \
    > "${work}/fido2.json"
  cryptsetup token import --json-file "${work}/fido2.json" "${1}"
}

: > "${out}/commands.txt"

# arg1: fixture file, arg2: volume name for commands.txt, arg3: device,
# args: the cryptsetup command, with {} for the device
record() {
  local file="${1}" name="${2}" device="${3}" status
  shift 3
  local args=( "${@/\{\}/${device}}" )
  cryptsetup "${args[@]}" > "${out}/${file}" 2>/dev/null
  status=$?
  printf '%s\t%s\t%s\n' "${file}" "${status}" "cryptsetup ${*/\{\}/${name}}" >> "${out}/commands.txt"
}

tpm2="$( volume tpm2 luks2 )"
PASSWORD=pw systemd-cryptenroll --tpm2-device="${tcti}" --tpm2-pcrs= "${tpm2}" >/dev/null 2>&1
record luksdump-tpm2.json tpm2 "${tpm2}" luksDump --dump-json-metadata {}
record luksdump-tpm2.txt tpm2 "${tpm2}" luksDump {}
record token-tpm2.json tpm2 "${tpm2}" token export --token-id 0 {}

pin="$( volume pin luks2 )"
NEWPIN=1234 PASSWORD=pw systemd-cryptenroll --tpm2-device="${tcti}" --tpm2-pcrs=7 \
  --tpm2-with-pin=yes "${pin}" >/dev/null 2>&1
record token-tpm2-pin.json pin "${pin}" token export --token-id 0 {}

three="$( volume three luks2 )"
PASSWORD=pw systemd-cryptenroll --tpm2-device="${tcti}" --tpm2-pcrs= "${three}" >/dev/null 2>&1
fido2_token "${three}" 2
printf 'pw' | clevis luks bind -y -k - -d "${three}" tpm2 '{}' >/dev/null 2>&1
record luksdump-three.json three "${three}" luksDump --dump-json-metadata {}
record luksdump-three.txt three "${three}" luksDump {}
record token-fido2.json three "${three}" token export --token-id 1 {}
record token-clevis.json three "${three}" token export --token-id 2 {}
cryptsetup token remove --token-id 1 "${three}"
record luksdump-gap.json three "${three}" luksDump --dump-json-metadata {}
record luksdump-gap.txt three "${three}" luksDump {}

plain="$( volume plain luks2 )"
record luksdump-plain.json plain "${plain}" luksDump --dump-json-metadata {}
record luksdump-plain.txt plain "${plain}" luksDump {}

luks1="$( volume luks1 luks1 )"
record luksdump-luks1.json luks1 "${luks1}" luksDump --dump-json-metadata {}
record luksdump-luks1.txt luks1 "${luks1}" luksDump {}

cryptsetup --version > "${out}/version.txt"
echo "recorded $( wc -l < "${out}/commands.txt" ) fixtures in ${out}"
