//! A case: its state directory, environment, console and timed prompts,
//! and the sessions of the LUKS code that run in it.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicU32;
use std::time::{Duration, Instant};

use zbm_luks::device::OpenCheck;
use zbm_luks::unlocker::{Luks, Settings, Unlocker};
use zbm_platform::console::{Console, Size, Timed};
use zbm_platform::env::Environment;
use zbm_platform::files::{Files, RealFiles};
use zbm_platform::interaction::{
    Answer, Asked, Interaction, PromptRequest, SecretRequest, Tell, Wait, WaitRequest,
};
use zbm_platform::{KernelLog, Secret, System, SystemClock, SystemProcesses};
use zbm_tools::console::ConsoleInteraction;

use super::{DeviceRunner, Dump, Lab, record_name};

/// The environment of the LUKS code in a case, which several sessions of the
/// case may read at once.
#[derive(Default)]
pub struct CaseEnv {
    vars: Mutex<BTreeMap<String, OsString>>,
}

impl CaseEnv {
    /// Sets a variable.
    pub fn set(&self, name: &str, value: impl Into<OsString>) {
        self.vars
            .lock()
            .unwrap()
            .insert(name.to_owned(), value.into());
    }
}

impl Environment for CaseEnv {
    fn var_os(&self, name: &str) -> Option<OsString> {
        self.vars.lock().unwrap().get(name).cloned()
    }

    fn vars_os(&self) -> Vec<(OsString, OsString)> {
        self.vars
            .lock()
            .unwrap()
            .iter()
            .map(|(name, value)| (OsString::from(name), value.clone()))
            .collect()
    }
}

/// What the console holds for the code under test, as the standard input of
/// a Bash case does: lines that are there from the start (`<<<`), that
/// arrive later (`< <(sleep N; echo ...)`), and when the input ends.
pub struct LinesConsole {
    lines: Mutex<VecDeque<String>>,
    ready: Instant,
    end: Instant,
    output: Mutex<Vec<u8>>,
}

impl LinesConsole {
    /// `lines`, available after `delay`, then the end of the input after
    /// `end` (counted from now, and at least `delay`).
    pub fn new(lines: &[&str], delay: Duration, end: Duration) -> LinesConsole {
        let now = Instant::now();
        LinesConsole {
            lines: Mutex::new(lines.iter().map(|line| (*line).to_owned()).collect()),
            ready: now + delay,
            end: now + end.max(delay),
            output: Mutex::new(Vec::new()),
        }
    }

    /// What was written to the console.
    pub fn output(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    fn next(&self) -> Option<String> {
        let now = Instant::now();
        if now < self.ready {
            std::thread::sleep(self.ready - now);
        }
        if let Some(line) = self.lines.lock().unwrap().pop_front() {
            return Some(line);
        }
        let now = Instant::now();
        if now < self.end {
            std::thread::sleep(self.end - now);
        }
        None
    }
}

impl Console for LinesConsole {
    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        self.output.lock().unwrap().extend_from_slice(bytes);
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(Size {
            rows: 25,
            columns: 80,
        })
    }

    fn read_key(&self, _timeout: Option<Duration>) -> io::Result<Option<u8>> {
        Ok(None)
    }

    fn read_secret(&self) -> io::Result<Option<Secret>> {
        Ok(self.next().map(|line| Secret::copy_from(line.as_bytes())))
    }

    fn read_line(&self) -> io::Result<Option<String>> {
        Ok(self.next())
    }

    fn set_echo(&self, _on: bool) {}

    fn read_secret_within(&self, timeout: Duration) -> io::Result<Timed<Secret>> {
        let now = Instant::now();
        let waiting = !self.lines.lock().unwrap().is_empty();
        let arrives = if waiting { self.ready } else { self.end };
        if arrives > now + timeout {
            std::thread::sleep(timeout);
            return Ok(Timed::TimedOut);
        }
        Ok(match self.next() {
            Some(line) => Timed::Read(Secret::copy_from(line.as_bytes())),
            None => Timed::Ended,
        })
    }
}

/// Timed prompts recorded and answered as the seams of `impl.sh` do: one
/// status per line of the answers file, 0 for RETURN, ESCAPE when there is
/// none, after `delay`; secrets, waits and messages go to the console.
struct Recorded<'a> {
    console: ConsoleInteraction<'a>,
    prompts: PathBuf,
    answers: PathBuf,
    delay: Duration,
}

