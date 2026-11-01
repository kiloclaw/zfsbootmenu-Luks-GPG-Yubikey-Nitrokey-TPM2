//! What an image is built with: the builder's settings (`zfsbootmenu_*` of
//! dracut's and mkinitcpio's configuration files, and a few of theirs), as
//! the shim passes them, a line each: `NAME<TAB>VALUE` for a variable,
//! `NAME[]<TAB>VALUE` for each element of an array.

use std::collections::BTreeMap;

/// The builder that installs the plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Builder {
    /// dracut.
    Dracut,
    /// mkinitcpio.
    Mkinitcpio,
}

impl Builder {
    /// Its name, as `ZBM_BUILDSTYLE` has it.
    pub fn name(self) -> &'static str {
        match self {
            Builder::Dracut => "dracut",
            Builder::Mkinitcpio => "mkinitcpio",
        }
    }

    /// The builder named `name`.
    pub fn parse(name: &str) -> Option<Builder> {
        match name {
            "dracut" => Some(Builder::Dracut),
            "mkinitcpio" => Some(Builder::Mkinitcpio),
            _ => None,
        }
    }
}

/// The runtime of the image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Runtime {
    /// The shell runtime.
    Legacy,
    /// The Rust runtime.
    Rust,
}

impl Runtime {
    /// Its name, as `ZBM_RUNTIME` has it.
    pub fn name(self) -> &'static str {
        match self {
            Runtime::Legacy => "legacy",
            Runtime::Rust => "rust",
        }
    }
}

/// A setting: a variable or an array.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Value {
    Scalar(String),
    Array(Vec<String>),
}

/// The settings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Config {
    values: BTreeMap<String, Value>,
}

impl Config {
    /// The settings of `text`, as the shim writes them.
    pub fn parse(text: &str) -> Config {
        let mut config = Config::default();
        for line in text.lines() {
            let Some((name, value)) = line.split_once('\t') else {
                continue;
            };
            if let Some(name) = name.strip_suffix("[]") {
                match config
                    .values
                    .entry(name.to_owned())
                    .or_insert(Value::Array(Vec::new()))
                {
                    Value::Array(values) => values.push(value.to_owned()),
                    scalar @ Value::Scalar(_) => *scalar = Value::Array(vec![value.to_owned()]),
                }
            } else {
                config
                    .values
                    .insert(name.to_owned(), Value::Scalar(value.to_owned()));
            }
        }
        config
    }

    /// Sets `name` to `value`, for tests and defaults.
    #[must_use]
    pub fn with(mut self, name: &str, value: &str) -> Config {
        self.values
            .insert(name.to_owned(), Value::Scalar(value.to_owned()));
        self
    }

    /// The variable `name`, or the first element of an array; `None` when it
    /// is empty or unset, as the shell's `[ -n ]` tells.
    pub fn get(&self, name: &str) -> Option<&str> {
        match self.values.get(name)? {
            Value::Scalar(value) => Some(value.as_str()).filter(|value| !value.is_empty()),
            Value::Array(values) => values.first().map(String::as_str),
        }
    }

    /// `name`, or `default` when it is unset or empty.
    pub fn or<'a>(&'a self, name: &str, default: &'a str) -> &'a str {
        self.get(name).unwrap_or(default)
    }

    /// The words of `name`: an array's elements, a variable's words.
    pub fn words(&self, name: &str) -> Vec<String> {
        match self.values.get(name) {
            Some(Value::Array(values)) => values.clone(),
            Some(Value::Scalar(value)) => value.split_whitespace().map(str::to_owned).collect(),
            None => Vec::new(),
        }
    }

    /// Whether `name` is on: anything but `no`, `n`, `off`, `0` and `false`,
    /// `default` when unset (`zbm_opt_enabled`).
    pub fn is_on(&self, name: &str, default: bool) -> bool {
        match self.get(name) {
            None => default,
            Some(value) => !matches!(
                value.to_ascii_lowercase().as_str(),
                "no" | "n" | "off" | "0" | "false"
            ),
        }
    }

