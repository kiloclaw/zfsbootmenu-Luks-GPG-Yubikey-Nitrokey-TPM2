//! Following a boot from ZFSBootMenu into the test boot environment: the
//! prompts answered, the reports of the test hooks, and the checks common
//! to the scenarios.
//!
//! The hooks of the test images (`tests/luks/vm/hooks`) print
//! ZFSBootMenu's pool status (`ZS| ` lines) and `zluks status` (`ZL| `)
//! between `ZBMTEST-STATUS-BEGIN` and `-END`, then what the teardown left
//! (`ZBMTEST-TEARDOWN`). The boot environment's init prints `BE-*` lines
//! between `BE-INIT-START` and `BE-INIT-DONE`.

use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use crate::error::Result;
use crate::pyfmt;
use crate::record::Test;
use crate::settings::{PW, ZFSKEY};
use crate::vm::Vm;

mod menu;

pub use menu::{ask_menu_for_shell, shell_after_keyload, shell_from_menu};

/// The prompts and messages a boot shows, by kind.
pub mod prompt {
    /// ZFSBootMenu asks for a LUKS passphrase.
    pub const LUKS: &str = r"Passphrase \(leave empty to skip\): ";
    /// ZFSBootMenu asks for a ZFS key.
    pub const ZFSKEY: &str = r"assphrase for '[^']+':";
    /// ZFSBootMenu asks for a TPM2 PIN.
    pub const TPM2PIN: &str = r"TPM2 PIN \(leave empty to use another method\): ";
    /// ZFSBootMenu asks for the passphrase of a GnuPG key file.
    pub const GPG: &str = r"Key file passphrase \(leave empty to use another method\): ";
    /// A device is missing.
    pub const MISSING: &str = r"is missing";
    /// The import retry loop.
    pub const RETRY: &str = r"Unable to import";
    /// The status hook starts.
    pub const STATUS: &str = r"ZBMTEST-STATUS-BEGIN";
    /// A ZFS key could not be loaded.
    pub const KEYLOAD: &str = r"unable to load key";
    /// The new menu could not load a ZFS key: its error dialog, on the
    /// screen.
    pub const MENU_KEYLOAD: &str = r"screen:unable to load key";
    /// Nothing to boot.
    pub const EMERGENCY: &str = r"Dropping to an emergency shell";
    /// The boot menu of the legacy runtime.
    pub const MENU: &str = r"edit kcl";
    /// The new menu, on the screen: it redraws only the cells that change,
    /// so its text may lack letters.
    pub const NEW_MENU: &str = r"screen:BOOT ENVIRONMENTS";
    /// The new menu found no boot environment, on the screen.
    pub const NO_ENVIRONMENTS: &str = r"screen:No Boot Environments";
    /// The boot environment asks for a LUKS passphrase.
    pub const BE_ASK: &str = r"Please enter passphrase for disk|Enter passphrase for /dev/";
    /// The boot environment asks for its ZFS key.
    pub const BE_ZFSKEY: &str = r"(?:Enter passphrase for|Encrypted ZFS password for) [a-z]+/ROOT";
}

/// The kinds of prompts [`follow_boot`] answers or acts on.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Kind {
    /// [`prompt::LUKS`].
    Luks,
    /// [`prompt::ZFSKEY`].
    ZfsKey,
    /// [`prompt::TPM2PIN`].
    Tpm2Pin,
    /// [`prompt::GPG`].
    Gpg,
    /// [`prompt::MISSING`].
    Missing,
    /// [`prompt::RETRY`].
    Retry,
    /// [`prompt::STATUS`].
    Status,
    /// [`prompt::KEYLOAD`].
    KeyLoad,
    /// [`prompt::MENU_KEYLOAD`].
    MenuKeyLoad,
    /// [`prompt::EMERGENCY`].
    Emergency,
    /// [`prompt::MENU`].
    Menu,
    /// [`prompt::NEW_MENU`].
    NewMenu,
}

