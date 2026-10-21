#!/bin/bash
# shellcheck disable=SC2034,SC2155  # test harness: variables are read by the sourced libraries; ls of the private /dev/mapper is intended
# Unit suite: key management in zfsbootmenu-luks-keys.sh, with real LUKS2
# headers, a software TPM, systemd-cryptenroll, clevis and GnuPG, and stand-ins
# for FIDO2 keys and YubiKeys.
# Usage: tokens.sh [ZFSBootMenu source tree]   (root; see docs/luks/testing.rst)
# shellcheck source=isolate.sh
source "$(dirname "$0")/isolate.sh"
# shellcheck source=impl.sh
source "$(dirname "$0")/impl.sh"; zbm_impl_check
REPO="${1:-$(cd "$(dirname "$0")/../../.." && pwd)}"
S="$(cd "$(dirname "$0")" && pwd)"
K=/tmp/zk; rm -rf "$K"; mkdir -p "$K"   # short path: gpg-agent sockets
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  ok   - $*"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL - $*"; }
check(){ local m="$1"; shift; if "$@"; then ok "$m"; else bad "$m"; fi; }

# --- software TPM
mkdir -p "$K/tpm"; swtpm_setup --tpm2 --tpmstate "$K/tpm" --overwrite >/dev/null 2>&1
[ -f /tmp/zk-swtpm.pid ] && kill "$(cat /tmp/zk-swtpm.pid)" 2>/dev/null; sleep 0.5
swtpm socket --tpm2 --tpmstate dir="$K/tpm" --server type=tcp,port=2421 --ctrl type=tcp,port=2422 \
  --flags not-need-init,startup-clear -d --pid file=/tmp/zk-swtpm.pid; sleep 1
TCTI="swtpm:host=127.0.0.1,port=2421"
export SYSTEMD_TPM2_DEVICE="$TCTI" TPM2TOOLS_TCTI="$TCTI"

mkluks() { truncate -s 32M "$K/$1.img"; printf 'pw' | cryptsetup luksFormat -q --type luks2 --pbkdf pbkdf2 \
  --pbkdf-force-iterations 1000 --label "$1" --key-file=- "$K/$1.img"; losetup -f --show "$K/$1.img"; }
addkey() { printf 'pw' | cryptsetup luksAddKey -q --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --key-file=- "$1" "$2"; }

T0=$(mkluks tpm); PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= "$T0" >/dev/null 2>&1
T1=$(mkluks tpmpin); NEWPIN=1234 PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= --tpm2-with-pin=yes "$T1" >/dev/null 2>&1
T2=$(mkluks tpmpcr); PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs=16 "$T2" >/dev/null 2>&1
CL=$(mkluks clevis); printf 'pw' | clevis luks bind -y -k - -d "$CL" tpm2 '{}' >/dev/null 2>&1
# GnuPG: symmetric key file, and a key file encrypted to a (software) secret key
export GNUPGHOME="$K/gen"; mkdir -m 700 "$GNUPGHOME"
head -c 64 /dev/urandom > "$K/k1"; head -c 64 /dev/urandom > "$K/k2"
GS=$(mkluks gpgsym); addkey "$GS" "$K/k1"
GP=$(mkluks gpgpub); addkey "$GP" "$K/k2"
GP2=$(mkluks gpgpub2); addkey "$GP2" "$K/k2"
gpg -q --batch --pinentry-mode loopback --passphrase gpgpass --symmetric -o "$K/k1.gpg" "$K/k1"
gpg -q --batch --pinentry-mode loopback --passphrase keypass --quick-gen-key 'ZBM <zbm@test>' ed25519 sign never
FPR=$(gpg --list-keys --with-colons | awk -F: '/^fpr/{print $10; exit}')
gpg -q --batch --pinentry-mode loopback --passphrase keypass --quick-add-key "$FPR" cv25519 encr never
gpg -q --batch --trust-model always -r "$FPR" --encrypt -o "$K/k2.gpg" "$K/k2"
mkdir -m 700 "$K/imgpub" "$K/imgsec"
gpg --batch --export "$FPR" | GNUPGHOME="$K/imgpub" gpg -q --batch --import
cp -a "$GNUPGHOME/." "$K/imgsec/"; gpgconf --kill all; unset GNUPGHOME
# YubiKey challenge-response: LUKS passphrase = HMAC-SHA1(challenge)
YK=$(mkluks yk); printf '%s' "$(printf 'ykpass' | openssl dgst -sha1 -hmac yk-secret | awk '{print $NF}')" > "$K/ykresp"; addkey "$YK" "$K/ykresp"
YK2=$(mkluks yk2); addkey "$YK2" "$K/ykresp"
YH=$(mkluks ykhash); CH=$(printf 'ykpass' | sha256sum | cut -c1-64); printf '%s%s' "$CH" "$(printf '%s' "$CH" | openssl dgst -sha1 -hmac yk-secret | awk '{print $NF}')" > "$K/ykresp2"; addkey "$YH" "$K/ykresp2"
# FIDO2: systemd-fido2 token whose keyslot passphrase is the hmac-secret (base64)
export FIDO_CRED=$(head -c 64 /dev/urandom | base64 -w0) FIDO_SALT=$(head -c 32 /dev/urandom | base64 -w0) FIDO_HMAC=$(head -c 32 /dev/urandom | base64 -w0)
F2=$(mkluks fido); printf '%s' "$FIDO_HMAC" > "$K/fidopass"; addkey "$F2" "$K/fidopass"
printf '{"type":"systemd-fido2","keyslots":["1"],"fido2-credential":"%s","fido2-salt":"%s","fido2-rp":"io.systemd.cryptsetup","fido2-clientPin-required":false,"fido2-up-required":true,"fido2-uv-required":false}' \
  "$FIDO_CRED" "$FIDO_SALT" > "$K/fido.json"; cryptsetup token import --json-file "$K/fido.json" "$F2"
