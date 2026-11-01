//! ZFS pools on LUKS volumes: the programs that unlock them, LUKS2 tokens,
//! GnuPG and its public keys, YubiKeys, and the crypttab with the key files
//! and key scripts it names.

use std::path::{Path, PathBuf};

use zbm_tools::host::{gpg_components, gpg_keyring, gpg_packets};

use super::libs::find_library;
use super::lists::{
    LUKS_BINARIES, LUKS_CLEVIS_BINARIES, LUKS_FIDO2_BINARIES, LUKS_LEGACY_BINARIES,
    LUKS_LEGACY_TPM2_BINARIES, LUKS_TOKEN_LIBS, LUKS_TPM2_BINARIES, LUKS_YUBIKEY_BINARIES,
};
use super::{Entry, Image};

/// Where systemd's cryptsetup token plugins may be.
const PLUGIN_DIRS: [&str; 6] = [
    "/usr/lib/*/cryptsetup",
    "/usr/lib64/cryptsetup",
    "/usr/lib/cryptsetup",
    "/lib/*/cryptsetup",
    "/lib64/cryptsetup",
    "/lib/cryptsetup",
];

/// What the PC/SC daemon reads.
const PCSC_PATHS: [&str; 4] = [
    "/usr/lib/pcsc",
    "/usr/lib64/pcsc",
    "/etc/reader.conf.d",
    "/etc/libccid_Info.plist",
];

/// Something the LUKS configuration names did not go into the image; the
/// plan warns what.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Incomplete;

/// `Ok` when `whole`.
fn whole_if(whole: bool) -> Result<(), Incomplete> {
    if whole { Ok(()) } else { Err(Incomplete) }
}

/// Whether the image unlocks LUKS volumes (`zfsbootmenu_luks`).
pub fn is_enabled(image: &Image) -> bool {
    image.config.is_on("zfsbootmenu_luks", true)
}

/// Whether the image gets the LUKS programs: they are asked for, and the
/// host has cryptsetup.
pub fn is_wanted(image: &Image) -> bool {
    is_enabled(image) && image.host.which("cryptsetup").is_some()
}

/// The programs that unlock LUKS volumes in an image of the runtime: the
/// Rust runtime needs none of the legacy runtime's hashing and encoding
/// programs.
pub fn binaries(image: &Image) -> Vec<&'static str> {
    let mut names = LUKS_BINARIES.to_vec();
    if !image.is_rust() {
        names.extend(LUKS_LEGACY_BINARIES);
    }
    names
}

/// LUKS2 tokens: systemd's token plugins for systemd-tpm2 and
/// systemd-fido2, the libraries they and tpm2-tools load, the programs of
/// the native support, and clevis.
fn tokens(image: &mut Image) {
    if !image.config.is_on("zfsbootmenu_luks_tokens", true) {
        return;
    }
    if image.config.is_on("zfsbootmenu_luks_token_plugins", true) {
        for plugin in ["systemd-tpm2", "systemd-fido2"] {
            let file = format!("libcryptsetup-token-{plugin}.so");
            let found = PLUGIN_DIRS
                .iter()
                .flat_map(|dir| image.host.glob(dir))
                .map(|dir| dir.join(&file))
                .find(|path| image.host.exists(path));
            match found {
                Some(path) => image.plan.add(Entry::Library { src: path }),
                None => image.plan.info(format!(
                    "cryptsetup {plugin} token plugin not found; using native support"
                )),
            }
        }
    }
    for library in LUKS_TOKEN_LIBS {
        if let Some(path) = find_library(image.host, library) {
            image.plan.add(Entry::Library { src: path });
        }
    }
    let mut tpm2 = LUKS_TPM2_BINARIES.to_vec();
    if image.is_rust() {
        // Measured boot reads and extends PCRs 8 and 9 before kexec (SE10)
        tpm2.extend(["tpm2_pcrread", "tpm2_pcrextend"]);
    } else {
        tpm2.extend(LUKS_LEGACY_TPM2_BINARIES);
    }
    if !image.executables(&tpm2).is_empty() {
        image.plan.info(format!(
            "{}: incomplete; native TPM2 unlocking may be unavailable",
            tpm2.join(" ")
        ));
    }
    if !image.executables(LUKS_FIDO2_BINARIES).is_empty() {
        image
            .plan
            .info("fido2-tools not found; native FIDO2 unlocking unavailable");
    }
    clevis(image);
}