impl Kind {
    fn pattern(self) -> &'static str {
        match self {
            Kind::Luks => prompt::LUKS,
            Kind::ZfsKey => prompt::ZFSKEY,
            Kind::Tpm2Pin => prompt::TPM2PIN,
            Kind::Gpg => prompt::GPG,
            Kind::Missing => prompt::MISSING,
            Kind::Retry => prompt::RETRY,
            Kind::Status => prompt::STATUS,
            Kind::KeyLoad => prompt::KEYLOAD,
            Kind::MenuKeyLoad => prompt::MENU_KEYLOAD,
            Kind::Emergency => prompt::EMERGENCY,
            Kind::Menu => prompt::MENU,
            Kind::NewMenu => prompt::NEW_MENU,
        }
    }

    /// The name a boot records for an answered prompt.
    fn name(self) -> &'static str {
        match self {
            Kind::Luks => "luks",
            Kind::ZfsKey => "zfskey",
            Kind::Tpm2Pin => "tpm2pin",
            Kind::Gpg => "gpg",
            _ => "",
        }
    }
}

/// What a boot showed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Boot {
    /// The prompts and events, in order: `luks`, `zfskey`, `tpm2pin`, `gpg`,
    /// `menu`, `emergency`, `keyload-failed`, `be-zfskey`,
    /// `be-luks-prompt`, `be-panic`, `be-emergency`.
    pub prompts: Vec<String>,
    /// ZFSBootMenu's pool status.
    pub zbm_status: String,
    /// `zluks status` in ZFSBootMenu.
    pub zluks: String,
    /// What the teardown left: device-mapper devices, pools and state, if
    /// the report came.
    pub teardown: Option<(String, String, String)>,
    /// The boot environment's report.
    pub be: String,
    /// Its `BE-*` lines, by name.
    pub be_lines: HashMap<String, Vec<String>>,
}

impl Boot {
    /// Whether a prompt or event of that name was recorded.
    pub fn has_prompt(&self, name: &str) -> bool {
        self.prompts.iter().any(|prompt| prompt == name)
    }

    /// How many prompts or events of that name were recorded.
    pub fn count(&self, name: &str) -> usize {
        self.prompts.iter().filter(|prompt| *prompt == name).count()
    }

    /// The prompts as Python writes a list.
    pub fn prompts_text(&self) -> String {
        pyfmt::list(&self.prompts)
    }

    /// The pools and their states in ZFSBootMenu's status, in order.
    pub fn pools(&self) -> Pools {
        pools(&self.zbm_status)
    }

    /// The first value of the `BE-*` line `name`, or empty.
    pub fn be_line(&self, name: &str) -> &str {
        self.be_lines
            .get(name)
            .and_then(|lines| lines.first())
            .map_or("", String::as_str)
    }

    /// The pool status the boot environment reported (`BE| ` lines).
    pub fn be_status(&self) -> String {
        tagged(&self.be, "BE| ")
    }

    /// The feature states the boot environment reported (`BE-F| ` lines):
    /// (pool, feature) to state.
    pub fn be_features(&self) -> HashMap<(String, String), String> {
        let mut features = HashMap::new();
        for line in self.be.lines() {
            let Some((_, rest)) = line.split_once("BE-F| ") else {
                continue;
            };
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let [pool, feature, state, ..] = parts[..] {
                let feature = feature.replace("feature@", "");
                features.insert((pool.to_owned(), feature), state.to_owned());
            }
        }
        features
    }
}

/// Pools and their states, in the order of the status.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Pools(pub Vec<(String, String)>);

impl Pools {
    /// The state of `pool`, if it is listed.
    pub fn get(&self, pool: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(name, _)| name == pool)
            .map(|(_, state)| state.as_str())
    }

    /// The state of `pool` as Python writes `dict.get`: the state, or `None`.
    pub fn show(&self, pool: &str) -> String {
        pyfmt::option(self.get(pool))
    }

    /// Whether `pool` is listed.
    pub fn contains(&self, pool: &str) -> bool {
        self.get(pool).is_some()
    }

    /// The pools as Python writes a dictionary.
    pub fn text(&self) -> String {
        pyfmt::dict(&self.0)
    }

    /// The pool names, sorted, as Python writes a list.
    pub fn sorted_names(&self) -> String {
        let mut names: Vec<&str> = self.0.iter().map(|(name, _)| name.as_str()).collect();
        names.sort_unstable();
        pyfmt::list(&names)
    }
}

