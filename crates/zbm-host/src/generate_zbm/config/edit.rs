//! `--enable` and `--disable` (ADR 0013): the one value of
//! `Global.ManageImages` changed in the text of `config.yaml`, which keeps
//! its comments, order and layout.

/// The indentation of `line`, in bytes.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// Whether `line` holds nothing: blank, or a comment.
fn is_blank(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// The key of the block mapping entry on `line`, without its indentation,
/// and what follows its `:`.
fn entry(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_end_matches(['\n', '\r']);
    let (key, rest) = line.trim_start_matches(' ').split_once(':')?;
    (rest.is_empty() || rest.starts_with([' ', '\t'])).then_some((key, rest))
}

/// What follows a key's `:` holds a plain scalar and maybe a comment:
/// the comment, with the space before it, when it does.
fn plain_value(rest: &str) -> Option<&str> {
    let value = rest.trim_start();
    let (value, comment) = match value.find([' ', '\t']).filter(|_| value.contains('#')) {
        Some(_) => match value.find(" #").or_else(|| value.find("\t#")) {
            Some(at) => {
                let body = value[..at].trim_end();
                (body, &value[body.len()..])
            }
            None => (value.trim_end(), ""),
        },
        None => (value.trim_end(), ""),
    };
    let is_plain =
        !value.is_empty() && !value.starts_with(['&', '*', '!', '{', '[', '"', '\'', '|', '>']);
    is_plain.then_some(comment)
}

/// Why the file cannot be changed this way, with what to do instead.
fn refuse(why: &str) -> String {
    format!("{why}; set Global.ManageImages in the file by hand")
}

/// `text`, the configuration, with `Global.ManageImages` set to `enable`:
/// its line changed in place, or one added at the start of `Global`, or a
/// `Global` section added at the end.
///
/// # Errors
///
/// When `Global` or its `ManageImages` are not written as block mappings
/// and plain scalars on lines of their own, which this does not change.
pub fn with_manage_images(text: &str, enable: bool) -> Result<String, String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let global = lines
        .iter()
        .position(|line| indent(line) == 0 && entry(line).is_some_and(|(key, _)| key == "Global"));
    let Some(global) = global else {
        let mut out = text.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("Global:\n  ManageImages: {enable}\n"));
        return Ok(out);
    };
    let (_, after) = entry(lines[global]).unwrap_or_default();
    if !is_blank(after) {
        return Err(refuse("Global is not a block mapping"));
    }
    let block_end = lines[global + 1..]
        .iter()
        .position(|line| !is_blank(line) && indent(line) == 0)
        .map_or(lines.len(), |end| global + 1 + end);
    let block = &lines[global + 1..block_end];
    let key_indent = block
        .iter()
        .find(|line| !is_blank(line))
        .map_or(2, |line| indent(line));
    let mut out: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
    let found = block.iter().position(|line| {
        indent(line) == key_indent && entry(line).is_some_and(|(key, _)| key == "ManageImages")
    });
    match found {
        Some(at) => {
            let line = block[at];
            let (_, rest) = entry(line).unwrap_or_default();
            let comment = plain_value(rest)
                .ok_or_else(|| refuse("Global.ManageImages is not a plain value"))?;
            let ending = if line.ends_with('\n') { "\n" } else { "" };
            out[global + 1 + at] = format!(
                "{}ManageImages: {enable}{comment}{ending}",
                " ".repeat(key_indent)
            );
        }
        None => out.insert(
            global + 1,
            format!("{}ManageImages: {enable}\n", " ".repeat(key_indent)),
        ),
    }
    Ok(out.concat())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_value_changes_in_place_and_the_rest_stays() {
        let text = "# My configuration\nGlobal:\n    # Images\n    ManageImages: false  # off\n    \
                    BootMountPoint: /boot/efi\nComponents:\n    Enabled: true\n";
        assert_eq!(
            with_manage_images(text, true).unwrap(),
            "# My configuration\nGlobal:\n    # Images\n    ManageImages: true  # off\n    \
             BootMountPoint: /boot/efi\nComponents:\n    Enabled: true\n"
        );
        let last = "Global:\n  ManageImages: yes";
        assert_eq!(
            with_manage_images(last, false).unwrap(),
            "Global:\n  ManageImages: false"
        );
    }

    #[test]
    fn a_missing_key_or_section_is_added() {
        assert_eq!(
            with_manage_images(
                "Global:\n    DracutConfDir: /d\nEFI:\n  Enabled: true\n",
                true
            )
            .unwrap(),
            "Global:\n    ManageImages: true\n    DracutConfDir: /d\nEFI:\n  Enabled: true\n"
        );
        assert_eq!(
            with_manage_images("Global:\n", true).unwrap(),
            "Global:\n  ManageImages: true\n"
        );
        assert_eq!(
            with_manage_images("EFI:\n  Enabled: true", false).unwrap(),
            "EFI:\n  Enabled: true\nGlobal:\n  ManageImages: false\n"
        );
        assert_eq!(
            with_manage_images("", true).unwrap(),
            "Global:\n  ManageImages: true\n"
        );
        // A nested ManageImages is not Global's.
        assert_eq!(
            with_manage_images("Global:\n  Other:\n    ManageImages: x\n", true).unwrap(),
            "Global:\n  ManageImages: true\n  Other:\n    ManageImages: x\n"
        );
    }

    #[test]
    fn what_is_not_a_plain_line_is_refused() {
        for text in [
            "Global: {ManageImages: true}\n",
            "Global:\n  ManageImages: &yes true\n",
            "Global:\n  ManageImages: \"true\"\n",
            "Global:\n  ManageImages:\n",
        ] {
            let error = with_manage_images(text, false).unwrap_err();
            assert!(
                error.ends_with("set Global.ManageImages in the file by hand"),
                "{text}"
            );
        }
    }
}
