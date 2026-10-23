//! Values written as Python writes them, so that the messages of the checks
//! read as those of the Python harness, and the results of the two can be
//! compared message by message (`zbm-vmtest compare`).

use std::fmt::Write as _;

/// A string as `repr` writes it: in single quotes.
pub fn string(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// A list of strings: `['luks', 'gpg']`.
pub fn list<S: AsRef<str>>(items: &[S]) -> String {
    let items: Vec<String> = items.iter().map(|item| string(item.as_ref())).collect();
    format!("[{}]", items.join(", "))
}

/// A dictionary of strings, in its order: `{'zroot': 'ONLINE'}`.
pub fn dict<K: AsRef<str>, V: AsRef<str>>(items: &[(K, V)]) -> String {
    let mut out = String::from("{");
    for (index, (key, value)) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        write!(out, "{}: {}", string(key.as_ref()), string(value.as_ref())).ok();
    }
    out.push('}');
    out
}

/// An optional value as an f-string writes it: the value, or `None`.
pub fn option(value: Option<&str>) -> String {
    value.unwrap_or("None").to_owned()
}

/// A list of optional strings: `['enabled', None]`.
pub fn option_list(items: &[Option<&str>]) -> String {
    let items: Vec<String> = items
        .iter()
        .map(|item| item.map_or_else(|| "None".to_owned(), string))
        .collect();
    format!("[{}]", items.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn values_read_as_python_writes_them() {
        assert_eq!(list(&["luks", "be-zfskey"]), "['luks', 'be-zfskey']");
        assert_eq!(list::<&str>(&[]), "[]");
        assert_eq!(
            dict(&[("zroot", "ONLINE"), ("tank", "DEGRADED")]),
            "{'zroot': 'ONLINE', 'tank': 'DEGRADED'}"
        );
        assert_eq!(string("it's"), "\"it's\"");
        assert_eq!(string("a'b\"c\n"), "'a\\'b\"c\\n'");
        assert_eq!(option(None), "None");
        assert_eq!(option(Some("ONLINE")), "ONLINE");
        assert_eq!(option_list(&[Some("enabled"), None]), "['enabled', None]");
    }
}