    /// Whether `name` says yes: `yes`, `y`, `on` or `1`, as the release
    /// build and tracing switches read.
    pub fn is_yes(&self, name: &str) -> bool {
        self.get(name).is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "yes" | "y" | "on" | "1"
            )
        })
    }

    /// The runtime asked for.
    ///
    /// # Errors
    ///
    /// For a runtime that is not one.
    pub fn runtime(&self) -> Result<Runtime, String> {
        let runtime = self.or("zfsbootmenu_runtime", "legacy");
        match runtime.to_ascii_lowercase().as_str() {
            "legacy" => Ok(Runtime::Legacy),
            "rust" => Ok(Runtime::Rust),
            _ => Err(format!(
                "zfsbootmenu_runtime={runtime}: unknown runtime; \
                 use zfsbootmenu_runtime=legacy or rust"
            )),
        }
    }

    /// Whether this is a release build, which keeps everything of the build
    /// host's out.
    pub fn is_release(&self) -> bool {
        self.is_yes("zfsbootmenu_release_build")
    }

    /// The directory of ZFSBootMenu's files on the build host.
    pub fn module_root(&self) -> &str {
        self.or("zfsbootmenu_module_root", "/usr/share/zfsbootmenu")
    }

    /// The directory of the user's hooks on the build host.
    pub fn hook_root(&self) -> &str {
        self.or("zfsbootmenu_hook_root", "/etc/zfsbootmenu/hooks")
    }

    /// The zbm program of the build host.
    pub fn zbm(&self) -> &str {
        self.or("zfsbootmenu_zbm", "/usr/libexec/zfsbootmenu/zbm")
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn variables_and_arrays_are_read_as_the_shim_writes_them() {
        let config = Config::parse(
            "zfsbootmenu_runtime\tRust\n\
             zfsbootmenu_teardown[]\t/a b/hook\n\
             zfsbootmenu_teardown[]\t/c\n\
             zfsbootmenu_setup\t/x /y\n\
             zfsbootmenu_luks\toff\n\
             zfsbootmenu_release_build\tYes\n\
             zfsbootmenu_empty\t\n\
             no tab here\n",
        );

        assert_eq!(config.runtime(), Ok(Runtime::Rust));
        assert_eq!(config.words("zfsbootmenu_teardown"), ["/a b/hook", "/c"]);
        assert_eq!(config.words("zfsbootmenu_setup"), ["/x", "/y"]);
        assert_eq!(config.get("zfsbootmenu_teardown"), Some("/a b/hook"));
        assert_eq!(config.get("zfsbootmenu_empty"), None);
        assert!(config.words("none").is_empty());
        assert!(!config.is_on("zfsbootmenu_luks", true));
        assert!(config.is_on("zfsbootmenu_luks_gpg", true));
        assert!(!config.is_on("zfsbootmenu_luks_pcscd", false));
        assert!(config.is_release());
        assert_eq!(config.module_root(), "/usr/share/zfsbootmenu");
        assert_eq!(config.hook_root(), "/etc/zfsbootmenu/hooks");
        assert_eq!(config.zbm(), "/usr/libexec/zfsbootmenu/zbm");
        let wrong = Config::default().with("zfsbootmenu_runtime", "perl");
        assert!(wrong.runtime().unwrap_err().contains("unknown runtime"));
        assert_eq!(Config::default().runtime(), Ok(Runtime::Legacy));
        assert_eq!(Builder::parse("dracut"), Some(Builder::Dracut));
        assert_eq!(
            Builder::parse("mkinitcpio").map(Builder::name),
            Some("mkinitcpio")
        );
        assert_eq!(Builder::parse("make"), None);
        assert_eq!(Runtime::Rust.name(), "rust");
        // An array overrides a variable of its name.
        let both = Config::parse("a\tx\na[]\ty\n");
        assert_eq!(both.words("a"), ["y"]);
    }
}
