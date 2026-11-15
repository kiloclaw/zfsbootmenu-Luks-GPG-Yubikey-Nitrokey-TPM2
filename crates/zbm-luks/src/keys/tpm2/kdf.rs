//! How a systemd-tpm2 token's PIN becomes what the TPM checks, as the token
//! records it:
//!
//! - the PIN itself, for tokens of systemd before 255;
//! - PBKDF2 with the token's `tpm2_salt`, for systemd 255 to 261 and systemd
//!   262's `--tpm2-with-pin=direct`;
//! - Argon2id with the token's salt and the costs of its
//!   `tpm2_argon2id_memcost`, `_iterations` and `_lanes`, for systemd 262's
//!   `--tpm2-with-pin=yes`.
//!
//! The fields come from the LUKS header, which a tampered disk or a hostile
//! USB device chooses (input I8 of `docs/dev/threat-model.md`). [`PinKdf::of`]
//! reads them with systemd's own parser ([`Token::argon2id`]), and refuses,
//! for a PIN that is to be derived, what Argon2 could never derive: more
//! lanes than Argon2 has (RFC 9106, section 3.1), less than 8 KiB of memory
//! per lane, 4 TiB of memory or more, and a salt shorter than 8 bytes or
//! missing. Nothing is allocated or derived before that. The salt is
//! decoded as systemd decodes it, with whitespace anywhere skipped.
//! [`check_systemd_rules`] applies systemd's parser alone, to every token.
//!
//! Before a derivation, and before its PIN is asked for, [`check_cost`]
//! checks that the machine can afford it now: its memory cost within the
//! machine's memory, as systemd requires; the memory Argon2 uses, with a
//! margin, within what is available, from `/proc/meminfo` and the limits of
//! the process's cgroups ([`Memory::read`]), refused when they cannot be
//! read; its work (memory times passes) within [`WORK_CAP`]; and the work of
//! the whole unlocking run within its budget, [`RUN_BUDGET`].

use std::path::{Path, PathBuf};

use zbm_crypt::Token;
use zbm_crypt::token::BadField;
use zbm_platform::files::Files;

use crate::crypto::base64_decode;
use crate::crypto::kdf::KdfError;

/// The most lanes Argon2 has: 2²⁴ − 1.
const MAX_LANES: u32 = 0x00FF_FFFF;
/// The least memory per lane, in KiB, that Argon2 accepts.
const MIN_KIB_PER_LANE: u64 = 8;
/// The shortest salt Argon2 accepts, in bytes; systemd writes 32.
const MIN_SALT: usize = 8;
/// A GiB, in bytes.
const GIB: u64 = 1 << 30;
/// The most work one derivation may cost: 32 GiB-passes, in bytes · passes,
/// some 40 seconds on one core of a fast machine.
pub(super) const WORK_CAP: u128 = 32 << 30;
/// The most work the derivations of one unlocking run may cost together:
/// 64 GiB-passes, in bytes · passes.
pub(super) const RUN_BUDGET: u128 = 64 << 30;
/// The least memory left to the rest of the system while Argon2 works.
const MIN_MARGIN: u64 = 64 << 20;
/// Where the kernel's cgroup2 file system is mounted.
const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// How a PIN token's PIN is turned into the TPM's authorization value.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum PinKdf {
    /// The PIN as it is typed.
    Plain,
    /// PBKDF2-HMAC-SHA256 with the token's salt. A salt that is not base64
    /// is empty, as it always was here.
    Pbkdf2 {
        /// The decoded `tpm2_salt`.
        salt: Vec<u8>,
    },
    /// Argon2id with the token's salt and costs.
    Argon2id {
        /// The decoded `tpm2_salt`, 8 bytes or more.
        salt: Vec<u8>,
        /// The costs, valid for a derivation.
        params: Argon2idParams,
    },
}

/// The costs of an Argon2id PIN, checked so that Argon2 can derive with
/// them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Argon2idParams {
    /// The memory cost as the token gives it, in bytes.
    pub(super) memcost_bytes: u64,
    /// The memory, in KiB: the token's bytes divided by 1024, rounded down,
    /// as systemd passes them to OpenSSL.
    pub(super) memory_kib: u32,
    /// The passes over the memory.
    pub(super) iterations: u32,
    /// The lanes, from 1 to 2²⁴ − 1, with at least 8 KiB each.
    pub(super) lanes: u32,
}

