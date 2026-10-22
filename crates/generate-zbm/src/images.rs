//! Naming and pruning image files, the counterparts of `maxRevision`,
//! `groupKernels`, `pruneVersions` and `purgeFiles` in `generate-zbm`.
//!
//! Images are named `<prefix>-<version>_<revision>` (with a suffix such as
//! `.EFI` for EFI bundles). When `Versions` is set, each build adds a revision
//! and old ones are pruned; the logic here decides which files to remove, in
//! order, and the caller removes them.

use std::collections::BTreeMap;

use crate::version::{sort_versions, versioncmp};

/// The highest revision among `files` that match `_<digits><suffix>` at the
/// end, or 0 when none do.
pub fn max_revision(files: &[String], suffix: &str) -> u64 {
    files
        .iter()
        .filter_map(|file| revision_of(file, suffix))
        .max()
        .unwrap_or(0)
}

/// The revision at the end of `file` (`_<digits><suffix>`), if it has one.
fn revision_of(file: &str, suffix: &str) -> Option<u64> {
    let body = file.strip_suffix(suffix)?;
    let digits = body.rsplit_once('_').map(|(_, digits)| digits)?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // Perl compares the captured digits numerically; an overflowing value cannot
    // occur for real revisions.
    digits.parse().ok()
}

/// Groups `files` matching `<prefix>-<version>_<digits><suffix>` by their
/// version, as `groupKernels` does.
///
/// The version is the greedy match of `.+` between the prefix and the final
/// `_<digits>`, so a version may itself contain `_<digits>`.
pub fn group_versions(
    files: &[String],
    prefix: &str,
    suffix: &str,
) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files {
        if let Some(version) = version_of(file, prefix, suffix) {
            groups.entry(version).or_default().push(file.clone());
        }
    }
    groups
}

/// The version part of `file` named `<prefix>-<version>_<digits><suffix>`.
fn version_of(file: &str, prefix: &str, suffix: &str) -> Option<String> {
    let body = file.strip_prefix(prefix)?.strip_suffix(suffix)?;
    let after_dash = body.strip_prefix('-')?;
    // Greedy `(.+)_\d+`: the last `_` that is followed only by digits, with at
    // least one character before it.
    let mut split = None;
    let bytes = after_dash.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'_'
            && index > 0
            && index + 1 < bytes.len()
            && bytes[index + 1..].iter().all(u8::is_ascii_digit)
        {
            split = Some(index);
        }
    }
    split.map(|index| after_dash[..index].to_owned())
}

/// Perl's `$keep gt 0`: a string comparison, so any non-empty keep that is not
/// `"0"` and does not sort before `"0"` counts as "keep some". For the
/// non-negative integers used here, this is "keep is at least 1".
fn keep_some(keep: u64) -> bool {
    keep > 0
}

/// The files `purgeFiles(files, keep)` removes, in order: when keeping some,
/// the oldest by version until `keep` remain; otherwise all of them, in their
/// given order.
fn purge(files: &[String], keep: u64) -> Vec<String> {
    if keep_some(keep) {
        let mut sorted = files.to_vec();
        sort_versions(&mut sorted);
        let remove = sorted
            .len()
            .saturating_sub(usize::try_from(keep).unwrap_or(usize::MAX));
        sorted.truncate(remove);
        sorted
    } else {
        files.to_vec()
    }
}