/// The pools and their states in `zpool status` output.
pub fn pools(text: &str) -> Pools {
    let mut pools: Vec<(String, String)> = Vec::new();
    let mut pool: Option<String> = None;
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(name) = line.strip_prefix("pool: ") {
            pool = name.split_whitespace().next().map(str::to_owned);
        }
        if let (Some(state), Some(name)) = (line.strip_prefix("state: "), &pool) {
            let state = state
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned();
            match pools.iter_mut().find(|(known, _)| known == name) {
                Some(entry) => entry.1 = state,
                None => pools.push((name.clone(), state)),
            }
        }
    }
    Pools(pools)
}

/// The lines of a `zpool status` text after `pool: <pool>`, up to the next
/// pool.
pub fn pool_block(status: &str, pool: &str) -> String {
    let header = format!("pool: {pool}");
    let mut lines = status.lines().skip_while(|line| line.trim() != header);
    lines.next();
    let block: Vec<&str> = lines
        .take_while(|line| !line.trim_start().starts_with("pool: "))
        .collect();
    block.join("\n")
}

/// The text after `tag` on the lines that have it.
fn tagged(text: &str, tag: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split_once(tag).map(|(_, rest)| rest))
        .collect();
    lines.join("\n")
}

/// A callback for a prompt: plugs in a disk, for example.
pub type OnPrompt<'a> = Box<dyn FnOnce(&mut Vm<'_>) -> Result<()> + 'a>;

/// How to follow a boot.
#[derive(Default)]
pub struct Follow<'a> {
    /// Replies to prompts, by kind; by default LUKS gets [`PW`], the ZFS
    /// key [`ZFSKEY`] and the others an empty line.
    pub answers: HashMap<Kind, Vec<String>>,
    /// Callbacks for [`Kind::Missing`] and [`Kind::Retry`], each run once.
    pub on: HashMap<Kind, OnPrompt<'a>>,
    /// Whether to stop after ZFSBootMenu's reports, without following the
    /// boot environment.
    pub no_be: bool,
    /// How long each wait may take, in seconds; default 2400.
    pub timeout: Option<u64>,
    /// Press Enter in the boot menu, for boots that do not start by
    /// themselves (after the emergency shell).
    pub menu: bool,
}

impl<'a> Follow<'a> {
    /// Replies `replies` to the prompts of `kind`.
    #[must_use]
    pub fn answer(mut self, kind: Kind, replies: &[&str]) -> Follow<'a> {
        self.answers.insert(
            kind,
            replies.iter().map(|&reply| reply.to_owned()).collect(),
        );
        self
    }

    /// Runs `callback` at the first prompt of `kind`.
    #[must_use]
    pub fn on(
        mut self,
        kind: Kind,
        callback: impl FnOnce(&mut Vm<'_>) -> Result<()> + 'a,
    ) -> Follow<'a> {
        self.on.insert(kind, Box::new(callback));
        self
    }
}

/// Follows a boot: answers prompts, captures ZFSBootMenu's pool status and
/// teardown report, then the boot environment's report.
///
/// # Errors
///
/// When an awaited message does not come.
pub fn follow_boot(vm: &mut Vm<'_>, mut follow: Follow<'_>) -> Result<Boot> {
    let timeout = follow.timeout.unwrap_or(2400);
    let mut boot = Boot::default();
    let mut kinds = vec![
        Kind::Luks,
        Kind::ZfsKey,
        Kind::Tpm2Pin,
        Kind::Gpg,
        Kind::Missing,
        Kind::Retry,
        Kind::Status,
        Kind::KeyLoad,
        Kind::MenuKeyLoad,
        Kind::Emergency,
    ];
    if follow.menu {
        kinds.extend([Kind::Menu, Kind::NewMenu]);
    }
    loop {
        let patterns: Vec<&str> = kinds.iter().map(|kind| kind.pattern()).collect();
        let kind = kinds[vm.expect(&patterns, timeout)?];
        match on_prompt(vm, &mut boot, &mut follow, kind)? {
            Next::Wait => {}
            Next::WaitWithoutMenu => {
                kinds.retain(|&known| !matches!(known, Kind::Menu | Kind::NewMenu));
            }
            Next::Reports => break,
            Next::Stop => return Ok(boot),
        }
    }
    reports(vm, &mut boot)?;
    if follow.no_be {
        return Ok(boot);
    }
    follow_be(vm, &mut boot, timeout)?;
    Ok(boot)
}

