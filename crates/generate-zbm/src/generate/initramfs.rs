//! The initramfs, built with dracut or mkinitcpio, and the way build commands
//! run.

use zbm_config::spec::OptionSpec;
use zbm_config::spec::generate::{GLOBAL_DRACUT_FLAGS, GLOBAL_INIT_CPIO_FLAGS};
use zbm_core::shell::quote;

use super::graphics::build_options;
use super::settings::Settings;
use super::{Generator, Step, Stop};
use crate::config::Value;

impl Generator<'_, '_, '_> {
    /// Runs a command built for the initramfs or the bundle: its output is logged
    /// when it succeeds and printed when it fails, and failure ends the program
    /// with the command's status.
    pub(super) fn run_build_step(&mut self, words: &[String], output_file: &str) -> Step<()> {
        let command = words.join(" ");
        self.log(&format!("Executing: {command}"));
        let (lines, status) = self.execute(&command)?;
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
        let mut words = environment(settings);
        let flags_key = if settings.usecpio {
            words.extend([
                "mkinitcpio".to_owned(),
                "--config".to_owned(),
                quote(&settings.confd),
            ]);
            if self.debug {
                words.push("-v".to_owned());
            }
            for hook_dir in settings.hookd.iter().flatten() {
                words.push("--hookdir".to_owned());
                words.push(quote(hook_dir));
            }
            &GLOBAL_INIT_CPIO_FLAGS
        } else {
            words.extend([
                "dracut".to_owned(),
                "-f".to_owned(),
                "--confdir".to_owned(),
                quote(&settings.confd),
            ]);
            if !self.debug {
                words.push("-q".to_owned());
            }
            // Fixed file times and order in the archive, with SOURCE_DATE_EPOCH
            // (ROADMAP.md #5); the Perl program does not pass it
            if self.context.reproducible {
                words.push("--reproducible".to_owned());
            }
            &GLOBAL_DRACUT_FLAGS
        };
        words.extend(self.flags(flags_key)?);
        if settings.usecpio {
            words.extend([
                "-A".to_owned(),
                "zfsbootmenu".to_owned(),
                "--generate".to_owned(),
                quote(&output),
                "--kernel".to_owned(),
                quote(kernel_version),
            ]);
        } else {
            words.extend([quote(&output), quote(kernel_version)]);
        }
        self.run_build_step(&words, &output)?;
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
fn environment(settings: &Settings) -> Vec<String> {
    let mut words = Vec::new();
    if let Some(family) = &settings.console_font {
        words.push(format!("zfsbootmenu_console_font={}", quote(family)));
    }
    if let Some(selection) = &settings.graphics {
        words.extend(build_options(selection));
    }
    words
}
