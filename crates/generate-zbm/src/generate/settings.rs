//! The configuration file and the settings of a run: loading the file,
//! rewriting it for `--enable` and `--disable`, and combining it with the
//! command line options.

use std::io::Write;

use zbm_config::spec::generate::{
    GLOBAL_CONSOLE_FONT, GLOBAL_DRACUT_CONF_DIR, GLOBAL_INIT_CPIO, GLOBAL_INIT_CPIO_CONFIG,
    GLOBAL_INIT_CPIO_HOOK_DIRS, GLOBAL_MANAGE_IMAGES, GLOBAL_VERSION, KERNEL_COMMAND_LINE,
    KERNEL_PATH, KERNEL_PREFIX, KERNEL_VERSION,
};
use zbm_core::fs::{is_readable, strerror};

use super::kernels::KernelRequest;
use super::{Generator, Step, Stop, VERSION, has_value};
use crate::cli::Options;
use crate::config::Value;
use crate::{dump, perl};

/// The values the Perl program keeps in `%runConf` once the options and the
/// configuration are combined.
pub(super) struct Settings {
    /// Whether mkinitcpio builds the initramfs instead of dracut.
    pub(super) usecpio: bool,
    /// The dracut configuration directory or the mkinitcpio configuration file.
    pub(super) confd: String,
    /// The mkinitcpio hook directories.
    pub(super) hookd: Option<Vec<String>>,
    /// The kernel command line written into the EFI bundle.
    pub(super) cmdline: Option<String>,
    /// The ZFSBootMenu version in image names.
    pub(super) version: String,
    /// `Global.ConsoleFont`, for the build option `zfsbootmenu_console_font`;
    /// the Perl program has no such key.
    pub(super) console_font: Option<String>,
}