[ -e /dev/hidraw99 ] || mknod /dev/hidraw99 c 1 3

cleanup() { rm -f /dev/mapper/* 2>/dev/null; for d in $T0 $T1 $T2 $CL $GS $GP $GP2 $YK $YK2 $YH $F2; do losetup -d "$d"; done
  rm -f /dev/hidraw99; kill "$(cat /tmp/zk-swtpm.pid)" 2>/dev/null; }
trap cleanup EXIT
echo "devices: tpm=$T0 tpmpin=$T1 tpmpcr=$T2 clevis=$CL gpgsym=$GS gpgpub=$GP yk=$YK ykhash=$YH fido=$F2"

run() {
  export FAKE; FAKE="$(mktemp -d)"; mkdir -p "$FAKE"/{holders,maps,content,pools,busy}
  rm -f /dev/mapper/* 2>/dev/null
  echo "== $1"
  (
    export PATH="$S/kstubs:$S/stubs:$PATH" BASE="$FAKE/base" LUKS_TPM2_TCTI="$TCTI"
    mkdir -p "$BASE/cmdline.d"; echo 0 > "$BASE/cmdline.d/zbm.luks_token_wait"
    zinfo()  { echo "INFO: $*"  >> "$FAKE/log"; }
    zwarn()  { echo "WARN: $*"  >> "$FAKE/log"; }
    zerror() { echo "ERROR: $*" >> "$FAKE/log"; }
    zdebug() { [ $# -gt 0 ] && echo "DEBUG: $*" >> "$FAKE/log"; return 0; }
    colorize() { shift; echo -ne "$@"; }
    timed_prompt() { echo "timed_prompt $*" >> "$FAKE/prompts"; return 1; }
    zbm_impl_load "$REPO" "$FAKE/crypttab" || exit 1
    luks_is_open() { [ -e "$FAKE/holders/$(basename "$(readlink -f "$1")")" ]; }
    "$2"
  ) > "$FAKE/out" 2>&1
  RC=$?
  if [ -n "$DUMP" ] && [[ "$1" == *"$DUMP"* ]]; then cat "$FAKE/out"; echo "--- calls"; cat "$FAKE/calls"; echo "--- log"; cat "$FAKE/log"; fi
}
mapped() { [ -b "/dev/mapper/$1" ]; }
prompts() { grep -c "$1" "$FAKE/out"; }
tab() { printf '%s\n' "$@" > "$ZBM_CRYPTTAB"; }

t() { ZBM_CRYPTTAB="$FAKE/crypttab"; tab "v $T0 none"; luks_unlock_all; }
run "TPM2 token without PIN (plugin present but no TPM device -> native)" t < /dev/null
check "mapped with no prompt" mapped v
check "no passphrase prompt" [ "$(prompts 'Passphrase (leave')" -eq 0 ]
check "unlocked via TPM2" grep -q "unlocked $T0 with TPM2" "$FAKE/log"

t() { LUKS_NO_TOKEN_PLUGINS=1; tab "v $T0 none"; luks_unlock_all; }
run "TPM2 token without PIN, plugins disabled" t < /dev/null
check "mapped" mapped v

t() { LUKS_NO_TOKEN_PLUGINS=1; tab "v $T1 none"; luks_unlock_all; }
run "TPM2 PIN token: wrong PIN, then right PIN" t <<< $'0000\n1234'
check "mapped" mapped v
check "asked for TPM2 PIN twice" [ "$(prompts 'TPM2 PIN')" -eq 2 ]
check "wrong-PIN message" grep -q "did not release the key" "$FAKE/out"
check "no passphrase prompt" [ "$(prompts 'Passphrase (leave')" -eq 0 ]

t() { LUKS_NO_TOKEN_PLUGINS=1; tab "v $T1 none"; luks_unlock_all; }
run "TPM2 PIN token: empty PIN falls back to passphrase" t <<< $'\npw'
check "mapped via passphrase" mapped v
check "passphrase prompt shown" [ "$(prompts 'Passphrase (leave')" -eq 1 ]

t() { LUKS_NO_TOKEN_PLUGINS=1; tab "a $T1 none" "b $T0 none"; luks_unlock_all; }
run "TPM2 PIN and no-PIN volumes together" t <<< $'1234'
check "a mapped" mapped a
check "b mapped" mapped b

# The authorization value derived from the PIN reaches tpm2_unseal on standard
# input, never in its arguments, which any process can read in /proc
UNSEAL="$(command -v tpm2_unseal)"
t() { LUKS_NO_TOKEN_PLUGINS=1; mkdir -p "$FAKE/argv"
  printf '#!/bin/sh\necho "$*" >> "%s/unseal-args"\nexec "%s" "$@"\n' "$FAKE" "$UNSEAL" > "$FAKE/argv/tpm2_unseal"
  chmod +x "$FAKE/argv/tpm2_unseal"; PATH="$FAKE/argv:$PATH"; tab "v $T1 none"; luks_unlock_all; }
run "TPM2 PIN token: the PIN's authorization value stays out of arguments" t <<< $'1234'
check "mapped" mapped v
check "tpm2_unseal ran" [ -s "$FAKE/unseal-args" ]
check "authorization read from standard input" grep -q '+file:-' "$FAKE/unseal-args"
check "no authorization value in the arguments" bash -c "! grep -q 'hex:' '$FAKE/unseal-args'"

t() { LUKS_NO_TOKEN_PLUGINS=1; tpm2_pcrextend 16:sha256=0000000000000000000000000000000000000000000000000000000000000001; tab "v $T2 none"; luks_unlock_all; }
run "TPM2 token bound to PCR 16 after PCR 16 changed -> passphrase" t <<< $'pw'
check "mapped via passphrase" mapped v
check "passphrase prompt shown" [ "$(prompts 'Passphrase (leave')" -eq 1 ]

t() { LUKS_NO_TOKEN_PLUGINS=1; tab "v $T1 none"; luks_unlock_all; }
run "TPM2 PIN token: repeated wrong PINs hit the TPM lockout" t <<< $'000\n111\n222\npw'
check "lockout explained" grep -q "locked against PIN guessing" "$FAKE/out"
check "fell back to passphrase" mapped v
check "no PIN prompt after lockout" [ "$(prompts 'TPM2 PIN')" -le 3 ]
t() { LUKS_NO_TOKEN_PLUGINS=1; tab "v $T1 none"; luks_unlock_all; }
run "TPM2 PIN token while TPM is in lockout: no PIN prompt at all" t <<< $'pw'
check "no PIN prompt" [ "$(prompts 'TPM2 PIN')" -eq 0 ]
check "lockout explained" grep -q "locked against PIN guessing" "$FAKE/out"
tpm2_dictionarylockout -c >/dev/null 2>&1

t() { tab "v $CL none"; luks_unlock_all; }
run "clevis TPM2 token" t < /dev/null
check "mapped with no prompt" mapped v
check "unlocked via clevis" grep -q "unlocked $CL with clevis" "$FAKE/log"

t() { tab "v $CL none no-tokens"; luks_unlock_all; }
run "no-tokens option skips tokens" t <<< $'pw'
check "mapped via passphrase" mapped v
check "clevis not used" bash -c "! grep -q 'with clevis' $FAKE/log"

t() { echo 0 > "$BASE/cmdline.d/zbm.luks_tokens"; tab "v $T0 none"; luks_unlock_all; }
run "zbm.luks_tokens=0 disables tokens" t <<< $'pw'
check "passphrase prompt shown" [ "$(prompts 'Passphrase (leave')" -eq 1 ]

t() { LUKS_GNUPG_DIR="$K/imgpub"; tab "v $GS $K/k1.gpg"; luks_unlock_all; }
run "GnuPG symmetric key file: wrong, then right passphrase" t <<< $'nope\ngpgpass'
check "mapped" mapped v
check "key file passphrase asked twice" [ "$(prompts 'Key file passphrase')" -eq 2 ]
check "decrypted key not left behind" bash -c "! ls /run/zbm-key.* /run/zbm-gpg.* 2>/dev/null | grep -q ."

t() { LUKS_GNUPG_DIR="$K/imgsec"; tab "v $GP $K/k2.gpg"; luks_unlock_all; }
run "GnuPG key file encrypted to a secret key" t <<< $'keypass'
check "mapped" mapped v
check "secret key passphrase prompt" [ "$(prompts 'Secret key passphrase')" -eq 1 ]

t() { LUKS_GNUPG_DIR="$K/imgpub"; tab "v $GP $K/k2.gpg"; luks_unlock_all; }
run "GnuPG key file, secret key/card absent -> passphrase, no PIN prompt" t <<< $'pw'
check "mapped via passphrase" mapped v
check "never asked for a PIN" bash -c "! grep -qE 'Card PIN|Secret key passphrase' $FAKE/out"
check "logged missing key" grep -q "no secret key for" "$FAKE/log"

t() { LUKS_GNUPG_DIR="$K/imgpub"; cp "$K/k1.gpg" "$K/k1.bin"; tab "v $GS $K/k1.bin gpg"; luks_unlock_all; }
run "gpg option for key file without .gpg suffix" t <<< $'gpgpass'
check "mapped" mapped v

t() { tab "v $YK none ykchalresp"; luks_unlock_all; }
run "YubiKey challenge-response (slot 2)" t <<< $'ykpass'
check "mapped" mapped v
check "challenge sent to slot 2 via stdin" grep -q "ykchalresp -2 -i-" "$FAKE/calls"

t() { tab "v $YH none ykchalresp=2,ykchalresp-hash,ykchalresp-concat"; luks_unlock_all; }
run "YubiKey challenge-response, hash + concat" t <<< $'ykpass'
check "mapped" mapped v

t() { tab "v $YK none ykchalresp"; luks_unlock_all; }
run "YubiKey wrong passphrase then right" t <<< $'bad\nykpass'
check "mapped" mapped v

t() { LUKS_NO_TOKEN_PLUGINS=1; tab "v $F2 none"; luks_unlock_all; }
run "FIDO2 token via native fido2-assert" t < /dev/null
check "mapped" mapped v
check "touch instruction shown" grep -q "Touch the security key" "$FAKE/out"
check "fido2-assert used hmac-secret, up=true, no PIN" grep -q "fido2-assert -G -h -t up=true -t uv=false /dev/hidraw99" "$FAKE/calls"

t() { LUKS_NO_TOKEN_PLUGINS=1; export FIDO_NEED_PIN=1; sed 's/"fido2-clientPin-required":false/"fido2-clientPin-required":true/' "$K/fido.json" > "$FAKE/f.json"
      cryptsetup token remove --token-id 0 "$F2"; cryptsetup token import --json-file "$FAKE/f.json" "$F2"; tab "v $F2 none"; luks_unlock_all
      cryptsetup token remove --token-id 0 "$F2"; cryptsetup token import --json-file "$K/fido.json" "$F2"; }
run "FIDO2 token requiring PIN passes pin=true" t < /dev/null
check "mapped" mapped v
check "pin=true requested" grep -q "pin=true" "$FAKE/calls"

t() { LUKS_NO_TOKEN_PLUGINS=1; mv /dev/hidraw99 /dev/xhidraw99; tab "v $F2 none"; luks_unlock_all; mv /dev/xhidraw99 /dev/hidraw99; }
run "FIDO2 key absent -> passphrase" t <<< $'pw'
check "mapped via passphrase" mapped v
check "logged missing key" grep -q "no FIDO2 security key" "$FAKE/log"

t() { LUKS_GNUPG_DIR="$K/imgsec"; tab "a $GP $K/k2.gpg" "b $GP2 $K/k2.gpg"; luks_unlock_all; }
run "two volumes sharing one GnuPG key file: one prompt" t <<< $'keypass'
check "a mapped" mapped a
check "b mapped" mapped b
check "secret key passphrase asked once" [ "$(prompts 'Secret key passphrase')" -eq 1 ]
check "decrypted key wiped afterwards" bash -c "! ls /run/zbm-key.* /tmp/zbm-key.* 2>/dev/null | grep -q ."

t() { tab "a $YK none ykchalresp" "b $YK2 none ykchalresp"; luks_unlock_all; }
run "two volumes on one YubiKey slot: one challenge" t <<< $'ykpass'
check "a mapped" mapped a
check "b mapped" mapped b
check "challenge asked once" [ "$(prompts 'YubiKey challenge-response')" -eq 1 ]

echo
echo "passed: $PASS failed: $FAIL"
[ "$FAIL" -eq 0 ]