impl Interaction for Recorded<'_> {
    fn ask(&self, request: &PromptRequest) -> Answer {
        let mut text = std::fs::read_to_string(&self.prompts).unwrap_or_default();
        text.push_str(&format!("timed_prompt {}\n", request.arguments().join(" ")));
        let _ = std::fs::write(&self.prompts, text);
        std::thread::sleep(self.delay);
        let answers = std::fs::read_to_string(&self.answers).unwrap_or_default();
        let (first, rest) = answers.split_once('\n').unwrap_or((&answers, ""));
        let _ = std::fs::write(&self.answers, rest);
        if first == "0" {
            Answer::Continue
        } else {
            Answer::Escape
        }
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

/// A case: a fresh state directory, crypttab, command line and device
/// runner, in which the LUKS code runs one or more times.
pub struct Case<'l> {
    /// The lab, which outlives its cases and their loop devices.
    _lab: &'l Lab,
    /// The case's directory (`$FAKE`).
    pub fake: PathBuf,
    /// The state directory (`$BASE`).
    pub base: PathBuf,
    /// The device-mapper directory.
    pub mapper: PathBuf,
    /// The runner, shared by the sessions of the case.
    pub runner: DeviceRunner,
    /// The environment of the LUKS code.
    pub env: CaseEnv,
    /// Seconds each timed prompt waits.
    pub prompt_delay: Duration,
    console_output: Mutex<String>,
}

