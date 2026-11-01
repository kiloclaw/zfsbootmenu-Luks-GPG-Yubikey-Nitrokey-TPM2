//! The records of the console on the display (`zbm console`, ROADMAP.md
//! #19): its process, the layout and font it uses, and its screenshots and
//! scrollback, in a directory of their own ([`CONSOLE`](super::CONSOLE),
//! ADR 0012), which the console's user may write. They are for those who
//! read them, hooks and users, and tell the boot whether a console runs;
//! what is asked of the console goes on its control socket
//! (`zbm_app::console`).

use std::path::PathBuf;

use zbm_platform::error::{Error, Result};

use super::{MAX_SCREENSHOTS, SCROLLBACK, State};

impl State<'_> {
    /// The directory of the console's records.
    pub fn console_records(&self) -> PathBuf {
        self.path(super::CONSOLE)
    }

    /// Writes the record `name` of the console's records, making their
    /// directory first when the state directory has none yet, as when the
    /// console runs without the supervisor.
    fn record(&self, name: &str, contents: &[u8]) -> Result<()> {
        self.ensure_records();
        self.write(&format!("{}/{name}", super::CONSOLE), contents)
    }

    /// Makes the directory of the console's records when the state
    /// directory is there and they are not.
    fn ensure_records(&self) {
        let records = self.console_records();
        if self.files.exists(&self.base) && !self.files.exists(&records) {
            let _ = self.files.create_dir_all(&records);
        }
    }

    /// Makes the directory of the console's records, if it is missing: the
    /// supervisor makes it first, and gives it to the console's user.
    ///
    /// # Errors
    ///
    /// When it cannot be made.
    pub fn create_console_records(&self) -> Result<()> {
        let path = self.console_records();
        self.files
            .create_dir_all(&path)
            .map_err(|error| Error::io(format!("unable to create {}", path.display()), error))
    }

    /// The process ID of the console on the display (`zbm console`,
    /// ROADMAP.md #19), while it holds the display: the console writes it
    /// when it starts, and removes it once it handed the display back.
    pub fn console_pid(&self) -> Option<u32> {
        self.read_line("console/pid")?.parse().ok()
    }

    /// Records the console with process ID `pid`.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_console(&self, pid: u32) -> Result<()> {
        self.record("pid", format!("{pid}\n").as_bytes())
    }

    /// Records that no console holds the display, nor reads the keyboards.
    pub fn clear_console(&self) {
        self.remove("console/pid");
        self.remove("console/layout");
        self.remove("console/font");
    }

    /// The process ID of the console that handed the display back before
    /// `kexec` and relays on the kernel console: when `kexec` fails, it is
    /// asked to take the display again (`zbm console reattach`).
    pub fn released_console_pid(&self) -> Option<u32> {
        self.read_line("console/released")?.parse().ok()
    }

    /// Records the console with process ID `pid` as one that handed the
    /// display back.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_released_console(&self, pid: u32) -> Result<()> {
        self.record("released", format!("{pid}\n").as_bytes())
    }

    /// Records that no console waits to take the display again.
    pub fn clear_released_console(&self) {
        self.remove("console/released");
    }

    /// The keyboard layout the console on the display types with.
    pub fn console_layout(&self) -> Option<String> {
        self.read_line("console/layout")
            .filter(|name| !name.is_empty())
    }

    /// Records the keyboard layout the console types with.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_console_layout(&self, name: &str) -> Result<()> {
        self.record("layout", format!("{name}\n").as_bytes())
    }

    /// The font the console on the display draws with: its name, its
    /// cells' size and how many.
    pub fn console_font(&self) -> Option<String> {
        self.read_line("console/font")
            .filter(|font| !font.is_empty())
    }

    /// Records the font the console draws with.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_console_font(&self, font: &str) -> Result<()> {
        self.record("font", format!("{font}\n").as_bytes())
    }

    /// Saves a screenshot of the console, its `text` and its `image` (a
    /// PNG file), as `screenshot-N.txt` and `screenshot-N.png`, N the first
    /// number not taken; returns the image's path.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn save_screenshot(&self, text: &[u8], image: &[u8]) -> Result<PathBuf> {
        let number = (1..=MAX_SCREENSHOTS)
            .find(|number| {
                !self
                    .files
                    .exists(&self.path(&format!("console/screenshot-{number}.png")))
            })
            .ok_or_else(|| Error::invalid("too many screenshots"))?;
        self.record(&format!("screenshot-{number}.txt"), text)?;
        let name = format!("screenshot-{number}.png");
        self.record(&name, image)?;
        Ok(self.console_records().join(name))
    }

    /// Saves `text`, the console's scrollback, in [`SCROLLBACK`]; returns
    /// its path.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn save_scrollback(&self, text: &[u8]) -> Result<PathBuf> {
        self.ensure_records();
        self.replace(SCROLLBACK, text)?;
        Ok(self.path(SCROLLBACK))
    }
}
