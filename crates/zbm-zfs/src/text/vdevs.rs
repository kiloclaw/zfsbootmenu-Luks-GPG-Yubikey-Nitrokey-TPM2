//! The text that describes pools rather than lists them: `zpool status -p
//! -P`, the listing of `zpool import`, and the configuration that `zdb -e
//! -C` prints.
//!
//! `zpool` writes each pool as keys at the start of a line, such as `
//! pool:`, `status:` or `config:`, padded on the left to line up (in the
//! listing of `zpool import`, by another space for a pool with a comment
//! and, before OpenZFS 2.3, for every pool, but for `status:`), whose text
//! goes on over lines that start with a tab. Under `config:` comes a blank
//! line, in `zpool status` a heading (`NAME STATE READ WRITE CKSUM`), and
//! the tree of vdevs: one per line after a tab, two more spaces for each
//! level below the pool, the name, the state, in `zpool status` the error
//! counts, and a note.
//! The vdevs of other classes follow, each class under its name (`logs`,
//! `cache`, `spares`, `special`, `dedup`) at the pool's level. Spares have
//! no error counts; in the listing of `zpool import`, cache devices and
//! spares have no state either.
//!
//! The words are those of the C locale, which the commands run in. A
//! name with spaces in it, which no device has, is read as its first word.

use zbm_types::{HostId, Untrusted};

use super::TextError;
use crate::model::{
    Health, Importable, PoolHost, PoolStatus, Vdev, VdevClass, VdevErrors, VdevKind,
};
use crate::rows::Readable as _;

/// How many levels of vdevs are kept, the top-level vdevs' included: more
/// than any pool has (a spare being replaced in a mirror is four), few
/// enough that a forged tree cannot nest without end. A deeper row is a
/// sibling of the deepest.
const MAX_LEVELS: usize = 8;

/// The keys and the configuration of one pool.
struct Block<'a> {
    /// The line that starts it, `pool: <name>`, for errors.
    first: &'a str,
    /// Each key and its text, continued lines joined by a space.
    keys: Vec<(&'a str, String)>,
    /// The lines of the configuration, without the first tab.
    config: Vec<&'a str>,
    /// Whether a line of its own says that the pool may be active on
    /// another system, which `zpool import` adds to the `action:` or the
    /// `comment:` of a pool not exported whose data it cannot read.
    active_elsewhere: bool,
}

impl Block<'_> {
    /// The text of `key`.
    fn value(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.as_str())
    }

    /// The text of `key`, untrusted, when it has any.
    fn text(&self, key: &str) -> Option<Untrusted<String>> {
        self.value(key)
            .filter(|value| !value.is_empty())
            .map(|value| Untrusted::new(value.to_owned()))
    }

    /// The first word of `state:`.
    fn state(&self) -> Result<Health, TextError> {
        self.value("state")
            .and_then(|state| state.split_whitespace().next())
            .map(Health::parse)
            .ok_or_else(|| TextError::new(self.first, "no state"))
    }

    /// The pool's vdevs.
    fn vdevs(&self) -> Vec<Vdev> {
        vdev_tree(&self.config)
    }
}

/// The key and the rest of a line that starts with one, such as `
/// state: ONLINE`: lowercase letters after spaces, and a colon.
fn key_line(line: &str) -> Option<(&str, &str)> {
    if line.starts_with('\t') {
        return None;
    }
    let (key, rest) = line.trim_start_matches(' ').split_once(':')?;
    let is_key = !key.is_empty() && key.bytes().all(|byte| byte.is_ascii_lowercase());
    is_key.then_some((key, rest))
}

