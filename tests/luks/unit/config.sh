#!/bin/bash
# shellcheck disable=SC2010,SC2034,SC2155  # test harness: variables are read by the sourced libraries; ls of the private /dev/mapper is intended
# Unit suite: crypttab and command-line parsing, device specifications,
# passphrases and USB key sticks.
# Usage: config.sh [ZFSBootMenu source tree]   (root; see docs/luks/testing.rst)
# shellcheck source=isolate.sh
source "$(dirname "$0")/isolate.sh"
# shellcheck source=impl.sh
source "$(dirname "$0")/impl.sh"; zbm_impl_check
REPO="${1:-$(cd "$(dirname "$0")/../../.." && pwd)}"; S="$(cd "$(dirname "$0")" && pwd)"
K=/tmp/zf; rm -rf "$K"; mkdir -p "$K"
PASS=0; FAIL=0
ok(){ PASS=$((PASS+1)); echo "  ok   - $*"; }; bad(){ FAIL=$((FAIL+1)); echo "  FAIL - $*"; }
check(){ local m="$1"; shift; if "$@"; then ok "$m"; else bad "$m"; fi; }

mkdir -p "$K/tpm"; swtpm_setup --tpm2 --tpmstate "$K/tpm" --overwrite >/dev/null 2>&1
[ -f /tmp/zf-swtpm.pid ] && kill "$(cat /tmp/zf-swtpm.pid)" 2>/dev/null; sleep 0.5
swtpm socket --tpm2 --tpmstate dir="$K/tpm" --server type=tcp,port=2731 --ctrl type=tcp,port=2732 --flags not-need-init,startup-clear -d --pid file=/tmp/zf-swtpm.pid; sleep 1
TCTI="swtpm:host=127.0.0.1,port=2731"; export SYSTEMD_TPM2_DEVICE="$TCTI" TPM2TOOLS_TCTI="$TCTI"

mkluks(){ truncate -s 32M "$K/$1.img"; printf 'pw' | cryptsetup luksFormat -q --type "${2:-luks2}" --pbkdf pbkdf2 --pbkdf-force-iterations 1000 ${3:+--label "$1"} --key-file=- "$K/$1.img"; losetup -f --show "$K/$1.img"; }
addkey(){ printf 'pw' | cryptsetup luksAddKey -q --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --key-file=- "$1" "$2"; }