/// clevis, its subcommands and the programs its pins run.
fn clevis(image: &mut Image) {
    if !image.config.is_on("zfsbootmenu_luks_clevis", true) {
        return;
    }
    let Some(clevis) = image.host.which("clevis") else {
        return;
    };
    let dir = clevis.parent().unwrap_or(Path::new("/")).to_owned();
    let mut commands = vec![clevis];
    commands.extend(image.host.glob(&format!("{}/clevis-*", dir.display())));
    for command in commands {
        if image.host.is_runnable(&command) {
            image.plan.binary(command.clone(), command);
        } else if image.host.exists(&command) {
            image.install(&command, &command.clone());
        }
    }
    image.executables(LUKS_CLEVIS_BINARIES);
}

/// GnuPG, for key files encrypted to a smart card or a passphrase: its
/// programs and agents, the PC/SC daemon when asked for
/// (`zfsbootmenu_luks_pcscd`), and, but in a release, a keyring of the
/// public keys of `zfsbootmenu_luks_gpg_keys` and
/// `zfsbootmenu_luks_gpg_pubkeys`.
fn gnupg(image: &mut Image) -> Result<(), Incomplete> {
    if !image.config.is_on("zfsbootmenu_luks_gpg", true) || image.host.which("gpg").is_none() {
        return Ok(());
    }
    let mut whole = image
        .executables(&["gpg", "gpgconf", "gpg-connect-agent"])
        .is_empty();
    // gpg-agent, scdaemon and keyboxd are where each distribution puts them
    for line in gpg_components(image.host.runner).lines() {
        let mut fields = line.split(':');
        let (Some(component), Some(path)) = (fields.next(), fields.nth(1)) else {
            continue;
        };
        let path = PathBuf::from(path);
        if matches!(component, "gpg-agent" | "scdaemon" | "keyboxd")
            && image.host.is_runnable(&path)
        {
            image.plan.binary(path.clone(), path);
        }
    }
    if image.config.is_on("zfsbootmenu_luks_pcscd", false) && image.executable("pcscd").is_some() {
        for path in PCSC_PATHS.map(Path::new) {
            if image.host.is_dir(path) {
                image.tree(path);
            } else if image.host.exists(path) {
                image.install(path, path);
            }
        }
    }
    if image.config.is_release() {
        return whole_if(whole);
    }
    let ids = image.config.words("zfsbootmenu_luks_gpg_keys");
    let keyfiles = image.config.words("zfsbootmenu_luks_gpg_pubkeys");
    if ids.is_empty() && keyfiles.is_empty() {
        return whole_if(whole);
    }
    whole &= keyring(image, &ids, &keyfiles).is_ok();
    whole_if(whole)
}

/// The keyring of the public keys `ids` of the host's keyring and of the
/// key files `keyfiles`, built in the host's scratch directory, in
/// `/etc/zfsbootmenu/gnupg`.
fn keyring(image: &mut Image, ids: &[String], keyfiles: &[String]) -> Result<(), Incomplete> {
    let host = image.host;
    if host.files.create_dir_all(&host.scratch).is_err() {
        image
            .plan
            .warn("unable to make a directory for the GnuPG keyring");
        return Err(Incomplete);
    }
    let _ = host.files.set_mode(&host.scratch, 0o700);
    let (kept, failed) = gpg_keyring(host.runner, host.files, &host.scratch, (ids, keyfiles));
    remove_tree(host, &host.scratch);
    let whole = failed.is_empty();
    for warning in failed {
        image.plan.warn(warning);
    }
    let home = Path::new("/etc/zfsbootmenu/gnupg");
    image.plan.add(Entry::Dir {
        dest: home.to_owned(),
        mode: 0o700,
    });
    for (name, contents) in kept {
        image.plan.write(home.join(name), 0o600, contents);
    }
    whole_if(whole)
}

/// Removes `path` and what is below it, as far as it can.
fn remove_tree(host: &super::Host, path: &Path) {
    for name in host.names(path) {
        let below = path.join(name);
        if host.is_dir(&below) && !host.is_link(&below) {
            remove_tree(host, &below);
        } else {
            let _ = host.files.remove_file(&below);
        }
    }
    let _ = host.files.remove_dir(path);
}