/// Why a PIN token's Argon2id fields cannot be used.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Malformed {
    /// systemd's own parser refuses them.
    #[error(transparent)]
    Field(#[from] BadField),
    /// There is no salt.
    #[error("tpm2_salt is missing")]
    NoSalt,
    /// The salt is not base64.
    #[error("tpm2_salt is not base64")]
    SaltNotBase64,
    /// The salt is shorter than Argon2 accepts.
    #[error("tpm2_salt has {0} bytes, fewer than 8")]
    ShortSalt(usize),
    /// The memory is 2³² KiB or more.
    #[error("tpm2_argon2id_memcost is 4 TiB or more")]
    Memory,
    /// There are more lanes than Argon2 has.
    #[error("tpm2_argon2id_lanes is more than 16777215")]
    Lanes,
    /// The memory is less than 8 KiB per lane.
    #[error("tpm2_argon2id_memcost is less than 8 KiB per lane")]
    MemoryPerLane,
}

impl PinKdf {
    /// How the PIN of the PIN token `token` is derived.
    ///
    /// # Errors
    ///
    /// [`Malformed`] when the Argon2id fields are not systemd's, or are, but
    /// Argon2 could not derive with them.
    pub(super) fn of(token: &Token) -> Result<PinKdf, Malformed> {
        let salt = token.text("tpm2_salt");
        let Some(fields) = token.argon2id()? else {
            return Ok(salt.map_or(PinKdf::Plain, |salt| PinKdf::Pbkdf2 {
                salt: base64_decode(salt).unwrap_or_default(),
            }));
        };
        let salt =
            systemd_base64(salt.ok_or(Malformed::NoSalt)?).ok_or(Malformed::SaltNotBase64)?;
        if salt.len() < MIN_SALT {
            return Err(Malformed::ShortSalt(salt.len()));
        }
        let memory_kib =
            u32::try_from(fields.memcost_bytes / 1024).map_err(|_| Malformed::Memory)?;
        if fields.lanes > MAX_LANES {
            return Err(Malformed::Lanes);
        }
        if u64::from(memory_kib) < MIN_KIB_PER_LANE * u64::from(fields.lanes) {
            return Err(Malformed::MemoryPerLane);
        }
        let params = Argon2idParams {
            memcost_bytes: fields.memcost_bytes,
            memory_kib,
            iterations: fields.iterations,
            lanes: fields.lanes,
        };
        Ok(PinKdf::Argon2id { salt, params })
    }
}

/// Whether `token`'s Argon2id fields pass the rules systemd's parser
/// applies to every token, with a PIN or without: those of
/// [`Token::argon2id`], and a salt when the three fields are there.
///
/// # Errors
///
/// [`Malformed`] for the first rule the fields break.
pub(super) fn check_systemd_rules(token: &Token) -> Result<(), Malformed> {
    if token.argon2id()?.is_some() && token.text("tpm2_salt").is_none() {
        return Err(Malformed::NoSalt);
    }
    Ok(())
}

impl Argon2idParams {
    /// The memory Argon2 works in, in bytes: whole groups of 4 · lanes
    /// blocks of 1 KiB, as many as fit in the memory cost. At most 2⁴²
    /// bytes, as the memory is below 2³² KiB.
    pub(super) fn need(&self) -> u64 {
        let group = 4 * u64::from(self.lanes.max(1));
        u64::from(self.memory_kib) / group * group * 1024
    }

    /// The work of a derivation: the memory Argon2 works in times its
    /// passes, in bytes · passes.
    pub(super) fn work(&self) -> u128 {
        u128::from(self.need()) * u128::from(self.iterations)
    }
}

/// The memory a derivation may use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Memory {
    /// The machine's memory (`MemTotal`), in bytes, from which the margin
    /// is reckoned.
    pub(super) machine: u64,
    /// The machine's memory capped by the `memory.max` of the process's
    /// cgroups, in bytes.
    pub(super) total: u64,
    /// The memory available (`MemAvailable`), capped by the room each of
    /// the process's cgroups has left (`memory.max` less `memory.current`),
    /// in bytes.
    pub(super) available: u64,
}

impl Memory {
    /// The memory of the machine and of the process's cgroups now; `None`
    /// when `/proc/meminfo`, `/proc/self/cgroup` or a cgroup's limit cannot
    /// be read, or when the process is in a cgroup below the root and the
    /// cgroup2 file system is not at `/sys/fs/cgroup`. A cgroup without a
    /// limit caps nothing; so does a kernel without cgroups, one with
    /// cgroup v1 alone, and the root group where cgroup2 is not mounted, as
    /// in the initramfs.
    pub(super) fn read(files: &dyn Files) -> Option<Memory> {
        let meminfo = files.read(Path::new("/proc/meminfo")).ok()?;
        let mut memory = parse_meminfo(&String::from_utf8_lossy(&meminfo))?;
        let cgroup = match files.read(Path::new("/proc/self/cgroup")) {
            Ok(cgroup) => cgroup,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(memory),
            Err(_) => return None,
        };
        let dirs = cgroup_dirs(&String::from_utf8_lossy(&cgroup))?;
        let in_root = dirs.len() == 1;
        for dir in dirs {
            match cgroup_limit(files, &dir)? {
                Limit::Max { max, current } => {
                    memory.total = memory.total.min(max);
                    memory.available = memory.available.min(max.saturating_sub(current));
                }
                Limit::Unlimited => {}
                Limit::Unmounted if in_root => {}
                Limit::Unmounted => return None,
            }
        }
        Some(memory)
    }

