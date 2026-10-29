//! What the start-up and boot code needs from the program that runs it.

/// The recovery shell and late LUKS unlocking, which the start-up program
/// provides; tests provide fakes.
pub trait Recovery: Send + Sync {
    /// Unlocks LUKS volumes that appeared since the first unlock.
    ///
    /// # Errors
    ///
    /// When none was opened.
    fn unlock_late(&self) -> Result<(), crate::error::BootError>;

    /// Opens the recovery shell with a message; the caller carries on when it
    /// ends.
    fn emergency_shell(&self, message: &str);
}