/// The pools of `text`, each from its `pool:` line to the next.
fn blocks(text: &str) -> Vec<Block<'_>> {
    let mut blocks: Vec<Block<'_>> = Vec::new();
    // In a configuration, and whether a row of it was read, after which a
    // blank line ends it.
    let mut config = false;
    let mut rows = false;
    for line in text.lines() {
        if let Some((key, rest)) = key_line(line) {
            if key == "pool" {
                blocks.push(Block {
                    first: line,
                    keys: Vec::new(),
                    config: Vec::new(),
                    active_elsewhere: false,
                });
            }
            config = key == "config";
            rows = false;
            if let Some(block) = blocks.last_mut() {
                block.keys.push((key, rest.trim().to_owned()));
            }
            continue;
        }
        let Some(block) = blocks.last_mut() else {
            continue;
        };
        if line.trim().is_empty() {
            config &= !rows;
        } else if config {
            if let Some(row) = line.strip_prefix('\t') {
                block.config.push(row);
                rows = true;
            }
        } else if line.starts_with(['\t', ' '])
            && let Some((_, value)) = block.keys.last_mut()
        {
            block.active_elsewhere |= line.trim().starts_with(ACTIVE_ELSEWHERE);
            value.push(' ');
            value.push_str(line.trim());
        }
    }
    blocks
}

/// The vdev of a row below the pool: its name, then the state, the three
/// error counts and a note, each of which may be missing.
fn row_vdev(name: &str, fields: &[&str], class: VdevClass) -> Vdev {
    let state = fields.first().map(|state| Health::parse(state));
    let rest = fields.get(1..).unwrap_or_default();
    let counts: Vec<u64> = rest
        .iter()
        .take(3)
        .map_while(|field| field.parse().ok())
        .collect();
    let (errors, rest) = match counts.as_slice() {
        [read, write, cksum] => (
            Some(VdevErrors {
                read: *read,
                write: *write,
                cksum: *cksum,
            }),
            rest.get(3..).unwrap_or_default(),
        ),
        _ => (None, rest),
    };
    Vdev {
        name: Untrusted::new(name.to_owned()),
        // Known once its children are: see `close`.
        kind: VdevKind::Device,
        class,
        state,
        errors,
        note: (!rest.is_empty()).then(|| Untrusted::new(rest.join(" "))),
        children: Vec::new(),
    }
}

/// Moves the open vdevs at `depth` or deeper into the vdev above each, or
/// into `top`, naming their kind now that their children are known.
fn close(open: &mut Vec<(usize, Vdev)>, top: &mut Vec<Vdev>, depth: usize) {
    while open.last().is_some_and(|(at, _)| *at >= depth)
        && let Some((_, mut vdev)) = open.pop()
    {
        vdev.kind = VdevKind::of_name(vdev.name.get(), !vdev.children.is_empty());
        match open.last_mut() {
            Some((_, parent)) => parent.children.push(vdev),
            None => top.push(vdev),
        }
    }
}

/// The tree of the rows of a configuration, without the pool itself: its
/// top-level vdevs, those of the data first and then each class in the
/// order printed.
fn vdev_tree(rows: &[&str]) -> Vec<Vdev> {
    let mut top = Vec::new();
    let mut open: Vec<(usize, Vdev)> = Vec::new();
    let mut class = VdevClass::Data;
    for row in rows {
        let depth = row.len() - row.trim_start_matches(' ').len();
        let mut fields = row.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        let fields: Vec<&str> = fields.collect();
        if depth == 0 {
            // The pool, the heading, or the name of a class.
            close(&mut open, &mut top, 0);
            if fields.is_empty()
                && let Some(heading) = VdevClass::of_heading(name)
            {
                class = heading;
            }
            continue;
        }
        close(&mut open, &mut top, depth);
        if open.len() >= MAX_LEVELS
            && let Some(&(deepest, _)) = open.last()
        {
            close(&mut open, &mut top, deepest);
        }
        open.push((depth, row_vdev(name, &fields, class)));
    }
    close(&mut open, &mut top, 0);
    top.sort_by_key(|vdev| vdev.class);
    top
}

/// The number of data errors that `errors:` gives: none for `No known
/// data errors`, otherwise the number it starts with.
fn data_errors(text: &str) -> Option<u64> {
    if text == "No known data errors" {
        return Some(0);
    }
    text.split_whitespace().next()?.parse().ok()
}

