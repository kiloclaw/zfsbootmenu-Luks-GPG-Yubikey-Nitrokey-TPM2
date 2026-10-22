//! Lines of `zpool` output, split as the legacy loops read them.

/// The lines `while read -r line ... done <<< "$( command )"` reads: spaces
/// and tabs are trimmed.
pub fn here_lines(output: &str) -> Vec<String> {
    output
        .trim_end_matches('\n')
        .split('\n')
        .map(|line| line.trim_matches([' ', '\t']).to_owned())
        .collect()
}

/// The lines `while IFS=$'\t' read -r ...; done <<< "$( command )"` reads:
/// only tabs are trimmed, and NUL bytes are dropped.
pub fn tab_lines(output: &str) -> Vec<String> {
    let text = format!("{}\n", output.trim_end_matches('\n'));
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.replace('\0', ""))
        .collect();
    lines.pop();
    lines
}

/// The two fields `IFS=$'\t' read -r first rest` reads from a line.
pub fn tab_fields(line: &str) -> (&str, &str) {
    let line = line.trim_matches('\t');
    match line.split_once('\t') {
        Some((first, rest)) => (first, rest.trim_start_matches('\t')),
        None => (line, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn lines_and_fields_are_read_as_bash_reads_them() {
        assert_eq!(here_lines(" a \n\tb\n\n"), ["a", "b"]);
        assert_eq!(here_lines(""), [""]);
        assert_eq!(tab_lines("a\tb\n\0c\n"), ["a\tb", "c"]);
        assert_eq!(tab_lines(""), [""]);
        assert_eq!(tab_fields("\ttank\t\tDEGRADED\t"), ("tank", "DEGRADED"));
        assert_eq!(tab_fields(" tank "), (" tank ", ""));
    }
}