STALE=$(mkluks stale luks2 L); PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= "$STALE" >/dev/null 2>&1; cryptsetup luksKillSlot -q "$STALE" 1 <<< pw 2>/dev/null || printf 'pw' | cryptsetup luksKillSlot -q --key-file=- "$STALE" 1
MULTI=$(mkluks multi luks2 L); PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= "$MULTI" >/dev/null 2>&1
export FIDO_CRED=$(head -c 64 /dev/urandom | base64 -w0) FIDO_SALT=$(head -c 32 /dev/urandom | base64 -w0) FIDO_HMAC=$(head -c 32 /dev/urandom | base64 -w0)
printf '%s' "$FIDO_HMAC" > "$K/fp"; addkey "$MULTI" "$K/fp"
printf '{"type":"systemd-fido2","keyslots":["2"],"fido2-credential":"%s","fido2-salt":"%s","fido2-rp":"io.systemd.cryptsetup","fido2-clientPin-required":false,"fido2-up-required":true,"fido2-uv-required":false}' "$FIDO_CRED" "$FIDO_SALT" > "$K/f.json"; cryptsetup token import --json-file "$K/f.json" "$MULTI"
FIDOONLY=$(mkluks fidoonly luks2 L); addkey "$FIDOONLY" "$K/fp"; sed 's/"keyslots":\["2"\]/"keyslots":["1"]/' "$K/f.json" > "$K/f1.json"; cryptsetup token import --json-file "$K/f1.json" "$FIDOONLY"
TOK=$(mkluks tok luks2 L); PASSWORD=pw systemd-cryptenroll --tpm2-device="$TCTI" --tpm2-pcrs= "$TOK" >/dev/null 2>&1
L1=$(mkluks one luks1)
PLAIN=$(mkluks plain luks2 L)
truncate -s 32M "$K/hdrdata.img"; truncate -s 16M "$K/hdr"; HDRDATA=$(losetup -f --show "$K/hdrdata.img"); printf 'pw' | cryptsetup luksFormat -q --type luks2 --header "$K/hdr" --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --key-file=- "$HDRDATA"
head -c 64 /dev/urandom > "$K/good.key"; GK=$(mkluks gk luks2 L); addkey "$GK" "$K/good.key"; head -c 64 /dev/urandom > "$K/wrong.key"
export GNUPGHOME="$K/gen"; mkdir -m 700 "$GNUPGHOME"
for n in a b; do gpg -q --batch --pinentry-mode loopback --passphrase "pass$n" --quick-gen-key "K$n <$n@t>" ed25519 sign never; done
FA=$(gpg --list-keys --with-colons a@t | awk -F: '/^fpr/{print $10; exit}'); FB=$(gpg --list-keys --with-colons b@t | awk -F: '/^fpr/{print $10; exit}')
gpg -q --batch --pinentry-mode loopback --passphrase passa --quick-add-key "$FA" cv25519 encr never
gpg -q --batch --pinentry-mode loopback --passphrase passb --quick-add-key "$FB" cv25519 encr never
gpg -q --batch --trust-model always -r "$FA" -r "$FB" --encrypt -o "$K/good.key.gpg" "$K/good.key"
mkdir -m 700 "$K/onlyb"; gpg --batch --pinentry-mode loopback --passphrase passb --export-secret-keys "$FB" | GNUPGHOME="$K/onlyb" gpg -q --batch --pinentry-mode loopback --passphrase passb --import; gpg --batch --export "$FA" | GNUPGHOME="$K/onlyb" gpg -q --batch --import
gpg -q --batch --pinentry-mode loopback --passphrase symp --symmetric -o "$K/good.key.sym.gpg" "$K/good.key"
gpgconf --kill all; GNUPGHOME="$K/onlyb" gpgconf --kill all; unset GNUPGHOME
printf '#!/bin/sh\nprintf pw\n' > "$K/ks"; chmod +x "$K/ks"
[ -e /dev/hidraw99 ] || mknod /dev/hidraw99 c 1 3
cleanup(){ rm -f /dev/mapper/* 2>/dev/null; detach_own_loops "$K"; rm -f /dev/hidraw99; kill "$(cat /tmp/zf-swtpm.pid)" 2>/dev/null; }
trap cleanup EXIT

run(){ export FAKE; FAKE="$(mktemp -d)"; mkdir -p "$FAKE"/{holders,maps,content,pools,busy}; rm -f /dev/mapper/* 2>/dev/null
  for d in $STALE $MULTI $FIDOONLY $TOK $L1 $PLAIN $HDRDATA $GK; do echo zfs_member > "$FAKE/content/${d##*/}"; done
  echo "== $1"
  ( export PATH="$S/kstubs:$S/stubs:$PATH" BASE="$FAKE/base" LUKS_TPM2_TCTI="$TCTI" ANSWERS="$FAKE/answers"
    export ZBMTEST_PROMPT_DELAY=1
    mkdir -p "$BASE/cmdline.d"; echo 0 > "$BASE/cmdline.d/zbm.luks_token_wait"
    # Logging, timed prompts and open volumes go through the seams of impl.sh
    zbm_impl_load "$REPO" "$FAKE/crypttab" || exit 1
    "$2" ) > "$FAKE/out" 2>&1; RC=$?
  N=$((N+1)); mkdir -p "$K/cases"; { echo "### $1 (rc=$RC)"; cat "$FAKE/out"; echo "--- calls"; cat "$FAKE/calls" 2>/dev/null; echo "--- log"; cat "$FAKE/log" 2>/dev/null; echo "--- prompts"; cat "$FAKE/prompts" 2>/dev/null; echo "--- mapper"; ls /dev/mapper; } > "$K/cases/$N.txt" 2>&1; }
mapped(){ [ -b "/dev/mapper/$1" ]; }; prompts(){ grep -c "$1" "$FAKE/out"; }; tab(){ printf '%s\n' "$@" > "$ZBM_CRYPTTAB"; }
noleft(){ ! ls /run/zbm-key.* /run/zbm-gpg.* /run/zbm-tpm2.* /tmp/zbm-key.* /tmp/zbm-tpm2.* 2>/dev/null | grep -q .; }

