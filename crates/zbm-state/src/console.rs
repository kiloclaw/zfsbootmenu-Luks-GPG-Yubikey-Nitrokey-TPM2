//! The records of the console on the display (`zbm console`, ROADMAP.md
//! #19): its process, the layout and font it uses and those asked of it,
//! the process showing a secret prompt, and its screenshots and scrollback.

use std::path::PathBuf;

use zbm_platform::error::{Error, Result};

use super::{MAX_SCREENSHOTS, SCROLLBACK, State};

impl State<'_> {
    /// The process ID of the console on the display (`zbm console`,
    /// ROADMAP.md #19), while it holds the display: the console writes it
    /// once it can take requests, and removes it once it handed the
    /// display back.
    pub fn console_pid(&self) -> Option<u32> {
        self.read_line("console")?.parse().ok()
    }

    /// Records the console with process ID `pid`.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_console(&self, pid: u32) -> Result<()> {
        self.write("console", format!("{pid}\n").as_bytes())
    }

    /// Records that no console holds the display, nor reads the keyboards.
    pub fn clear_console(&self) {
        self.remove("console");
        self.remove("console-layout");
        self.remove("console-font");
    }

    /// The process ID of the console that handed the display back before
    /// `kexec` and relays on the kernel console: when `kexec` fails, it is
    /// asked to take the display again (`zbm console reattach`).
    pub fn released_console_pid(&self) -> Option<u32> {
        self.read_line("console-released")?.parse().ok()
    }

    /// Records the console with process ID `pid` as one that handed the
    /// display back.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_released_console(&self, pid: u32) -> Result<()> {
        self.write("console-released", format!("{pid}\n").as_bytes())
    }

    /// Records that no console waits to take the display again.
    pub fn clear_released_console(&self) {
        self.remove("console-released");
    }

    /// The keyboard layout the console on the display types with.
    pub fn console_layout(&self) -> Option<String> {
        self.read_line("console-layout")
            .filter(|name| !name.is_empty())
    }

    /// Records the keyboard layout the console types with.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_console_layout(&self, name: &str) -> Result<()> {
        self.write("console-layout", format!("{name}\n").as_bytes())
    }

    /// Asks the console for the keyboard layout `name`
    /// (`zbm console layout`); the console takes the request.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn request_console_layout(&self, name: &str) -> Result<()> {
        self.write("console-layout-request", format!("{name}\n").as_bytes())
    }

    /// Whether a request for a keyboard layout waits for the console.
    pub fn has_console_layout_request(&self) -> bool {
        self.files.exists(&self.path("console-layout-request"))
    }

    /// The keyboard layout asked for, while the request waits.
    pub fn console_layout_request(&self) -> Option<String> {
        self.read_line("console-layout-request")
            .filter(|name| !name.is_empty())
    }

    /// Removes the request for a keyboard layout: the console took it, or
    /// it came too late.
    pub fn clear_console_layout_request(&self) {
        self.remove("console-layout-request");
    }

    /// The font the console on the display draws with: its name, its
    /// cells' size and how many.
    pub fn console_font(&self) -> Option<String> {
        self.read_line("console-font")
            .filter(|font| !font.is_empty())
    }

    /// Records the font the console draws with.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_console_font(&self, font: &str) -> Result<()> {
        self.write("console-font", format!("{font}\n").as_bytes())
    }

    /// Asks the console for the font's size `size` (`zbm console font`);
    /// the console takes the request.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn request_console_font(&self, size: &str) -> Result<()> {
        self.write("console-font-request", format!("{size}\n").as_bytes())
    }

    /// Whether a request for a font's size waits for the console.
    pub fn has_console_font_request(&self) -> bool {
        self.files.exists(&self.path("console-font-request"))
    }

    /// The font's size asked for, while the request waits.
    pub fn console_font_request(&self) -> Option<String> {
        self.read_line("console-font-request")
            .filter(|size| !size.is_empty())
    }

    /// Removes the request for a font's size: the console took it, or it
    /// came too late.
    pub fn clear_console_font_request(&self) {
        self.remove("console-font-request");
    }

    /// The process ID of the program that shows a secret prompt now (a
    /// passphrase, a PIN): the console on the display keeps no scrollback
    /// and shows nothing typed while it is there.
    pub fn secret_prompt_pid(&self) -> Option<u32> {
        self.read_line("secret-prompt")?.parse().ok()
    }

    /// Records that process `pid` shows a secret prompt.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn set_secret_prompt(&self, pid: u32) -> Result<()> {
        self.write("secret-prompt", format!("{pid}\n").as_bytes())
    }

    /// Records that no secret prompt is shown.
    pub fn clear_secret_prompt(&self) {
        self.remove("secret-prompt");
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
                    .exists(&self.path(&format!("screenshot-{number}.png")))
            })
            .ok_or_else(|| Error::invalid("too many screenshots"))?;
        self.write(&format!("screenshot-{number}.txt"), text)?;
        let name = format!("screenshot-{number}.png");
        self.write(&name, image)?;
        Ok(self.path(&name))
    }

    /// Saves `text`, the console's scrollback, in [`SCROLLBACK`]; returns
    /// its path.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn save_scrollback(&self, text: &[u8]) -> Result<PathBuf> {
        self.replace(SCROLLBACK, text)?;
        Ok(self.path(SCROLLBACK))
    }
}