    /// The memory left to the rest of the system while Argon2 works: a
    /// sixteenth of the machine's, and at least 64 MiB.
    pub(super) fn margin(&self) -> u64 {
        (self.machine / 16).max(MIN_MARGIN)
    }
}

/// `MemTotal` and `MemAvailable` of `/proc/meminfo`, in bytes.
fn parse_meminfo(text: &str) -> Option<Memory> {
    let field = |name: &str| {
        let line = text.lines().find_map(|line| line.strip_prefix(name))?;
        let kib = line
            .strip_prefix(':')?
            .trim()
            .strip_suffix("kB")?
            .trim_end();
        kib.parse::<u64>().ok()?.checked_mul(1024)
    };
    let machine = field("MemTotal")?;
    Some(Memory {
        machine,
        total: machine,
        available: field("MemAvailable")?,
    })
}

/// The directories of the process's cgroup2 group and of its parents up to
/// the root, from `/proc/self/cgroup`; none without cgroup2, and `None` for
/// a path that is not one. The root is among them: in a cgroup namespace,
/// as in a container, it is a group with a limit of its own.
fn cgroup_dirs(text: &str) -> Option<Vec<PathBuf>> {
    let Some(path) = text.lines().find_map(|line| line.strip_prefix("0::")) else {
        return Some(Vec::new());
    };
    let path = path.strip_prefix('/')?;
    let mut dir = PathBuf::from(CGROUP_ROOT);
    let mut dirs = vec![dir.clone()];
    if path.is_empty() {
        return Some(dirs);
    }
    if path
        .split('/')
        .any(|name| name.is_empty() || name == "." || name == "..")
    {
        return None;
    }
    for name in path.split('/') {
        dir.push(name);
        dirs.push(dir.clone());
    }
    dirs.reverse();
    Some(dirs)
}

/// The memory limit of a cgroup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Limit {
    /// It has none: no `memory.max`, as the root group or one without the
    /// memory controller has, or `max`.
    Unlimited,
    /// It is not there: the cgroup2 file system is not mounted at
    /// `/sys/fs/cgroup`.
    Unmounted,
    /// Its `memory.max` and `memory.current`, in bytes.
    Max {
        /// The most it may use.
        max: u64,
        /// What it uses.
        current: u64,
    },
}

/// The memory limit of the cgroup `dir`; `None` when it cannot be read.
fn cgroup_limit(files: &dyn Files, dir: &Path) -> Option<Limit> {
    let read = |name: &str| {
        files
            .read(&dir.join(name))
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
    };
    let not_found = |error: &std::io::Error| error.kind() == std::io::ErrorKind::NotFound;
    let max = match read("memory.max") {
        Ok(max) if max == "max" => return Some(Limit::Unlimited),
        Ok(max) => max.parse::<u64>().ok()?,
        // Every group has `cgroup.controllers`; without `memory.max`, the
        // group has no memory controller, or the group is not there.
        Err(error) if not_found(&error) => {
            return match read("cgroup.controllers") {
                Ok(_) => Some(Limit::Unlimited),
                Err(error) if not_found(&error) => Some(Limit::Unmounted),
                Err(_) => None,
            };
        }
        Err(_) => return None,
    };
    let current = read("memory.current").ok()?.parse::<u64>().ok()?;
    Some(Limit::Max { max, current })
}