# ---------------------------------------------------------------------------
# Final edge cases
mkpw(){ truncate -s 32M "$K/$1.img"; printf '%s' "$2" | cryptsetup luksFormat -q --type luks2 --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --key-file=- "$K/$1.img"; losetup -f --show "$K/$1.img"; }
SPEC=$(mkpw spec 'with spaces  and "quotes" \back$HOME -x ünï 密码 ')
LEAD=$(mkpw lead '-starts-with-dash')
LONGPW=$(head -c 300 /dev/urandom | base64 -w0 | head -c 400); LONG=$(mkpw long "$LONGPW")
# partitioned image: LUKS inside partition 1 (PARTUUID tests)
truncate -s 64M "$K/part.img"; printf 'label: gpt\nsize=40MiB\n' | sfdisk -q "$K/part.img"; PARTLOOP=$(losetup -P -f --show "$K/part.img"); udevadm settle 2>/dev/null; sleep 1
PART="${PARTLOOP}p1"; printf 'pw' | cryptsetup luksFormat -q --type luks2 --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --key-file=- "$PART"
PARTUUID=$(blkid -c /dev/null -s PARTUUID -o value "$PART")
NOTLUKS=$(truncate -s 8M "$K/nl.img"; losetup -f --show "$K/nl.img")
# key file with junk around it (keyfile-offset / keyfile-size), and an empty key file
{ head -c 100 /dev/urandom; cat "$K/good.key"; head -c 50 /dev/urandom; } > "$K/padded.key"; : > "$K/empty.key"; mkdir -p "$K/dir.key"
# key stick image: vfat with the key, attached late
# (ext4: this sandbox kernel has no FAT driver; the VM test uses a real FAT stick)
truncate -s 16M "$K/stick.img"; mkfs.ext4 -q -L FINALKEY "$K/stick.img"; mkdir -p "$K/m"; mount -o loop "$K/stick.img" "$K/m" && mkdir -p "$K/m/keys" && cp "$K/good.key" "$K/m/keys/gk.key"; umount "$K/m"
for d in $SPEC $LEAD $LONG $PART $NOTLUKS; do :; done
UGK=$(cryptsetup luksUUID $GK)

run2(){ run "$@"; }   # same harness, content files must include the new devices
_run_orig="$(declare -f run)"; eval "${_run_orig/for d in \$STALE/for d in \$SPEC \$LEAD \$LONG \$PART \$NOTLUKS \$STALE}"

echo "### configuration parsing"
t(){ printf 'v %s none' "$GK" > "$ZBM_CRYPTTAB"; luks_unlock_all; }
run "crypttab whose last line has no trailing newline" t <<< $'pw'
check "mapped" mapped v

t(){ printf 'v %s none discard\r\nw %s none\r\n' "$GK" "$L1" > "$ZBM_CRYPTTAB"; luks_unlock_all; }
run "crypttab with CRLF line endings" t <<< $'pw\npw'
check "v mapped" mapped v; check "w mapped" mapped w; check "discard still recognised" grep -q -- "--allow-discards" "$FAKE/calls"

t(){ printf '\n# comment\n   \n\tv\t\t%s   none\t discard  \n# trailing comment\n' "$GK" > "$ZBM_CRYPTTAB"; luks_unlock_all; }
run "crypttab with tabs, blank lines, indentation and comments" t <<< $'pw'
check "mapped" mapped v

t(){ tab "v UUID=${UGK^^} none"; luks_unlock_all; }
run "uppercase UUID= in crypttab" t <<< $'pw'
check "mapped" mapped v

t(){ echo "${UGK^^}" > "$BASE/cmdline.d/zbm.luks"; rm -f "$ZBM_CRYPTTAB"; luks_unlock_all; }
run "uppercase bare UUID in zbm.luks" t <<< $'pw'
check "mapped as luks-<lowercase UUID>" mapped "luks-$UGK"

if [ -n "$PARTUUID" ]; then
t(){ tab "v PARTUUID=${PARTUUID^^} none"; luks_unlock_all; }
run "uppercase PARTUUID= (as printed by sgdisk)" t <<< $'pw'
check "mapped" mapped v

t(){ tab "v PARTUUID=$PARTUUID none"; luks_unlock_all; }
run "PARTUUID= of a LUKS partition" t <<< $'pw'
check "mapped" mapped v
else echo "  skip - PARTUUID cases: loop partitions are not available here"; fi

t(){ echo "luks-$UGK" > "$BASE/cmdline.d/zbm.luks"; rm -f "$ZBM_CRYPTTAB"; luks_unlock_all; }
run "zbm.luks=luks-<UUID>" t <<< $'pw'
check "mapped" mapped "luks-$UGK"

