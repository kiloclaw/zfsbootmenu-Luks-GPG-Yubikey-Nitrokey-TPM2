//! Where the value of each option came from, for `zbm debug config`.

use crate::spec::OptionSpec;

/// Where the value of an option came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Origin {
    /// The option was not given: its default applies.
    Default,
    /// The kernel command line, under `name` (the option's name or an
    /// alias).
    CommandLine {
        /// The name it was given under.
        name: &'static str,
    },
    /// The kernel command line gave `value` under `name`, which is not
    /// valid: the default applies.
    Invalid {
        /// The name it was given under.
        name: &'static str,
        /// The value given.
        value: String,
    },
}

/// The origins of the options a parser read, in the order it read them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Origins {
    entries: Vec<(&'static OptionSpec, Origin)>,
}

impl Origins {
    /// Records the origin of `spec`, replacing an earlier one.
    pub fn record(&mut self, spec: &'static OptionSpec, origin: Origin) {
        match self
            .entries
            .iter_mut()
            .find(|(known, _)| std::ptr::eq(*known, spec))
        {
            Some((_, known)) => *known = origin,
            None => self.entries.push((spec, origin)),
        }
    }

    /// The origin of `spec`, if the parser read it.
    pub fn of(&self, spec: &OptionSpec) -> Option<&Origin> {
        self.entries
            .iter()
            .find(|(known, _)| std::ptr::eq(*known, spec))
            .map(|(_, origin)| origin)
    }

    /// Every option read, with its origin.
    pub fn iter(&self) -> impl Iterator<Item = (&'static OptionSpec, &Origin)> {
        self.entries.iter().map(|(spec, origin)| (*spec, origin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::cmdline::{SORT_KEY, TIMEOUT};

    #[test]
    fn a_later_origin_replaces_an_earlier_one() {
        let mut origins = Origins::default();
        origins.record(&TIMEOUT, Origin::CommandLine { name: "timeout" });
        origins.record(&SORT_KEY, Origin::Default);
        origins.record(
            &TIMEOUT,
            Origin::Invalid {
                name: "timeout",
                value: "x".to_owned(),
            },
        );
        assert!(matches!(origins.of(&TIMEOUT), Some(Origin::Invalid { .. })));
        assert_eq!(origins.of(&SORT_KEY), Some(&Origin::Default));
        assert_eq!(origins.iter().count(), 2);
        assert_eq!(origins.of(&crate::spec::cmdline::SKIP), None);
    }
}