/// What [`follow_boot`] does after a prompt.
enum Next {
    /// Waits for the next prompt.
    Wait,
    /// Waits for the next prompt; the menu was left.
    WaitWithoutMenu,
    /// Reads the reports of the test hooks.
    Reports,
    /// Stops: the boot ends here.
    Stop,
}

/// Acts on a prompt of `kind`.
fn on_prompt(
    vm: &mut Vm<'_>,
    boot: &mut Boot,
    follow: &mut Follow<'_>,
    kind: Kind,
) -> Result<Next> {
    match kind {
        Kind::Status => return Ok(Next::Reports),
        Kind::KeyLoad | Kind::MenuKeyLoad => {
            boot.prompts.push("keyload-failed".into());
            return Ok(Next::Stop);
        }
        Kind::Menu | Kind::NewMenu => {
            boot.prompts.push("menu".into());
            vm.test().step("boot menu -> Enter");
            thread::sleep(Duration::from_secs(2));
            vm.send("\r")?;
            return Ok(Next::WaitWithoutMenu);
        }
        Kind::Emergency => {
            // ZFSBootMenu found nothing to boot; the caller may open the
            // shell.
            boot.prompts.push("emergency".into());
            vm.test().step("ZFSBootMenu dropped to its emergency shell");
            return Ok(Next::Stop);
        }
        Kind::Missing | Kind::Retry => {
            if let Some(callback) = follow.on.remove(&kind) {
                callback(vm)?;
            }
            if kind == Kind::Retry {
                vm.expect1("seconds", 120)?;
            }
        }
        Kind::Luks | Kind::ZfsKey | Kind::Tpm2Pin | Kind::Gpg => {
            answer(vm, boot, follow, kind)?;
        }
    }
    Ok(Next::Wait)
}

/// Answers a prompt of `kind`.
fn answer(vm: &mut Vm<'_>, boot: &mut Boot, follow: &mut Follow<'_>, kind: Kind) -> Result<()> {
    boot.prompts.push(kind.name().into());
    let default = match kind {
        Kind::Luks => PW,
        Kind::ZfsKey => ZFSKEY,
        _ => "",
    };
    let reply = follow
        .answers
        .get_mut(&kind)
        .filter(|replies| !replies.is_empty())
        .map_or_else(|| default.to_owned(), |replies| replies.remove(0));
    let shown = if reply.is_empty() {
        "(empty)"
    } else {
        "answered"
    };
    vm.test()
        .step(&format!("prompt {} -> {shown}", kind.name()));
    thread::sleep(Duration::from_millis(500));
    vm.sendline(&reply)
}

/// Reads the status and teardown reports of the test hooks.
fn reports(vm: &mut Vm<'_>, boot: &mut Boot) -> Result<()> {
    vm.expect1("ZBMTEST-STATUS-END", 600)?;
    let before = vm.before();
    boot.zbm_status = tagged(&before, "ZS| ");
    boot.zluks = tagged(&before, "ZL| ");
    vm.expect1(
        r"ZBMTEST-TEARDOWN dm=\[(.*?)\] pools=\[(.*?)\] state=\[(.*?)\]",
        600,
    )?;
    let group = |n| vm.group(n).trim().to_owned();
    boot.teardown = Some((group(1), group(2), group(3)));
    Ok(())
}