/// The GnuPG-encrypted key file `file`, at its path with mode 0600: only
/// encrypted files, which are useless without their smart card or
/// passphrase.
fn gpg_keyfile(image: &mut Image, file: &Path) -> Result<(), Incomplete> {
    if !image.host.is_file(file) {
        image
            .plan
            .warn(format!("GnuPG key file {} not found", file.display()));
        return Err(Incomplete);
    }
    let packets = gpg_packets(image.host.runner, file);
    let starts = |packet: &str| packets.lines().any(|line| line.starts_with(packet));
    if !starts(":pubkey enc packet:") && !starts(":symkey enc packet:") {
        image.plan.warn(format!(
            "{} is not GnuPG-encrypted; refusing to embed it in the image",
            file.display()
        ));
        return Err(Incomplete);
    }
    if !starts(":pubkey enc packet:") {
        image.plan.warn(format!(
            "{} is only protected by a passphrase; anyone with the image can attack it offline",
            file.display()
        ));
    }
    image.plan.add(Entry::File {
        src: file.to_owned(),
        dest: file.to_owned(),
        mode: Some(0o600),
    });
    Ok(())
}

/// One line of the crypttab: its key file, when it is embedded, and its
/// key scripts.
fn crypttab_line(image: &mut Image, line: &str) -> Result<(), Incomplete> {
    let mut fields = line.split_whitespace();
    let name = fields.next().unwrap_or_default();
    if name.is_empty() || name.starts_with('#') {
        return Ok(());
    }
    let keyfile = fields.nth(1).unwrap_or_default();
    let options = fields.next().unwrap_or_default();
    let mut whole = true;
    // Plain key files are not embedded: the image is not encrypted
    let is_gpg = [".gpg", ".asc", ".pgp"]
        .iter()
        .any(|end| keyfile.ends_with(end))
        || options.split(',').any(|option| option == "gpg");
    match keyfile {
        "" | "-" | "none" => {}
        _ if keyfile.starts_with('/') && keyfile.contains(':') => {}
        _ if is_gpg => whole &= gpg_keyfile(image, Path::new(keyfile)).is_ok(),
        _ => image.plan.warn(format!(
            "crypttab entry '{name}' uses key file '{keyfile}', which is NOT embedded in the \
             image; provide it at boot time (e.g., from an early-setup hook) or use path:device"
        )),
    }
    for script in options
        .split(',')
        .filter_map(|option| option.strip_prefix("keyscript="))
    {
        let path = if script.starts_with('/') {
            PathBuf::from(script)
        } else {
            Path::new("/lib/cryptsetup/scripts").join(script)
        };
        if image.host.is_runnable(&path) {
            image.plan.binary(path.clone(), path);
        } else {
            image.plan.warn(format!(
                "key script '{}' for crypttab entry '{name}' not found",
                path.display()
            ));
            whole = false;
        }
    }
    whole_if(whole)
}

/// The LUKS configuration: tokens, GnuPG and YubiKeys, and, but in a
/// release, the crypttab (`zfsbootmenu_luks_crypttab`) with the key files
/// and key scripts it names. The builder's lines install dm-crypt's
/// programs, modules and rules.
///
/// # Errors
///
/// [`Incomplete`] when something it names could not go in.
pub fn luks(image: &mut Image) -> Result<(), Incomplete> {
    if !is_enabled(image) {
        return Ok(());
    }
    if image.host.which("cryptsetup").is_none() {
        image
            .plan
            .warn("cryptsetup not found; ZFS pools on LUKS volumes will not be unlocked");
        return Ok(());
    }
    tokens(image);
    let mut whole = gnupg(image).is_ok();
    if image.config.is_on("zfsbootmenu_luks_yubikey", true) {
        image.executables(LUKS_YUBIKEY_BINARIES);
    }
    let crypttab = PathBuf::from(
        image
            .config
            .or("zfsbootmenu_luks_crypttab", "/etc/zfsbootmenu/crypttab"),
    );
    if image.config.is_release() {
        return whole_if(whole);
    }
    let Some(text) = image.host.read(&crypttab) else {
        return whole_if(whole);
    };
    image.plan.add(Entry::File {
        src: crypttab,
        dest: "/etc/zfsbootmenu/crypttab".into(),
        mode: Some(0o600),
    });
    for line in text.lines() {
        whole &= crypttab_line(image, line.trim_end_matches('\r')).is_ok();
    }
    whole_if(whole)
}
