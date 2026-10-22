//! What the user answered at a timed prompt.

/// The answer to a timed prompt, such as "LUKS device /dev/sda2 is missing:
/// RETURN to retry now, ESCAPE to skip this device".
///
/// ```
/// use zbm_types::Answer;
///
/// assert_ne!(Answer::Continue, Answer::Escape);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Answer {
    /// RETURN, or the time ran out: what the prompt does by default.
    Continue,
    /// ESCAPE: the other choice.
    Escape,
}
