//! `runtime-RUNTIME-signals`: the signal dispositions of an early-setup
//! hook, of fzf in the menu and of the recovery shell, on the console. The
//! legacy run records them in `signals.json`; the Rust run must reproduce
//! those of the hook and the shell (the menu is package E).

use std::fmt::Write as _;
use std::fs;
use std::thread;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::expect::regex;
use crate::pyfmt;
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// Where the hook goes in the build directory.
const HOOK_PATH: &str = "hooks/early-setup.d/95-zbmtest-signals";

/// An early-setup hook that reports its own signal dispositions and records
/// those of fzf once the menu runs.
const HOOK: &str = r##"#!/bin/bash
sig() { grep -E '^Sig(Blk|Ign|Cgt):' "/proc/$1/status" | tr -s ' \t' ' ' | tr '\n' ' '; }
echo "ZBMTEST-SIG hook=[$(sig $$)]"
(
  n=0
  while [ "${n}" -lt 1800 ]; do
    for f in /proc/[0-9]*/comm; do
      read -r c < "$f" 2>/dev/null || continue
      [ "${c}" = fzf ] || continue
      p="${f#/proc/}"; p="${p%/comm}"
      echo "fzf=[$(sig "$p")]" > /zfsbootmenu/zbmtest-fzf-sig
      exit 0
    done
    sleep 1
    n=$((n + 1))
  done
) </dev/null >/dev/null 2>&1 &
"##;

/// What the recovery shell runs: fzf's dispositions as the hook recorded
/// them, then its own.
const SHELL_COMMAND: &str = concat!(
    r#"read -r l < /zfsbootmenu/zbmtest-fzf-sig 2>/dev/null; echo "${l}"; "#,
    r"grep -E '^Sig(Blk|Ign|Cgt)' /proc/$$/status"
);

/// The dispositions reported.
const SIG_KEYS: [&str; 3] = ["SigBlk", "SigIgn", "SigCgt"];

/// The processes whose dispositions are recorded, as `signals.json` names
/// them.
const PROCESSES: [&str; 3] = ["hook", "fzf", "shell"];

/// Signal masks by disposition, in the order reported.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Masks(Vec<(String, String)>);

impl Masks {
    /// The mask of `key`, if reported.
    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, mask)| mask.as_str())
    }

    /// Sets the mask of `key`, in place when it is known.
    fn set(&mut self, key: &str, mask: &str) {
        match self.0.iter_mut().find(|(name, _)| name == key) {
            Some(entry) => mask.clone_into(&mut entry.1),
            None => self.0.push((key.to_owned(), mask.to_owned())),
        }
    }

    /// Whether every disposition of [`SIG_KEYS`], and only those, was
    /// reported.
    fn is_complete(&self) -> bool {
        let mut keys: Vec<&str> = self.0.iter().map(|(key, _)| key.as_str()).collect();
        let mut expected = SIG_KEYS;
        keys.sort_unstable();
        expected.sort_unstable();
        keys == expected
    }

    /// The masks as Python writes a dictionary.
    fn text(&self) -> String {
        pyfmt::dict(&self.0)
    }

    /// The masks in a JSON value, if it is an object of strings.
    fn from_json(value: &serde_json::Value) -> Masks {
        let mut masks = Masks::default();
        for (key, mask) in value.as_object().into_iter().flatten() {
            if let Some(mask) = mask.as_str() {
                masks.set(key, mask);
            }
        }
        masks
    }
}

/// `{SigBlk, SigIgn, SigCgt: hex mask}` from `/proc/<pid>/status` lines.
fn masks(text: &str) -> Result<Masks> {
    let pattern = regex(r"(Sig(?:Blk|Ign|Cgt)):\s*([0-9a-f]+)")?;
    let mut masks = Masks::default();
    for captures in pattern.captures_iter(text) {
        masks.set(&captures[1], &captures[2]);
    }
    Ok(masks)
}

/// The dispositions of the three processes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Dispositions {
    hook: Masks,
    fzf: Masks,
    shell: Masks,
}

impl Dispositions {
    /// The masks of `process` (`hook`, `fzf` or `shell`).
    fn of(&self, process: &str) -> &Masks {
        match process {
            "hook" => &self.hook,
            "fzf" => &self.fzf,
            _ => &self.shell,
        }
    }

