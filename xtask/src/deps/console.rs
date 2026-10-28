//! The console stays out of the domain (ROADMAP.md, DM2): domain crates
//! take a `Context`, which has no console, and reach the person through an
//! `Interaction`, which front ends implement. Their code outside tests may
//! not name the `System` bundle or the `Console` port
//! ([`CONSOLE_PORTS`]), apart from the crates of [`UNTIL`].

use std::collections::HashSet;

use super::LAYERS;
use crate::error::Result;
use crate::metrics::scan::CONSOLE_PORTS;
use crate::metrics::tree::{ScannedFile, scan_package};
use crate::workspace::Workspace;

/// The layer whose crates may not reach the console.
pub const LAYER: &str = "domain";

/// Domain crates that still reach the console, and the item of ROADMAP.md
/// that ends it.
pub const UNTIL: &[(&str, &str)] = &[(
    "zbm-boot",
    "DM7: its prompts (the resume question, the boot message, key \
     loading) move to the front ends",
)];

/// Every place where a domain crate names a console port, one line each.
///
/// # Errors
///
/// When a source file cannot be read or parsed.
pub fn problems(workspace: &Workspace) -> Result<Vec<String>> {
    let crates = LAYERS
        .iter()
        .filter(|layer| layer.name == LAYER)
        .flat_map(|layer| layer.crates.iter().copied())
        .filter(|name| !UNTIL.iter().any(|(until, _)| until == name));
    let mut problems = Vec::new();
    for name in crates {
        let Some(package) = workspace.package(name) else {
            continue;
        };
        let files = scan_package(package, &HashSet::new())?;
        problems.extend(file_problems(name, &files));
    }
    Ok(problems)
}

/// The places in `files`, of the crate `name`, that name a console port.
fn file_problems(name: &str, files: &[ScannedFile]) -> Vec<String> {
    files
        .iter()
        .filter(|file| !file.test_code)
        .flat_map(|file| {
            file.scan.console_ports.iter().map(move |line| {
                format!(
                    "{}:{line}: {name} names {}: domain crates take a Context and ask \
                     through an Interaction",
                    file.path.display(),
                    CONSOLE_PORTS.join(" or ")
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::metrics::scan::scan_file;

    fn scanned(source: &str, test_code: bool) -> ScannedFile {
        let file = syn::parse_file(source).unwrap();
        ScannedFile {
            path: PathBuf::from("src/lib.rs"),
            test_code,
            scan: scan_file(&file, source.lines().count(), test_code, &HashSet::new()),
        }
    }

    #[test]
    fn the_console_ports_are_refused_outside_tests() {
        let source = "use zbm_platform::{Context, System};\n\
                      fn run(context: &Context<'_>) {}\n\
                      fn ask(console: &dyn zbm_platform::Console) {}\n\
                      #[cfg(test)]\n\
                      mod tests {\n\
                          fn fake(system: zbm_platform::System<'_>) {}\n\
                      }\n";

        let problems = file_problems("zbm-luks", &[scanned(source, false)]);

        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("src/lib.rs:1: zbm-luks names System or Console"));
        assert!(problems[1].starts_with("src/lib.rs:3: "));
        assert!(file_problems("zbm-luks", &[scanned(source, true)]).is_empty());
    }

    #[test]
    fn only_the_listed_crates_still_reach_the_console() {
        let domain = LAYERS.iter().find(|layer| layer.name == LAYER).unwrap();
        for (name, _) in UNTIL {
            assert!(domain.crates.contains(name), "{name} is not a domain crate");
        }
    }
}