/// The files `pruneVersions(groups, current, keep)` removes, in the order it
/// removes them.
///
/// The current version keeps its newest `keep` revisions. Older versions are
/// removed whole while more than `keep` versions remain, then the rest keep
/// their single newest revision.
pub fn prune_versions(
    groups: &BTreeMap<String, Vec<String>>,
    current: &str,
    keep: u64,
) -> Vec<String> {
    let mut removed = Vec::new();
    if let Some(files) = groups.get(current) {
        removed.extend(purge(files, keep));
    }

    let mut others: Vec<&String> = groups.keys().filter(|key| *key != current).collect();
    others.sort_by(|a, b| versioncmp(a, b));

    let keep = usize::try_from(keep).unwrap_or(usize::MAX);
    let mut index = 0;
    while others.len() - index > keep {
        if let Some(files) = groups.get(others[index]) {
            removed.extend(purge(files, 0));
        }
        index += 1;
    }
    for version in &others[index..] {
        if let Some(files) = groups.get(*version) {
            removed.extend(purge(files, 1));
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::legacy::{path, perl};
    use zbm_testkit::{Rng, have_program, parity};

    fn files(list: &[&str]) -> Vec<String> {
        list.iter().map(|f| (*f).to_owned()).collect()
    }

    #[test]
    fn revisions_and_groups() {
        let efi = files(&[
            "/b/zfsbootmenu-3.1.0_1.EFI",
            "/b/zfsbootmenu-3.1.0_2.EFI",
            "/b/zfsbootmenu-3.0.0_1.EFI",
            "/b/zfsbootmenu-backup.EFI",
        ]);
        assert_eq!(max_revision(&efi, ".EFI"), 2);
        let groups = group_versions(&efi, "/b/zfsbootmenu", ".EFI");
        assert_eq!(groups["3.1.0"].len(), 2);
        assert_eq!(groups["3.0.0"].len(), 1);
        assert!(!groups.contains_key("backup"));
        // A version may contain _<digits>.
        let odd = files(&["/b/p-1.2_3_4.EFI"]);
        assert_eq!(group_versions(&odd, "/b/p", ".EFI")["1.2_3"], odd);
        assert_eq!(max_revision(&odd, ".EFI"), 4);
        assert_eq!(max_revision(&files(&["/b/p-1.0.EFI"]), ".EFI"), 0);
    }

    #[test]
    fn pruning_keeps_the_newest() {
        let groups = group_versions(
            &files(&[
                "p-1_1", "p-1_2", "p-1_3", "p-2_1", "p-3_1", "p-3_2", "p-4_1",
            ]),
            "p",
            "",
        );
        // The current version 3 has 2 revisions and keeps 2, so none of it goes. Of the
        // other versions (1, 2, 4), the oldest are removed whole while more than 2
        // remain, so all of 1 goes; 2 and 4 keep their one revision.
        let removed = prune_versions(&groups, "3", 2);
        assert_eq!(removed, ["p-1_1", "p-1_2", "p-1_3"]);
        // With keep 1, the current version keeps its newest revision only.
        let removed = prune_versions(&groups, "3", 1);
        assert_eq!(removed[0], "p-3_1");
    }

    /// A Perl harness with the real `maxRevision`, `groupKernels`, `pruneVersions`
    /// and `purgeFiles` subs, which reports the files `pruneVersions` would remove,
    /// one per line.
    fn perl_harness() -> String {
        let source = std::fs::read_to_string(path("bin/generate-zbm")).unwrap();
        let extract = |name: &str| {
            let start = source.find(&format!("\nsub {name} ")).unwrap() + 1;
            let end = source[start..].find("\n}\n").unwrap() + start + 3;
            source[start..end].to_owned()
        };
        format!(
            "use strict; use warnings; use Sort::Versions;\n\
             our @removed; sub verboseUnlink {{ push @removed, $_[0]; }} sub Log {{ }}\n\
             {}{}{}{}\n\
             my ($current, $keep, @spec) = @ARGV;\n\
             my %groups;\n\
             for my $entry (@spec) {{ my ($v, @f) = split /,/, $entry; $groups{{$v}} = [@f]; }}\n\
             pruneVersions(\\%groups, $current, $keep);\n\
             print \"$_\\n\" for @removed;\n",
            extract("maxRevision"),
            extract("groupKernels"),
            extract("pruneVersions"),
            extract("purgeFiles"),
        )
    }

    #[test]
    fn pruning_matches_generate_zbm() {
        let mut parity = parity!("pruning_matches_generate_zbm");
        if parity.mode().needs_legacy() && !have_program("perl") {
            eprintln!("skipped: perl is missing");
            return;
        }
        let versions = ["1.0", "1.10", "2.0", "2.1", "3.0.0+dev", "3.0.0"];
        let mut rng = Rng::from_env(7);
        for _ in 0..200 {
            // Build groups: each version has a random set of revisions.
            let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for version in &versions {
                if rng.below(3) == 0 {
                    continue;
                }
                let revisions = 1 + rng.below(4);
                let files: Vec<String> = (1..=revisions)
                    .map(|revision| format!("p-{version}_{revision}"))
                    .collect();
                groups.insert((*version).to_owned(), files);
            }
            if groups.is_empty() {
                continue;
            }
            let current = *rng.choose(&versions);
            let keep = rng.below(4) as u64;

            let spec: Vec<String> = groups
                .iter()
                .map(|(version, files)| format!("{version},{}", files.join(",")))
                .collect();
            let mut args = vec![current.to_owned(), keep.to_string()];
            args.extend(spec);
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let expected: Vec<String> = parity.legacy(format!("{args:?}"), || {
                let output = perl(&perl_harness(), &arg_refs, b"");
                assert!(output.status.success(), "{}", output.stderr_text());
                output.stdout_text().lines().map(str::to_owned).collect()
            });
            let removed = prune_versions(&groups, current, keep);
            assert_eq!(
                removed, expected,
                "current {current}, keep {keep}, groups {groups:?}"
            );
        }
    }
}
