//! What the start-up and boot code needs from the program that runs it.

/// The recovery shell and late LUKS unlocking, which the start-up program
/// provides; tests provide fakes.
pub trait Recovery {
    /// Unlocks LUKS volumes that appeared since the first unlock
    /// (`zbm_luks_unlock_late`).
    ///
    /// # Errors
    ///
    /// When none was opened.
    fn unlock_late(&mut self) -> Result<(), crate::error::BootError>;

    /// Opens the recovery shell with a message (`emergency_shell`); the caller
    /// carries on when it ends.
    fn emergency_shell(&mut self, message: &str);
}
