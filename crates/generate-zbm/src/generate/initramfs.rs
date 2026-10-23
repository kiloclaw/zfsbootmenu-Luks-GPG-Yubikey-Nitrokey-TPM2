//! The initramfs, built with dracut or mkinitcpio, and the way build commands
//! run.

use zbm_config::spec::generate::{GLOBAL_DRACUT_FLAGS, GLOBAL_INIT_CPIO_FLAGS};
use zbm_core::shell::quote;

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

    /// `createInitramfs`: builds the initramfs with dracut or mkinitcpio.
    pub(super) fn create_initramfs(
        &mut self,
        dir: &str,
        kernel_version: &str,
        settings: &Settings,
    ) -> Step<String> {
        let output = format!("{dir}/zfsbootmenu.img");
        let mut words: Vec<String> = Vec::new();
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
            &GLOBAL_DRACUT_FLAGS
        };
        // The flags are shell words, as written in the configuration.
        match self.config.option(flags_key)? {
            Some(Value::Seq(flags)) => words.extend(
                flags
                    .borrow()
                    .iter()
                    .map(|flag| flag.perl_string().unwrap_or_default()),
            ),
            Some(flag) => words.push(flag.perl_string().unwrap_or_default()),
            None => {}
        }
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
}