impl<'l> Case<'l> {
    pub(super) fn new(lab: &'l Lab, fake: &Path) -> Case<'l> {
        let _ = std::fs::remove_dir_all(fake);
        for dir in [
            "holders",
            "maps",
            "content",
            "pools",
            "busy",
            "mapper",
            "base/cmdline.d",
        ] {
            std::fs::create_dir_all(fake.join(dir)).unwrap();
        }
        let base = fake.join("base");
        let mapper = fake.join("mapper");
        let env = CaseEnv::default();
        env.set("BASE", base.as_os_str());
        env.set("ZBM_CRYPTTAB", fake.join("crypttab").as_os_str());
        if !lab.tcti.is_empty() {
            env.set("LUKS_TPM2_TCTI", lab.tcti.as_str());
        }
        env.set(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );
        let runner = DeviceRunner {
            fake: fake.to_owned(),
            mapper: mapper.clone(),
            env: lab.tpm_env(),
            fido: lab.fido.clone(),
            calls: Mutex::new(Vec::new()),
            minor: AtomicU32::new(100),
            fido_needs_pin: Mutex::new(false),
            dump: Mutex::new(Dump::Plain),
            key_stubs: !lab.tcti.is_empty(),
        };
        Case {
            _lab: lab,
            fake: fake.to_owned(),
            base,
            mapper,
            runner,
            env,
            prompt_delay: Duration::ZERO,
            console_output: Mutex::new(String::new()),
        }
    }

    /// Writes the crypttab, one line per entry.
    pub fn crypttab(&self, lines: &[&str]) {
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(self.fake.join("crypttab"), text).unwrap();
    }

    /// Writes the crypttab as it is, without adding newlines.
    pub fn crypttab_raw(&self, text: &str) {
        std::fs::write(self.fake.join("crypttab"), text).unwrap();
    }

    /// Removes the crypttab.
    pub fn no_crypttab(&self) {
        let _ = std::fs::remove_file(self.fake.join("crypttab"));
    }

    /// Sets the ZFSBootMenu command line option `option` to `value`.
    pub fn cmdline(&self, option: &str, value: &str) {
        std::fs::write(self.base.join("cmdline.d").join(option), value).unwrap();
    }

    /// Gives the timed prompts their answers, one status per line.
    pub fn answers(&self, answers: &str) {
        std::fs::write(self.fake.join("answers"), answers).unwrap();
    }

    /// Says what `device` holds once it is open, for `blkid`.
    pub fn content(&self, device: &str, kind: &str) {
        std::fs::write(
            self.fake.join("content").join(record_name(device)),
            format!("{kind}\n"),
        )
        .unwrap();
    }

    /// A pool whose vdevs are `vdevs`, for `zpool`.
    pub fn pool(&self, name: &str, vdevs: &[&str]) {
        let text: String = vdevs.iter().map(|vdev| format!("{vdev}\n")).collect();
        std::fs::write(self.fake.join("pools").join(name), text).unwrap();
    }

    /// Makes the mapping `name` busy, so that closing it fails.
    pub fn busy(&self, name: &str) {
        std::fs::write(self.fake.join("busy").join(name), "").unwrap();
    }

    /// Runs `body` with a session on a console holding `console`.
    pub fn with_session<T>(
        &self,
        console: &LinesConsole,
        body: impl FnOnce(&mut Unlocker<'_>, &System<'_>) -> T,
    ) -> T {
        let files = RealFiles;
        let clock = SystemClock::new();
        let processes = SystemProcesses;
        let system = System {
            runner: &self.runner,
            files: &files,
            clock: &clock,
            console,
            processes: &processes,
            environment: &self.env,
        };
        let mut settings = Settings::from_environment(&self.env);
        settings.colors = false;
        self.mapper.clone_into(&mut settings.mapper);
        settings.open_check = OpenCheck::Markers(self.fake.join("holders"));
        let _log = KernelLog::new(&files, "7", &settings.base, std::process::id())
            .with_device(&self.fake.join("log"))
            .labelled()
            .install();
        let prompter = Recorded {
            console: ConsoleInteraction::new(system).colors(false),
            prompts: self.fake.join("prompts"),
            answers: self.fake.join("answers"),
            delay: self.prompt_delay,
        };
        let mut session = Unlocker::new(Luks {
            system,
            settings: &settings,
            interaction: &prompter,
        });
        let result = body(&mut session, &system);
        self.console_output
            .lock()
            .unwrap()
            .push_str(&console.output());
        result
    }

    /// `luks_unlock_all` with the console `console`: 0 when it succeeded.
    pub fn unlock_with(&self, console: &LinesConsole) -> i32 {
        self.with_session(console, |session, _| {
            i32::from(session.unlock_all().is_err())
        })
    }

    /// `luks_unlock_all` with `lines` on standard input (`<<<`).
    pub fn unlock(&self, lines: &[&str]) -> i32 {
        self.unlock_with(&LinesConsole::new(lines, Duration::ZERO, Duration::ZERO))
    }

    /// `luks_close_all [only]`: 0 when it succeeded.
    pub fn close(&self, only: Option<&str>) -> i32 {
        let console = LinesConsole::new(&[], Duration::ZERO, Duration::ZERO);
        let only = only.map(|name| name.parse().expect("a mapping name"));
        self.with_session(&console, |session, _| {
            i32::from(session.close_all(only.as_ref()).is_err())
        })
    }

    /// Whether the mapping `name` exists.
    pub fn mapped(&self, name: &str) -> bool {
        RealFiles
            .metadata(&self.mapper.join(name))
            .is_ok_and(|metadata| metadata.kind == zbm_platform::files::FileKind::BlockDevice)
    }

    /// The mappings that exist.
    pub fn mappings(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.mapper)
            .map(|entries| {
                entries
                    .filter_map(|entry| {
                        entry
                            .ok()
                            .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    })
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// Everything written to the consoles of the case (`$FAKE/out`).
    pub fn out(&self) -> String {
        self.console_output.lock().unwrap().clone()
    }

    /// How many times `text` appears in the console output, as `grep -c`
    /// counts lines.
    pub fn prompts(&self, text: &str) -> usize {
        self.out()
            .lines()
            .filter(|line| line.contains(text))
            .count()
    }

    /// The log (`$FAKE/log`).
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.fake.join("log")).unwrap_or_default()
    }

    /// The timed prompts (`$FAKE/prompts`).
    pub fn timed_prompts(&self) -> String {
        std::fs::read_to_string(self.fake.join("prompts")).unwrap_or_default()
    }

    /// The commands that ran (`$FAKE/calls`).
    pub fn calls(&self) -> Vec<String> {
        self.runner.calls.lock().unwrap().clone()
    }

    /// Whether a command that ran contains `text`.
    pub fn called(&self, text: &str) -> bool {
        self.calls().iter().any(|call| call.contains(text))
    }

    /// The state file of the mappings.
    pub fn state(&self) -> String {
        std::fs::read_to_string(self.base.join("luks-mappings")).unwrap_or_default()
    }

    /// The pools `zpool export` exported.
    pub fn exported(&self) -> Vec<String> {
        std::fs::read_to_string(self.fake.join("exported"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Whether the pool `name` still exists.
    pub fn has_pool(&self, name: &str) -> bool {
        self.fake.join("pools").join(name).exists()
    }
}