t(){ echo "UUID=$UGK,$GK,luks-$UGK" > "$BASE/cmdline.d/zbm.luks"; rm -f "$ZBM_CRYPTTAB"; luks_unlock_all; }
run "same device listed three times in zbm.luks" t <<< $'pw'
check "one mapping" [ "$(ls /dev/mapper | grep -c luks-)" -eq 1 ]; check "one prompt" [ "$(prompts 'Unlock LUKS volume')" -eq 1 ]

t(){ tab "a $GK none" "b UUID=$UGK none"; luks_unlock_all; }
run "same device under two names in crypttab" t <<< $'pw'
check "a mapped" mapped a; check "b not mapped" bash -c "! [ -b /dev/mapper/b ]"; check "one prompt" [ "$(prompts 'Unlock LUKS volume')" -eq 1 ]

t(){ mknod /dev/mapper/taken b 7 250; tab "taken $GK none" "w $L1 none"; luks_unlock_all; }
run "mapping name already used by another device; next volume still unlocked" t <<< $'pw\npw'
check "error reported" grep -q "mapping taken already exists" "$FAKE/log"; check "w mapped" mapped w; rm -f /dev/mapper/taken

t(){ tab "x FOO=bar none" "w $L1 none"; luks_unlock_all; }
run "unknown device specification; next volume still unlocked" t <<< $'pw'
check "error reported" grep -q "unrecognized LUKS device specification" "$FAKE/log"; check "w mapped" mapped w

t(){ tab "x $NOTLUKS none nofail" "w $L1 none"; luks_unlock_all; }
run "non-LUKS device in crypttab is skipped" t <<< $'pw'
check "warned" grep -q "is not a LUKS volume" "$FAKE/log"; check "w mapped" mapped w; check "no prompt for the non-LUKS device" bash -c "! grep -q 'Unlock LUKS volume $NOTLUKS' $FAKE/out"

echo "### invalid and edge-value options"
t(){ tab "v $GK none key-slot=abc"; luks_unlock_all; }
run "non-numeric key-slot= is ignored, passphrase still works" t <<< $'pw'
check "mapped" mapped v; check "warned" grep -q "invalid LUKS option 'key-slot=abc'" "$FAKE/log"

t(){ tab "v $GK $K/good.key keyfile-offset=x,keyfile-size=-5"; luks_unlock_all; }
run "invalid keyfile-offset/keyfile-size are ignored" t <<< $'pw'
check "mapped" mapped v; check "warned" grep -q "invalid LUKS option 'keyfile-offset=x'" "$FAKE/log"

t(){ tab "v $GK $K/padded.key keyfile-offset=100,keyfile-size=64"; luks_unlock_all; }
run "keyfile-offset + keyfile-size select the key inside a larger file" t < /dev/null
check "mapped without prompt" mapped v

t(){ tab "v $GK $K/empty.key"; luks_unlock_all; }
run "empty key file -> passphrase fallback" t <<< $'pw'
check "mapped" mapped v

t(){ tab "v $GK $K/dir.key"; luks_unlock_all; }
run "key file path is a directory -> passphrase fallback" t <<< $'pw'
check "mapped" mapped v

t(){ tab "v $GK none tries=abc"; luks_unlock_all; }
run "invalid tries= keeps the default" t <<< $'x\nx\npw'
check "mapped on third try" mapped v

t(){ tab "v $GK none tries=1"; luks_unlock_all; }
run "tries=1: one wrong passphrase, then retry offered" t <<< $'x'
check "retry/skip offered" bash -c "grep -q timed_prompt $FAKE/prompts"

echo "### passphrases"
t(){ tab "v $SPEC none"; luks_unlock_all; }
run "passphrase with spaces, quotes, backslash, \$, unicode and trailing space" t <<< 'with spaces  and "quotes" \back$HOME -x ünï 密码 '
check "mapped" mapped v

t(){ tab "v $LEAD none"; luks_unlock_all; }
run "passphrase starting with a dash" t <<< '-starts-with-dash'
check "mapped" mapped v

t(){ tab "v $LONG none"; luks_unlock_all; }
run "400-character passphrase" t <<< "$LONGPW"
check "mapped" mapped v

t(){ tab "v $GK none" "w $LEAD none"; luks_unlock_all; }
run "different passphrases: second volume prompts after cached one fails" t <<< $'pw\n-starts-with-dash'
check "v mapped" mapped v; check "w mapped" mapped w

