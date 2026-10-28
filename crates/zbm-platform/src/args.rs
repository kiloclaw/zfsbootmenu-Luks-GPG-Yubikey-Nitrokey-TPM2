//! A program's command line as text. `std::env::args` panics on an argument
//! that is not valid UTF-8; the programs say so and end instead.

use std::ffi::OsString;

/// The program's name and its arguments from `raw` (the command line,
/// the name first); the name is `fallback` when there is none. An argument
/// that is not valid UTF-8 is an error that names it, as well as it can be
/// shown.
///
/// # Errors
///
/// The message for the first argument that is not valid UTF-8.
pub fn split(
    raw: impl IntoIterator<Item = OsString>,
    fallback: &str,
) -> Result<(String, Vec<String>), String> {
    let mut raw = raw.into_iter();
    let program = raw.next().map_or_else(
        || fallback.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let args = raw
        .map(|arg| {
            arg.into_string().map_err(|arg| {
                format!(
                    "{program}: an argument is not valid UTF-8: {}",
                    arg.to_string_lossy()
                )
            })
        })
        .collect::<Result<Vec<String>, String>>()?;
    Ok((program, args))
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStringExt;

    use super::*;

    #[test]
    fn arguments_that_are_not_utf8_are_refused_by_name() {
        let raw = |args: &[&[u8]]| -> Vec<OsString> {
            args.iter()
                .map(|arg| OsString::from_vec(arg.to_vec()))
                .collect()
        };

        assert_eq!(
            split(raw(&[b"/bin/zbm", b"debug"]), "zbm"),
            Ok(("/bin/zbm".to_owned(), vec!["debug".to_owned()]))
        );
        assert_eq!(split(Vec::new(), "zbm"), Ok(("zbm".to_owned(), Vec::new())));
        assert_eq!(
            split(raw(&[b"generate-zbm", b"-c", b"/etc/\xff.yaml"]), "x"),
            Err("generate-zbm: an argument is not valid UTF-8: /etc/\u{fffd}.yaml".to_owned())
        );
        assert_eq!(
            split(raw(&[b"\xffkcl", b"a"]), "x").map(|(name, _)| name),
            Ok("\u{fffd}kcl".to_owned())
        );
    }
}