impl Generator<'_, '_, '_> {
    /// Loads the configuration file at `path`, or reports why it cannot.
    pub(super) fn load_config(&mut self, path: &str) -> Step<()> {
        if !is_readable(path) {
            self.print(&format!(
                "Configuration {path} does not exist or is unreadable\n"
            ));
            return Err(Stop::Exit(1));
        }
        let loaded = std::fs::read(path)
            .map_err(|error| strerror(&error))
            .and_then(|bytes| {
                crate::config::load(&perl::decode_utf8_perlqq(&bytes)).map_err(|error| error.0)
            });
        match loaded {
            Ok(config) => {
                self.config = config;
                Ok(())
            }
            Err(error) => {
                self.print(&format!("Unable to parse configuration {path} as YAML.\n"));
                let _ = writeln!(self.io.stderr, "{error}");
                Err(Stop::Exit(1))
            }
        }
    }

    /// `--enable` and `--disable`: rewrite the configuration with
    /// `Global.ManageImages` set.
    pub(super) fn set_manage_images(&mut self, path: &str, enable: bool) -> Step<()> {
        self.config
            .set_option(&GLOBAL_MANAGE_IMAGES, Value::Bool(enable))?;
        let text = dump::dump(&self.config);
        let mut file = std::fs::File::create(path).map_err(|error| {
            Stop::fatal(
                format!("Could not open '{path}' for writing: {}", strerror(&error)),
                &error,
            )
        })?;
        file.write_all(text.as_bytes()).map_err(|error| {
            Stop::fatal(
                format!("Unable to write {path}: {}", strerror(&error)),
                &error,
            )
        })?;
        self.print(&format!("ManageImages set to '{enable}' in {path}\n"));
        Err(Stop::Exit(0))
    }

    /// Combines the options with the configuration: the initramfs generator, its
    /// configuration and hook directories.
    pub(super) fn settings(&mut self, options: &Options) -> Step<Settings> {
        let usecpio = match options.initcpio {
            Some(usecpio) => usecpio,
            None => match self.config.option(&GLOBAL_INIT_CPIO)? {
                Some(value) => value.truthy(),
                None => {
                    let (_, status) = self.execute("sh -c \"command -v dracut\"")?;
                    if status == 0 {
                        self.print("No initramfs generator specified; using dracut\n");
                        false
                    } else {
                        self.print("No initramfs generator specified; using mkinitcpio\n");
                        true
                    }
                }
            },
        };

        let confd = match &options.confd {
            Some(confd) => confd.clone(),
            None => {
                let (default, key) = if usecpio {
                    ("/etc/zfsbootmenu/mkinitcpio.conf", &GLOBAL_INIT_CPIO_CONFIG)
                } else {
                    ("/etc/zfsbootmenu/dracut.conf.d", &GLOBAL_DRACUT_CONF_DIR)
                };
                self.setting(key)?.unwrap_or_else(|| default.to_owned())
            }
        };

        let mut hookd = options.hookd.clone();
        if usecpio && hookd.is_none() {
            let hooks = match self.config.option(&GLOBAL_INIT_CPIO_HOOK_DIRS)? {
                Some(Value::Seq(items)) => items
                    .borrow()
                    .iter()
                    .map(|item| item.perl_string().unwrap_or_default())
                    .collect(),
                Some(other) => vec![other.perl_string().unwrap_or_default()],
                None => Vec::new(),
            };
            hookd = Some(hooks);
        }

        Ok(Settings {
            usecpio,
            confd,
            hookd,
            cmdline: options.cmdline.clone(),
            version: String::new(),
            console_font: self.console_font()?,
        })
    }

    /// `Global.ConsoleFont`, in lower case; a value that is not a font family
    /// stops the run before anything is built.
    fn console_font(&mut self) -> Step<Option<String>> {
        let Some(font) = self.setting(&GLOBAL_CONSOLE_FONT)? else {
            return Ok(None);
        };
        let family = font.to_lowercase();
        if GLOBAL_CONSOLE_FONT.value.allows(Some(&family)) {
            return Ok(Some(family));
        }
        self.print(&format!(
            "Global.ConsoleFont must be terminus or spleen, not '{font}'\n"
        ));
        Err(Stop::Exit(1))
    }

    /// Fills in the kernel options that are not given from the configuration,
    /// and sets the command line and the ZFSBootMenu version of `settings`.
    /// Returns the kernel the options and the configuration ask for.
    pub(super) fn apply_defaults(
        &self,
        options: &Options,
        settings: &mut Settings,
    ) -> Step<KernelRequest> {
        // Configuration values supply defaults for options that are not given.
        let mut path = options.kernel.clone();
        let mut prefix = options.kernel_prefix.clone();
        let mut version = options.kernel_version.clone();
        let mut zbm_version = options.version.clone();
        if let Some(value) = self.setting(&KERNEL_PATH)?.filter(|text| !text.is_empty())
            && !has_value(path.as_ref())
        {
            path = Some(value);
        }
        if let Some(value) = self
            .setting(&KERNEL_PREFIX)?
            .filter(|text| !text.is_empty())
            && !has_value(prefix.as_ref())
        {
            prefix = Some(value);
        }
        if let Some(value) = self
            .setting(&KERNEL_VERSION)?
            .filter(|text| !text.is_empty())
            && !has_value(version.as_ref())
        {
            version = Some(perl::mark_current(&value));
        }
        if let Some(value) = self
            .setting(&GLOBAL_VERSION)?
            .filter(|text| !text.is_empty())
            && !has_value(zbm_version.as_ref())
        {
            zbm_version = Some(value);
        }
        if let Some(value) = self
            .setting(&KERNEL_COMMAND_LINE)?
            .filter(|text| !text.is_empty())
            && !has_value(settings.cmdline.as_ref())
        {
            settings.cmdline = Some(value);
        }
        settings.version = match zbm_version.filter(|text| !text.is_empty()) {
            Some(version) => perl::replace_current(&perl::mark_current(&version), VERSION),
            None => VERSION.to_owned(),
        };
        Ok(KernelRequest {
            path,
            prefix,
            version,
        })
    }
}