echo "### key devices (USB sticks) and boot-time races"
t(){ STICK=""; ( sleep 3; losetup -f "$K/stick.img"; udevadm settle 2>/dev/null ) & echo 5 > "$BASE/cmdline.d/zbm.luks_token_wait"; tab "v $GK /keys/gk.key:LABEL=FINALKEY"; luks_unlock_all; wait; }
run "key stick plugged in 3 s after unlocking starts" t < <(sleep 20)
check "mapped from the stick, no passphrase prompt" mapped v; check "no passphrase prompt" bash -c "! grep -q 'Passphrase' $FAKE/out"
check "stick unmounted again" bash -c "! grep -q luks-key /proc/mounts"
for i in 1 2 3 4 5; do l=$(losetup -j "$K/stick.img" | cut -d: -f1); [ -z "$l" ] && break; echo "$l" | xargs -r -n1 losetup -d 2>/dev/null; sleep 1; done
check "stick detached before the next case" [ -z "$(losetup -j "$K/stick.img")" ]

t(){ echo 2 > "$BASE/cmdline.d/zbm.luks_token_wait"; tab "v $GK /keys/gk.key:LABEL=NOSUCHSTICK"; luks_unlock_all; }
run "key stick absent -> short wait, then passphrase" t < <(sleep 4; echo pw)
check "mapped via passphrase" mapped v; check "waited for the stick" grep -q "Insert the key device" "$FAKE/out"

t(){ echo 0 > "$BASE/cmdline.d/zbm.luks_token_wait"; tab "v $GK /keys/gk.key:LABEL=NOSUCHSTICK"; luks_unlock_all; }
run "key stick absent with zbm.luks_token_wait=0 -> no wait" t <<< $'pw'
check "mapped via passphrase" mapped v; check "did not wait" bash -c "! grep -q 'Insert the key device' $FAKE/out"

t(){ local v; for v in 0 7 08 010 999999 1000000 -5 +5 abc 1e3 "" 99999999999999999999; do printf '%s' "$v" > "$BASE/cmdline.d/zbm.luks_token_wait"; echo "[$v]=$(luks_token_wait)"; done; }
run "zbm.luks_token_wait values: leading zeros, out of range, not a number" t
for pair in "[0]=0" "[7]=7" "[08]=8" "[010]=10" "[999999]=999999" "[1000000]=10" "[-5]=10" "[+5]=10" "[abc]=10" "[1e3]=10" "[]=10" "[99999999999999999999]=10"; do
  check "zbm.luks_token_wait $pair" grep -qxF "$pair" "$FAKE/out"
done
t(){ printf '08' > "$BASE/cmdline.d/zbm.luks_token_wait"; tab "v $GK /keys/gk.key:LABEL=NOSUCHSTICK"; luks_unlock_all; }
run "key stick absent with zbm.luks_token_wait=08 -> waits 8 s, no arithmetic error" t < <(sleep 10; echo pw)
check "no base error" bash -c "! grep -q 'value too great for base' '$FAKE/out'"; check "waited 8 seconds" grep -q "Waiting 8 seconds" "$FAKE/out"; check "mapped after passphrase" mapped v

t(){ echo 2 > "$BASE/cmdline.d/zbm.luks_token_wait"; tab "v $GK /keys/gk.key:LABEL=NOSUCHSTICK" "w $L1 /keys/l1.key:LABEL=NOSUCHSTICK"; luks_unlock_all; }
run "two volumes on one missing stick: waits once, not per volume" t < <(sleep 4; printf 'pw\npw\n')
check "v mapped" mapped v; check "w mapped" mapped w; check "waited once" [ "$(grep -c 'Insert the key device' "$FAKE/out")" -eq 1 ]

t(){ echo 5 > "$BASE/cmdline.d/zbm.luks_token_wait"; tab "v $GK /keys/gk.key:LABEL=NOSUCHSTICK"; LUKS_RETRY=1 luks_unlock_all; }
run "import retry with a missing stick does not wait" t < /dev/null
check "did not wait" bash -c "! grep -q 'Insert the key device' $FAKE/out"

S2=$(losetup -f --show "$K/stick.img")
t(){ tab "v $GK /keys/missing.key:LABEL=FINALKEY"; luks_unlock_all; }
run "key stick present but key file missing on it" t <<< $'pw'
check "mapped via passphrase" mapped v; check "warned" grep -q "not found on" "$FAKE/log"; check "stick unmounted" bash -c "! grep -q luks-key /proc/mounts"
losetup -d "$S2"

echo; echo "passed: $PASS failed: $FAIL"; [ "$FAIL" -eq 0 ]
