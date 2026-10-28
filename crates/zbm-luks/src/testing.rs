//! Helpers for the tests of this crate.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::Path;

use zbm_platform::{Installed, KernelLog};

use zbm_platform::interaction::{
    Answer, Asked, Interaction, PromptRequest, SecretRequest, Tell, VolumeRef, Wait, WaitRequest,
};
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::legacy::{Bash, LOG_STUBS};
use zbm_tools::console::ConsoleInteraction;

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

/// The volume the tests unlock, for the requests that name it.
pub const VOLUME: VolumeRef<'static> = VolumeRef {
    name: "luks-test",
    device: "/dev/sda",
};

/// A prompter for code that must not show a timed prompt: secrets, waits
/// and messages go to the fake machine's console, without colours.
pub struct NoPrompts<'a> {
    console: ConsoleInteraction<'a>,
}

impl NoPrompts<'_> {
    /// The prompter of `fake`'s console.
    pub fn new(fake: &FakeSystem) -> NoPrompts<'_> {
        NoPrompts {
            console: ConsoleInteraction::new(fake.system()).colors(false),
        }
    }
}

impl Interaction for NoPrompts<'_> {
    fn ask(&self, request: &PromptRequest) -> Answer {
        panic!("unexpected timed prompt {request:?}");
    }

    fn secret(&self, request: &SecretRequest<'_>) -> Asked {
        self.console.secret(request)
    }

    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        self.console.wait(request)
    }

    fn notify(&self, tell: Tell<'_>) {
        self.console.notify(tell);
    }
}

/// A prompter that records the timed prompts and answers them from a list,
/// with ESCAPE when the list is exhausted, as the unit suites answer them;
/// secrets, waits and messages go to the fake machine's console, without
/// colours.
pub struct Prompts<'a> {
    console: ConsoleInteraction<'a>,
    answers: RefCell<VecDeque<Answer>>,
    shown: RefCell<Vec<PromptRequest>>,
    consoles: Cell<usize>,
}

impl Prompts<'_> {
    /// A prompter of `fake`'s console that answers ESCAPE.
    pub fn new(fake: &FakeSystem) -> Prompts<'_> {
        Prompts::answering(fake, &[])
    }

    /// A prompter of `fake`'s console with these answers.
    pub fn answering<'a>(fake: &'a FakeSystem, answers: &[Answer]) -> Prompts<'a> {
        Prompts {
            console: ConsoleInteraction::new(fake.system()).colors(false),
            answers: RefCell::new(answers.iter().copied().collect()),
            shown: RefCell::new(Vec::new()),
            consoles: Cell::new(0),
        }
    }

    /// How many times a program was given the console.
    pub fn consoles(&self) -> usize {
        self.consoles.get()
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

impl Interaction for Prompts<'_> {
    fn ask(&self, request: &PromptRequest) -> Answer {
        self.shown.borrow_mut().push(request.clone());
        self.answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(Answer::Escape)
    }

    fn secret(&self, request: &SecretRequest<'_>) -> Asked {
        self.console.secret(request)
    }

    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        self.console.wait(request)
    }

    fn notify(&self, tell: Tell<'_>) {
        self.console.notify(tell);
    }

    fn lease_console(&self, run: &mut dyn FnMut(Option<&zbm_platform::System<'_>>)) {
        self.consoles.set(self.consoles.get() + 1);
        run(None);
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