/// How the line starts that `zpool import` adds to a pool that may be
/// imported elsewhere, below its `action:` or `comment:`.
const ACTIVE_ELSEWHERE: &str = "The pool may be active on another system";

/// Whether the `status:` of a pool not imported says that another system
/// used it last, or has it imported.
fn is_foreign(status: &str) -> bool {
    status.contains("last accessed by another system")
        || status.contains("currently imported by another system")
}

/// `zpool status -p -P`: each imported pool and its vdevs. Empty output,
/// with no pool imported, is no pool.
///
/// # Errors
///
/// [`TextError`] when no pool could be read, for a pool without a valid
/// name or a state.
pub fn status(text: &str) -> Result<Vec<PoolStatus>, TextError> {
    blocks(text)
        .into_iter()
        .map(|block| {
            Ok(PoolStatus {
                name: block
                    .value("pool")
                    .unwrap_or_default()
                    .parse()
                    .map_err(|_| TextError::new(block.first, "invalid pool name"))?,
                state: block.state()?,
                status: block.text("status"),
                action: block.text("action"),
                errors: block.value("errors").and_then(data_errors),
                vdevs: block.vdevs(),
            })
        })
        .readable()
}

/// The listing of `zpool import`: each pool that could be imported, with
/// its vdevs.
///
/// # Errors
///
/// [`TextError`] when no pool could be read, for a pool without a valid
/// name, a numeric identifier or a state.
pub fn importable(text: &str) -> Result<Vec<Importable>, TextError> {
    blocks(text)
        .into_iter()
        .map(|block| {
            let status = block.text("status");
            Ok(Importable {
                name: block
                    .value("pool")
                    .unwrap_or_default()
                    .parse()
                    .map_err(|_| TextError::new(block.first, "invalid pool name"))?,
                id: block
                    .value("id")
                    .and_then(|id| id.parse().ok())
                    .ok_or_else(|| TextError::new(block.first, "no numeric identifier"))?,
                state: block.state()?,
                foreign: block.active_elsewhere
                    || status.as_ref().is_some_and(|text| is_foreign(text.get())),
                status,
                action: block.text("action"),
                vdevs: block.vdevs(),
            })
        })
        .readable()
}

/// The configuration that `zdb -e -C <pool>` prints under `MOS
/// Configuration:`: the `hostid` (a number) and the `hostname` (quoted) at
/// the level of its first key, as `zdb` indents each list below the one it
/// is in. A host ID of 0, or none, is no host ID.
///
/// # Errors
///
/// [`TextError`] for text without that heading, without the pool's
/// `name` at the level of its first key, or with a `hostid` that is not a
/// number.
pub fn pool_config(text: &str) -> Result<PoolHost, TextError> {
    let mut lines = text.lines();
    lines
        .find(|line| line.trim() == "MOS Configuration:")
        .ok_or_else(|| {
            TextError::new(text.lines().next().unwrap_or_default(), "no configuration")
        })?;
    let mut host = PoolHost::default();
    let mut level = None;
    let mut named = false;
    for line in lines {
        let key_value = line.trim_start_matches(' ');
        let Some((key, value)) = key_value.split_once(": ") else {
            continue;
        };
        let indent = line.len() - key_value.len();
        if *level.get_or_insert(indent) != indent {
            continue;
        }
        match key {
            "name" => named = true,
            "hostid" if host.hostid.is_none() => {
                let id: u64 = value
                    .trim()
                    .parse()
                    .map_err(|_| TextError::new(line, "invalid host ID"))?;
                host.hostid = (id != 0)
                    .then(|| HostId::from_hex(&format!("{id:x}")))
                    .flatten();
            }
            "hostname" if host.hostname.is_none() => {
                host.hostname = value
                    .strip_prefix('\'')
                    .and_then(|name| name.strip_suffix('\''))
                    .map(|name| Untrusted::new(name.to_owned()));
            }
            _ => {}
        }
    }
    if !named {
        return Err(TextError::new("MOS Configuration:", "no pool name"));
    }
    Ok(host)
}

#[cfg(test)]
mod tests;
