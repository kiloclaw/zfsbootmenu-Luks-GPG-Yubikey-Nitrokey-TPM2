# shellcheck shell=bash
# Shell helpers the VM tests define in the ZFSBootMenu recovery shell, line by
# line. Every function is written on one line or with explicit continuation, and
# comment lines are skipped when sending.
#
# mkluks PASSPHRASE "PBKDF OPTIONS" DEVICE... : format LUKS2 volumes, open each
# as luks-<UUID> with TRIM allowed and stored in the header, and append the
# mapping to the array V and the UUID to the array U
mkluks() { local pw="$1" opts="$2" d u; shift 2; for d in "$@"; do printf '%s' "${pw}" | cryptsetup luksFormat -q --type luks2 ${opts} --key-file=- "${d}" || return 1; u="$(cryptsetup luksUUID "${d}")"; printf '%s' "${pw}" | cryptsetup open --allow-discards --persistent --key-file=- "${d}" "luks-${u}" || return 1; V+=( "/dev/mapper/luks-${u}" ); U+=( "${u}" ); done; }
# mkbe POOL [zfs create options] : create POOL/ROOT/be holding the test boot
# environment from the PAYLOAD disk and make it the pool's bootfs
mkbe() { local pool="$1"; shift; zfs create -o mountpoint=none "${pool}/ROOT" && zfs create -o mountpoint=/ -o canmount=noauto "$@" "${pool}/ROOT/be" && zpool set bootfs="${pool}/ROOT/be" "${pool}" || return 1; mkdir -p /be /pl; grep -q ' /pl ' /proc/mounts || mount -o ro /dev/disk/by-label/PAYLOAD /pl || return 1; mount -t zfs -o zfsutil "${pool}/ROOT/be" /be && cp -a /pl/rootfs/. /be/ && mkdir -p /be/boot && cp /pl/boot/vmlinuz-* /pl/boot/initramfs-* /be/boot/ && umount /be; }
# becmdline DATASET [UUID=KEY ...] UUID... : boot environment command line that
# unlocks the given LUKS volumes with the key file in its initramfs; an
# argument of the form UUID=/path selects another key file for that volume
becmdline() { local ds="$1" cl="console=ttyS0 loglevel=4 rd.luks.key=/etc/zbmtest/be.key" u; shift; for u in "$@"; do case "${u}" in *=*) cl="${cl} rd.luks.key=${u} rd.luks.uuid=${u%%=*}" ;; *) cl="${cl} rd.luks.uuid=${u}" ;; esac; done; zfs set org.zfsbootmenu:commandline="${cl}" "${ds}"; }
# finish : export every pool and close every LUKS mapping
finish() { grep -q ' /pl ' /proc/mounts && umount /pl; zpool export -a && for m in /dev/mapper/luks-* /dev/mapper/m-*; do [ -e "${m}" ] && cryptsetup close "${m##*/}"; done; ls /dev/mapper; }
# features POOL : list features that are active
features() { zpool get -H -o property,value all "$1" | awk '$1 ~ /^feature@/ && $2 == "active" { sub("feature@", "", $1); printf "%s ", $1 } END { print "" }'; }