/// Why a derivation cannot run now.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum TooCostly {
    /// Its work is above [`WORK_CAP`].
    #[error("its work is {} GiB-passes, more than the 32 allowed", gib_passes(*.work))]
    Work {
        /// Its work, in bytes · passes.
        work: u128,
    },
    /// The run has spent too much of its budget for it.
    #[error(
        "this unlock has spent {} of its {} GiB-passes, and it needs {} more",
        gib_passes(*.spent), gib_passes(*.budget), gib_passes(*.work)
    )]
    Budget {
        /// The work spent so far, in bytes · passes.
        spent: u128,
        /// The budget, in bytes · passes.
        budget: u128,
        /// Its work, in bytes · passes.
        work: u128,
    },
    /// The memory could not be read.
    #[error("the free memory could not be read from /proc/meminfo or the cgroup")]
    Unknown,
    /// Its memory cost is more than the machine's memory, which systemd
    /// refuses too.
    #[error("tpm2_argon2id_memcost is {memcost} bytes, more than the {total} of this machine")]
    Physical {
        /// The memory cost, in bytes.
        memcost: u64,
        /// The machine's memory, in bytes.
        total: u64,
    },
    /// The memory it works in and the margin are more than is available.
    #[error(
        "it needs {} of memory and {} are kept for the system, but {} are available",
        size(*.need), size(*.margin), size(*.available)
    )]
    Memory {
        /// The memory Argon2 works in, in bytes.
        need: u64,
        /// The memory kept for the system, in bytes.
        margin: u64,
        /// The memory available, in bytes.
        available: u64,
    },
    /// The derivation failed: its memory could not be reserved.
    #[error("{0}")]
    Failed(KdfError),
}

impl TooCostly {
    /// What the person at the console is told, in one line.
    pub(super) fn told(&self) -> String {
        match *self {
            TooCostly::Work { work } => format!(
                "This volume's TPM2 PIN needs more work to check than ZFSBootMenu allows ({} \
                 GiB-passes). Enrol it again with fixed --tpm2-argon2id-memory= and \
                 --tpm2-argon2id-iterations=.",
                gib_passes(work)
            ),
            TooCostly::Budget { .. } => {
                "ZFSBootMenu has spent its PIN-checking budget for this unlock.".to_owned()
            }
            TooCostly::Unknown => {
                "ZFSBootMenu cannot tell how much memory is free to check this volume's TPM2 PIN."
                    .to_owned()
            }
            TooCostly::Physical { memcost, total } => format!(
                "This volume's TPM2 PIN needs {} of memory to check; this machine has {}.",
                size(memcost),
                size(total)
            ),
            TooCostly::Memory {
                need,
                margin,
                available,
            } => format!(
                "This volume's TPM2 PIN needs {} of memory to check; {} is free.",
                size(need),
                size(available.saturating_sub(margin))
            ),
            TooCostly::Failed(_) => {
                "The memory to check this volume's TPM2 PIN could not be reserved.".to_owned()
            }
        }
    }
}

/// Whether a machine of `memory` has the memory systemd requires of a token
/// whose memory cost is `memcost_bytes`: no more than its own.
///
/// # Errors
///
/// [`TooCostly::Physical`] when it has less.
pub(super) fn check_physical(memcost_bytes: u64, memory: &Memory) -> Result<(), TooCostly> {
    if memcost_bytes > memory.total {
        return Err(TooCostly::Physical {
            memcost: memcost_bytes,
            total: memory.total,
        });
    }
    Ok(())
}

/// Whether a derivation with `params` can run now, on a machine of `memory`
/// (`None` when it could not be read), in a run that spent `spent` of its
/// `budget` (in bytes · passes).
///
/// # Errors
///
/// [`TooCostly`] for the first rule it breaks: the work cap, the run's
/// budget, the memory unknown, the machine's memory, the memory available.
pub(super) fn check_cost(
    params: &Argon2idParams,
    memory: Option<Memory>,
    spent: u128,
    budget: u128,
) -> Result<(), TooCostly> {
    let work = params.work();
    if work > WORK_CAP {
        return Err(TooCostly::Work { work });
    }
    if spent.saturating_add(work) > budget {
        return Err(TooCostly::Budget {
            spent,
            budget,
            work,
        });
    }
    let memory = memory.ok_or(TooCostly::Unknown)?;
    check_physical(params.memcost_bytes, &memory)?;
    let (need, margin) = (params.need(), memory.margin());
    if need.saturating_add(margin) > memory.available {
        return Err(TooCostly::Memory {
            need,
            margin,
            available: memory.available,
        });
    }
    Ok(())
}

/// `bytes` for a person: GiB with one decimal from 1 GiB, whole MiB below.
pub(super) fn size(bytes: u64) -> String {
    if bytes >= GIB {
        let tenths = bytes.saturating_add(GIB / 20) / (GIB / 10);
        format!("{}.{} GiB", tenths / 10, tenths % 10)
    } else {
        format!("{} MiB", bytes.saturating_add(1 << 19) >> 20)
    }
}

/// `work` in whole GiB-passes, rounded up.
fn gib_passes(work: u128) -> u128 {
    work.div_ceil(u128::from(GIB))
}

/// Decodes base64 text as systemd does, skipping the spaces, tabs and line
/// breaks anywhere in it.
fn systemd_base64(text: &str) -> Option<Vec<u8>> {
    let text: String = text.chars().filter(|c| !matches!(c, ' ' | '\t')).collect();
    base64_decode(&text)
}

#[cfg(test)]
mod tests;
