//! What every image gets, by list (formerly the arrays of
//! `install-helpers.sh`); `cargo xtask metrics` counts the C programs of
//! those a Rust image gets (`xtask/src/metrics/image.rs`).

/// The programs every image needs.
pub const ESSENTIAL_BINARIES: &[&str] = &[
    "bash",
    "zfs",
    "zpool",
    "zdb",
    "lsblk",
    "hostid",
    "mount",
    "mount.zfs",
    "kexec",
    "mkdir",
    "tput",
    "head",
    "mktemp",
    "sort",
    "sed",
    "grep",
    "tail",
    "tac",
    "blkid",
    "awk",
    "ps",
    "env",
    "chmod",
    "chroot",
    "od",
    "stty",
    "insmod",
    "modinfo",
    "lsmod",
    "depmod",
    "dmesg",
    "fzf",
    "setsid",
    "cat",
];

/// Programs an image goes without when the build host has none.
pub const OPTIONAL_BINARIES: &[&str] = &["mbuffer", "column", "less"];

/// The udev rules of ZFS, which every image needs.
pub const UDEV_RULES: &[&str] = &["90-zfs.rules", "69-vdev.rules", "60-zvol.rules"];

/// Device-mapper udev rules let cryptsetup synchronize with udev and create
/// /dev/disk/by-* links for unlocked LUKS volumes; names vary by distribution.
pub const LUKS_UDEV_RULES: &[&str] = &[
    "10-dm.rules",
    "13-dm-disk.rules",
    "55-dm.rules",
    "60-persistent-storage-dm.rules",
    "95-dm-notify.rules",
];

/// Programs needed to unlock LUKS volumes that hold ZFS vdevs; `luks_binaries`
/// adds those of the legacy runtime.
pub const LUKS_BINARIES: &[&str] = &["cryptsetup", "dmsetup", "udevadm", "cut", "tr", "wc"];

/// The hashing and encoding programs of the legacy runtime's LUKS and TPM2
/// code. The Rust runtime computes them in its own process, so that secrets
/// never pass through other programs (ROADMAP.md CD3): its images leave them
/// out.
pub const LUKS_LEGACY_BINARIES: &[&str] = &["base64", "sha256sum"];

/// The program the legacy runtime's TPM2 code also needs.
pub const LUKS_LEGACY_TPM2_BINARIES: &[&str] = &["openssl"];

/// TPM and USB/HID drivers for TPM2 chips, FIDO2 security keys and OpenPGP
/// smart cards; missing or built-in modules are skipped.
pub const LUKS_TOKEN_MODULES: &[&str] = &[
    "tpm",
    "tpm_tis_core",
    "tpm_tis",
    "tpm_crb",
    "xhci_pci",
    "xhci_hcd",
    "ehci_pci",
    "ehci_hcd",
    "ohci_pci",
    "ohci_hcd",
    "uhci_hcd",
    "usbhid",
    "hid_generic",
    "hid",
];

/// Libraries that the systemd token plugins and tpm2-tools load with
/// `dlopen()`.
pub const LUKS_TOKEN_LIBS: &[&str] = &[
    "libtss2-esys.so.0",
    "libtss2-rc.so.0",
    "libtss2-mu.so.0",
    "libtss2-tctildr.so.0",
    "libtss2-tcti-device.so.0",
    "libfido2.so.1",
];

/// The programs that unlock systemd-tpm2 tokens without systemd's token plugin.
pub const LUKS_TPM2_BINARIES: &[&str] = &[
    "tpm2_load",
    "tpm2_startauthsession",
    "tpm2_sessionconfig",
    "tpm2_policypcr",
    "tpm2_policyauthvalue",
    "tpm2_unseal",
    "tpm2_flushcontext",
    "tpm2_getcap",
];

/// The programs that unlock systemd-fido2 tokens without systemd's token
/// plugin.
pub const LUKS_FIDO2_BINARIES: &[&str] = &["fido2-token", "fido2-assert"];

/// The programs of clevis LUKS2 tokens (TPM2, tang and sss policies).
pub const LUKS_CLEVIS_BINARIES: &[&str] = &[
    "jose",
    "luksmeta",
    "curl",
    "tpm2_createprimary",
    "tpm2_load",
    "tpm2_unseal",
    "tpm2_flushcontext",
    "tpm2_pcrread",
    "tpm2_getcap",
    "tpm2_startauthsession",
    "tpm2_policypcr",
    "tpm2_createpolicy",
];

/// The programs of YubiKey HMAC-SHA1 challenge-response.
pub const LUKS_YUBIKEY_BINARIES: &[&str] = &["ykchalresp", "ykinfo"];

/// dm-crypt and the kernel crypto modules commonly used by LUKS; any that are
/// missing (or built into the kernel) are silently skipped.
pub const LUKS_MODULES: &[&str] = &[
    "dm_mod",
    "dm_crypt",
    "dm_integrity",
    "af_alg",
    "algif_skcipher",
    "algif_hash",
    "cryptd",
    "crypto_simd",
    "gf128mul",
    "aes_generic",
    "aesni_intel",
    "aes_ce_blk",
    "aes_ce_cipher",
    "aes_neon_blk",
    "aes_neon_bs",
    "xts",
    "cbc",
    "ecb",
    "ctr",
    "lrw",
    "essiv",
    "sha256_generic",
    "sha256_ssse3",
    "sha512_generic",
    "sha512_ssse3",
    "sha2_ce",
    "sha256_arm64",
    "sha512_arm64",
    "crc32c_generic",
    "crc32c_cryptoapi",
    "crc32c_intel",
    "serpent_generic",
    "twofish_generic",
];

/// The kernel modules of ZFS, which every image needs.
pub const ESSENTIAL_MODULES: &[&str] = &[
    "zfs", "zcommon", "znvpair", "zavl", "zunicode", "zlua", "icp", "spl",
];

/// Kernel modules an image goes without when the kernel has none, or has them
/// built in.
pub const OPTIONAL_MODULES: &[&str] = &[
    "zlib_deflate",
    "zlib_inflate",
    "fat",
    "vfat",
    "nls_iso8859_1",
    "nls_cp437",
];
