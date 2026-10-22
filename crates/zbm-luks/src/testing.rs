//! Helpers for the tests of this crate.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;

use zbm_platform::{Installed, KernelLog};

use crate::unlocker::{Answer, PromptRequest, Prompter};
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::legacy::{Bash, LOG_STUBS};

/// The legacy LUKS library, loaded as the unit suites load it, with messages
/// on standard error as `LEVEL: text`.
pub fn legacy_luks() -> Bash {
    Bash::new()
        .code(LOG_STUBS)
        .code("_ZFSBOOTMENU_CORE=1")
        .code(r#"LUKS_KEYS_LIB="$LEGACY/zfsbootmenu/lib/zfsbootmenu-luks-keys.sh""#)
        .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
        .source("zfsbootmenu/lib/zfsbootmenu-luks.sh")
}

/// Sends this thread's kernel log, at every level, to the fake machine's
/// `/dev/kmsg` while the returned guard lives.
pub fn log(fake: &FakeSystem) -> Installed {
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
}

/// The text of a legacy function from a library in `legacy/`, to run it with
/// changes.
pub fn legacy_function(library: &str, name: &str) -> String {
    let text = std::fs::read_to_string(zbm_testkit::legacy::path(library)).unwrap();
    let start = text.find(&format!("\n{name}() {{\n")).unwrap() + 1;
    let end = start + text[start..].find("\n}\n").unwrap() + 3;
    text[start..end].to_owned()
}

/// A prompter for code that must not show a timed prompt.
pub struct NoPrompts;

impl Prompter for NoPrompts {
    fn timed_prompt(&self, request: &PromptRequest) -> Answer {
        panic!("unexpected timed prompt {request:?}");
    }
}

/// A prompter that records the prompts and answers them from a list, with
/// ESCAPE when the list is exhausted, as the unit suites answer them.
#[derive(Default)]
pub struct Prompts {
    answers: RefCell<VecDeque<Answer>>,
    shown: RefCell<Vec<PromptRequest>>,
}

impl Prompts {
    /// A prompter with these answers.
    pub fn answering(answers: &[Answer]) -> Prompts {
        Prompts {
            answers: RefCell::new(answers.iter().copied().collect()),
            shown: RefCell::new(Vec::new()),
        }
    }

    /// The messages of the prompts shown.
    pub fn messages(&self) -> Vec<String> {
        self.shown
            .borrow()
            .iter()
            .map(|request| request.message.clone())
            .collect()
    }
}

impl Prompter for Prompts {
    fn timed_prompt(&self, request: &PromptRequest) -> Answer {
        self.shown.borrow_mut().push(request.clone());
        self.answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(Answer::Escape)
    }
}

/// The warnings in the fake machine's kernel log.
pub fn warnings(fake: &FakeSystem) -> Vec<String> {
    fake.kmsg()
        .lines()
        .filter_map(|line| line.strip_prefix("<4>ZFSBootMenu: "))
        .map(str::to_owned)
        .collect()
}