    /// `signals.json`, as Python's `json.dumps(indent=2)` writes it.
    fn to_json(&self) -> String {
        let quote = |text: &str| serde_json::Value::from(text).to_string();
        let mut out = String::from("{\n");
        for (index, process) in PROCESSES.iter().enumerate() {
            let masks = self.of(process);
            write!(out, "  {}: ", quote(process)).ok();
            if masks.0.is_empty() {
                out.push_str("{}");
            } else {
                let entries: Vec<String> = masks
                    .0
                    .iter()
                    .map(|(key, mask)| format!("    {}: {}", quote(key), quote(mask)))
                    .collect();
                write!(out, "{{\n{}\n  }}", entries.join(",\n")).ok();
            }
            out.push_str(if index + 1 < PROCESSES.len() {
                ",\n"
            } else {
                "\n"
            });
        }
        out.push_str("}\n");
        out
    }

    /// The dispositions in a `signals.json`.
    fn from_json(text: &str) -> Result<Dispositions> {
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| Error::Setup(format!("signals.json: {error}")))?;
        Ok(Dispositions {
            hook: Masks::from_json(&value["hook"]),
            fzf: Masks::from_json(&value["fzf"]),
            shell: Masks::from_json(&value["shell"]),
        })
    }
}

/// The dispositions of fzf and of the shell in the output of
/// [`SHELL_COMMAND`].
fn split_output(out: &str) -> Result<(Masks, Masks)> {
    match regex(r"fzf=\[(.*?)\]")?.captures(out) {
        Some(captures) => {
            let end = captures.get(0).map_or(out.len(), |whole| whole.end());
            Ok((masks(&captures[1])?, masks(&out[end..])?))
        }
        None => {
            let shell = out.rsplit("fzf=[").next().unwrap_or(out);
            Ok((Masks::default(), masks(shell)?))
        }
    }
}

/// Records the dispositions with `runtime` and checks them.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test, runtime: ZbmRuntime) -> Result<()> {
    let settings = test.settings();
    let image = super::image(settings, "auto-signals", runtime, Some((HOOK_PATH, HOOK)))?;
    let machine =
        Machine::boot(&image, super::ESCAPE_APPEND).with_disks([super::disk(settings, 0)?]);
    let mut vm = Vm::start(test, "boot", &machine)?;
    let result = observe(&mut vm, runtime)?;
    vm.kill();
    let path = test.dir().join("signals.json");
    fs::write(&path, result.to_json()).map_err(Error::io(format!("writing {}", path.display())))?;
    verify(test, runtime, &result)
}

/// Reads the dispositions on the console: the hook's at start-up, then, in
/// the recovery shell, fzf's as the hook recorded them and the shell's.
fn observe(vm: &mut Vm<'_>, runtime: ZbmRuntime) -> Result<Dispositions> {
    vm.expect1(r"ZBMTEST-SIG hook=\[(.*?)\]", 1200)?;
    let hook = masks(&vm.group(1))?;
    super::escape_countdown(vm)?;
    if runtime == ZbmRuntime::Legacy {
        vm.test()
            .step("menu: waiting for fzf, then CTRL+R for the recovery shell");
        thread::sleep(Duration::from_secs(15));
        vm.send("\x12")?;
    }
    vm.shell_ready()?;
    let (_, out) = vm.sh_show(SHELL_COMMAND)?;
    let (fzf, shell) = split_output(&out)?;
    Ok(Dispositions { hook, fzf, shell })
}

/// Checks the dispositions; the Rust run compares them with the legacy
/// run's.
fn verify(test: &Test, runtime: ZbmRuntime, result: &Dispositions) -> Result<()> {
    test.note(&format!(
        "hook {}; fzf {}; shell {}",
        result.hook.text(),
        result.fzf.text(),
        result.shell.text()
    ));
    test.check(
        result.hook.is_complete(),
        "the hook reported its dispositions",
    );
    test.check(
        result.shell.is_complete(),
        "the recovery shell reported its dispositions",
    );
    if runtime == ZbmRuntime::Legacy {
        test.check(result.fzf.is_complete(), "fzf's dispositions were recorded");
        return Ok(());
    }
    let reference = test
        .settings()
        .work
        .join("tests/runtime-legacy-signals/signals.json");
    test.check(reference.exists(), "the legacy run recorded the reference");
    if reference.exists() {
        let text = fs::read_to_string(&reference)
            .map_err(Error::io(format!("reading {}", reference.display())))?;
        let legacy = Dispositions::from_json(&text)?;
        for process in ["hook", "shell"] {
            for key in SIG_KEYS {
                let ours = result.of(process).get(key);
                let theirs = legacy.of(process).get(key);
                test.check(
                    ours == theirs,
                    &format!(
                        "{process} {key}: {} (legacy {})",
                        pyfmt::option(ours),
                        pyfmt::option(theirs)
                    ),
                );
            }
        }
        test.note("fzf runs in the menu, which arrives with package E");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
