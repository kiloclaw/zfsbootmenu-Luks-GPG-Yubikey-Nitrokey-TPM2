use std::borrow::Cow;

/// A convenient metadata struct that describes a crate
///
/// See [`metadata!`][crate::metadata!]
pub struct Metadata {
    pub(crate) name: Cow<'static, str>,
    pub(crate) version: Cow<'static, str>,
    pub(crate) authors: Option<Cow<'static, str>>,
    pub(crate) homepage: Option<Cow<'static, str>>,
    pub(crate) repository: Option<Cow<'static, str>>,
    pub(crate) support: Option<Cow<'static, str>>,
}

impl Metadata {
    /// See [`metadata!`][crate::metadata!]
    pub fn new(name: impl Into<Cow<'static, str>>, version: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            authors: None,
            homepage: None,
            repository: None,
            support: None,
        }
    }

    /// The list of authors of the crate
    pub fn authors(mut self, value: impl Into<Cow<'static, str>>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            self.authors = value.into();
        }
        self
    }

    /// The URL of the crate's website
    pub fn homepage(mut self, value: impl Into<Cow<'static, str>>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            self.homepage = value.into();
        }
        self
    }

    /// The URL of the crate's repository
    pub fn repository(mut self, value: impl Into<Cow<'static, str>>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            self.repository = value.into();
        }
        self
    }

    /// The support information
    pub fn support(mut self, value: impl Into<Cow<'static, str>>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            self.support = value.into();
        }
        self
    }
}
