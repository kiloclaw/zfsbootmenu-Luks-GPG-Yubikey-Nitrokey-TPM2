# Security policy

## Supported versions

Only the latest commit of the default branch (`master`) receives security fixes.

## Reporting a vulnerability

Report vulnerabilities privately with GitHub's private vulnerability reporting: open the **Security** tab of this
repository and choose **Report a vulnerability**. Do not open a public issue, pull request or discussion about it.

Include:

- what an attacker can do, and what they need (physical access, a stolen disk, network access to the SSH server in the
  image, and so on);
- the steps to reproduce it, with the ZFSBootMenu build, the configuration and the kernel command line;
- the unlocking methods involved (passphrase, key file, GnuPG, YubiKey, FIDO2, TPM2).

Problems in code that this fork shares with upstream ZFSBootMenu, such as the boot environment menu or `generate-zbm`,
also affect upstream: report them to the [upstream project](https://github.com/zbm-dev/zfsbootmenu) as well.

## Scope

In scope: the bootloader runtime (`legacy/zfsbootmenu/`), the LUKS and key handling code, the dracut and mkinitcpio
modules (`packaging/`), the build container, `generate-zbm`, and the Rust crates in `crates/`.

Out of scope: vulnerabilities in third-party software included in the images (the kernel, OpenZFS, cryptsetup, GnuPG,
systemd and so on), unless ZFSBootMenu uses it in an unsafe way. Report those to the projects concerned.