/// Follows the boot environment to its report.
fn follow_be(vm: &mut Vm<'_>, boot: &mut Boot, timeout: u64) -> Result<()> {
    let patterns = [
        r"BE-INIT-START",
        prompt::BE_ASK,
        prompt::BE_ZFSKEY,
        r"Kernel panic",
        r"emergency",
    ];
    let mut index = vm.expect(&patterns, timeout)?;
    while index == 2 {
        boot.prompts.push("be-zfskey".into());
        vm.test()
            .step("boot environment asks for the ZFS key -> answered");
        thread::sleep(Duration::from_secs(1));
        vm.sendline(ZFSKEY)?;
        index = vm.expect(&patterns, timeout)?;
    }
    if index != 0 {
        let event = ["", "be-luks-prompt", "", "be-panic", "be-emergency"][index];
        boot.prompts.push(event.into());
        return Ok(());
    }
    vm.expect1("BE-INIT-DONE", timeout)?;
    boot.be = vm.before();
    for line in boot.be.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("BE-") else {
            continue;
        };
        let name_len = rest
            .find(|c: char| !(c.is_ascii_uppercase() || c == '-'))
            .unwrap_or(rest.len());
        let Some(value) = rest[name_len..].strip_prefix(':') else {
            continue;
        };
        if name_len == 0 {
            continue;
        }
        let name = format!("BE-{}", &rest[..name_len]);
        boot.be_lines
            .entry(name)
            .or_default()
            .push(value.trim().to_owned());
    }
    if vm.expect_eof(300).is_err() {
        vm.kill();
    }
    vm.stop_tpm();
    Ok(())
}

/// The checks of a boot that reached the test boot environment.
pub fn check_be(
    test: &Test,
    boot: &Boot,
    root_pool: &str,
    pools: &[&str],
    states: &[(&str, &str)],
) {
    let state_of = |pool: &str| {
        states
            .iter()
            .find(|(name, _)| *name == pool)
            .map_or("ONLINE", |(_, state)| *state)
    };
    test.check(
        !boot.has_prompt("BE-INIT-START") && !boot.be.is_empty(),
        "kexec into the ZFS 2.4.4 boot environment",
    );
    if boot.be.is_empty() {
        return;
    }
    let kernel = boot.be_line("BE-KERNEL");
    test.check(
        kernel.starts_with("7.3"),
        &format!("boot environment kernel {kernel}"),
    );
    let zfs = boot.be_line("BE-ZFS-VERSION");
    test.check(
        zfs.contains("zfs-2.4.4") && zfs.contains("zfs-kmod-2.4.4"),
        &format!("boot environment runs ZFS 2.4.4 ({zfs})"),
    );
    let root = boot.be_line("BE-ROOT");
    test.check(
        root.starts_with(&format!("{root_pool}/")) && format!(" {root} ").contains(" zfs "),
        &format!("root file system {root}"),
    );
    let be_pools = self::pools(&boot.be_status());
    for pool in pools {
        let want = state_of(pool);
        let got = be_pools.show(pool);
        test.check(
            be_pools.get(pool) == Some(want),
            &format!("boot environment imported {pool}: {got} (expected {want})"),
        );
    }
    test.check(
        boot.be_line("BE-WRITE") == "ok",
        "root dataset is writable in the boot environment",
    );
    for line in boot.be_lines.get("BE-SCRUB").into_iter().flatten() {
        let pool = line.split_whitespace().next().unwrap_or_default();
        let healthy = line.contains("is healthy") || line.contains("errors: No known data errors");
        if state_of(pool) == "ONLINE" {
            let rest: String = line[pool.len()..].trim().chars().take(80).collect();
            test.check(
                healthy,
                &format!("scrub of {pool} in the boot environment: {rest}"),
            );
        }
    }
}

/// Every LUKS mapping closed and every pool exported before kexec, except
/// `readonly_pools`: pools imported read-only through unencrypted devices
/// only, which ZFSBootMenu leaves imported as it does without LUKS.
pub fn check_teardown(test: &Test, boot: &Boot, readonly_pools: &[&str]) {
    let Some((dm, pools, state)) = &boot.teardown else {
        test.check(false, "teardown report missing");
        return;
    };
    let mut left: Vec<&str> = pools
        .split_whitespace()
        .filter(|pool| !readonly_pools.contains(pool))
        .collect();
    left.sort_unstable();
    left.dedup();
    test.check(
        dm.is_empty() && left.is_empty() && state.is_empty(),
        &format!(
            "teardown exported pools and closed volumes (dm=[{dm}] pools=[{pools}] state=[{state}])"
        ),
    );
}

#[cfg(test)]
mod tests;
