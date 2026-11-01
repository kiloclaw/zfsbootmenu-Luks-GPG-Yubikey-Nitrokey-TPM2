//! The initramfs, built with dracut or mkinitcpio, and the way build commands
//! run.

use super::graphics::build_options;
use super::invocation::Invocation;
use super::settings::Settings;
use super::{Generator, Step, Stop};
use crate::generate_zbm::config::Value;
use zbm_config::spec::OptionSpec;
use zbm_config::spec::generate::{GLOBAL_DRACUT_FLAGS, GLOBAL_INIT_CPIO_FLAGS};

impl Generator<'_, '_, '_> {
    /// Runs a command built for the initramfs or the bundle: its output is logged
    /// when it succeeds and printed when it fails, and failure ends the program
    /// with the command's status.
    pub(super) fn run_build_step(
        &mut self,
        invocation: &Invocation,
        output_file: &str,
    ) -> Step<()> {
        self.log(&format!("Executing: {}", invocation.line()));
        let (lines, status) = self.execute(invocation)?;
        if status == 0 {
            self.log_lines(&lines);
            Ok(())
        } else {
            self.print_lines(&lines);
            self.print(&format!("Failed to create {output_file}\n"));
            Err(Stop::Exit(status))
        }
    }

    /// Builds the initramfs with dracut or mkinitcpio.
    pub(super) fn create_initramfs(
        &mut self,
        dir: &str,
        kernel_version: &str,
        settings: &Settings,
    ) -> Step<String> {
        let output = format!("{dir}/zfsbootmenu.img");
        let (mut invocation, flags_key) = if settings.usecpio {
            let mut invocation = Invocation::new("mkinitcpio")
                .plain("--config")
                .quoted(&settings.confd);
            if self.debug {
                invocation = invocation.plain("-v");
            }
            for hook_dir in settings.hookd.iter().flatten() {
                invocation = invocation.plain("--hookdir").quoted(hook_dir);
            }
            (invocation, &GLOBAL_INIT_CPIO_FLAGS)
        } else {
            let mut invocation = Invocation::new("dracut")
                .plain("-f")
                .plain("--confdir")
                .quoted(&settings.confd);
            if !self.debug {
                invocation = invocation.plain("-q");
            }
            // Fixed file times and order in the archive, with SOURCE_DATE_EPOCH
            // (ROADMAP.md #5); the Perl program does not pass it
            if self.context.reproducible {
                invocation = invocation.plain("--reproducible");
            }
            (invocation, &GLOBAL_DRACUT_FLAGS)
        };
        for (name, value) in environment(settings) {
            invocation = invocation.env(&name, &value);
        }
        for flag in self.flags(flags_key)? {
            invocation = invocation.words(&flag).map_err(|why| {
                self.print(&format!("{}: '{flag}': {why}\n", flags_key.name));
                Stop::Exit(1)
            })?;
        }
        let invocation = if settings.usecpio {
            invocation
                .plain("-A")
                .plain("zfsbootmenu")
                .plain("--generate")
                .quoted(&output)
                .plain("--kernel")
                .quoted(kernel_version)
        } else {
            invocation.quoted(&output).quoted(kernel_version)
        };
        self.run_build_step(&invocation, &output)?;
        Ok(output)
    }

    /// `Global.DracutFlags` or `Global.InitCPIOFlags`: shell words, as written
    /// in the configuration.
    fn flags(&self, key: &OptionSpec) -> Step<Vec<String>> {
        Ok(match self.config.option(key)? {
            Some(Value::Seq(flags)) => flags
                .borrow()
                .iter()
                .map(|flag| flag.perl_string().unwrap_or_default())
                .collect(),
            Some(flag) => vec![flag.perl_string().unwrap_or_default()],
            None => Vec::new(),
        })
    }
}

/// The build options of the run, for the dracut module or the mkinitcpio hook,
/// which read them from the environment: the console fonts and the graphics
/// drivers.
fn environment(settings: &Settings) -> Vec<(String, String)> {
    let mut variables = Vec::new();
    if let Some(family) = &settings.console_font {
        variables.push(("zfsbootmenu_console_font".to_owned(), family.clone()));
    }
    if let Some(selection) = &settings.graphics {
        variables.extend(build_options(selection));
    }
    variables
}
