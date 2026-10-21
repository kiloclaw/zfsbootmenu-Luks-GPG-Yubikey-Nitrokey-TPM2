//! What the start-up and boot code needs from the program that runs it.

/// The recovery shell and late LUKS unlocking, which the start-up program
/// provides; tests provide fakes.
pub trait Recovery {
    /// Unlocks LUKS volumes that appeared since the first unlock, and returns
    /// whether one was opened (`zbm_luks_unlock_late`).
    fn unlock_late(&mut self) -> bool;

    /// Opens the recovery shell with a message (`emergency_shell`); the caller
    /// carries on when it ends.
    fn emergency_shell(&mut self, message: &str);
}
