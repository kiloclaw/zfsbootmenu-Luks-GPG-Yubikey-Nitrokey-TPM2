//! A program the generator runs: its arguments and the variables it adds to
//! its environment, run without a shell (ADR 0013), and the line the debug
//! log shows for it, as a shell would read it.

use zbm_platform::shell::{WordsError, quote, split_words};

/// A program to run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Invocation {
    /// Variables added to its environment.
    env: Vec<(String, String)>,
    /// The program, then its arguments.
    args: Vec<String>,
    /// The words of the line the log shows.
    shown: Vec<String>,
}

impl Invocation {
    /// The program `program`, shown as it is.
    pub fn new(program: &str) -> Invocation {
        Invocation::default().plain(program)
    }

    /// Adds the argument `word`, shown as it is: an option or a name.
    #[must_use]
    pub fn plain(mut self, word: &str) -> Invocation {
        self.args.push(word.to_owned());
        self.shown.push(word.to_owned());
        self
    }

    /// Adds the argument `value`, shown in quotes: a path or a value.
    #[must_use]
    pub fn quoted(mut self, value: &str) -> Invocation {
        self.args.push(value.to_owned());
        self.shown.push(quote(value));
        self
    }

    /// Adds the argument `NAME=VALUE`, shown with the value in quotes.
    #[must_use]
    pub fn assignment(mut self, name: &str, value: &str) -> Invocation {
        self.args.push(format!("{name}={value}"));
        self.shown.push(format!("{name}={}", quote(value)));
        self
    }

    /// Adds `NAME=VALUE` to the program's environment, shown before it.
    #[must_use]
    pub fn env(mut self, name: &str, value: &str) -> Invocation {
        self.env.push((name.to_owned(), value.to_owned()));
        self.shown
            .insert(self.env.len() - 1, format!("{name}={}", quote(value)));
        self
    }

    /// Adds the shell words of `text`, from the configuration, shown as
    /// written.
    ///
    /// # Errors
    ///
    /// When `text` is not plain words ([`split_words`]).
    pub fn words(mut self, text: &str) -> Result<Invocation, WordsError> {
        self.args.extend(split_words(text)?);
        self.shown.push(text.to_owned());
        Ok(self)
    }

    /// The program and its arguments.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// The variables added to its environment.
    pub fn environment(&self) -> &[(String, String)] {
        &self.env
    }

    /// The line the log shows.
    pub fn line(&self) -> String {
        self.shown.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn programs_run_as_written_and_show_as_a_shell_reads_them() {
        let invocation = Invocation::new("dracut")
            .plain("-f")
            .quoted("/tmp/a b")
            .assignment(".osrel", "/etc/os-release")
            .env("zfsbootmenu_console_font", "terminus")
            .env("B", "it's")
            .words("--add 'zfs crypt'")
            .unwrap();

        assert_eq!(
            invocation.args(),
            [
                "dracut",
                "-f",
                "/tmp/a b",
                ".osrel=/etc/os-release",
                "--add",
                "zfs crypt"
            ]
        );
        assert_eq!(
            invocation.environment(),
            [
                ("zfsbootmenu_console_font".to_owned(), "terminus".to_owned()),
                ("B".to_owned(), "it's".to_owned()),
            ]
        );
        assert_eq!(
            invocation.line(),
            concat!(
                r"zfsbootmenu_console_font='terminus' B='it'\''s' dracut -f '/tmp/a b' ",
                ".osrel='/etc/os-release' --add 'zfs crypt'"
            )
        );
        assert_eq!(
            Invocation::new("x").words("$HOME"),
            Err(WordsError::Expansion('$'))
        );
    }
}
