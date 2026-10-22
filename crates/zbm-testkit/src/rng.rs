//! Generated test inputs from a seed.

/// A small random number generator for generated test inputs (xorshift64*).
///
/// The seed comes from `ZBM_TEST_SEED` when set, so a failing case can be run
/// again; generated-input tests print the seed they used.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator with the given seed.
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed.max(1) }
    }

    /// A generator seeded from `ZBM_TEST_SEED`, or with `default` when it is not
    /// set.
    pub fn from_env(default: u64) -> Rng {
        #[allow(
            clippy::disallowed_methods,
            reason = "the seed is chosen by whoever runs the tests"
        )]
        let seed = std::env::var("ZBM_TEST_SEED")
            .ok()
            .and_then(|seed| seed.parse().ok())
            .unwrap_or(default);
        #[allow(
            clippy::disallowed_macros,
            reason = "the test harness shows a failing test's standard error, \
                      with the seed to run it again"
        )]
        {
            eprintln!("generated inputs use ZBM_TEST_SEED={seed}");
        }
        Rng::new(seed)
    }

    /// The next random number.
    pub fn next_u64(&mut self) -> u64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number below `bound`, which must not be 0.
    pub fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("the bound fits in 64 bits");
        usize::try_from(self.next_u64() % bound).expect("the result is below the bound")
    }

    /// One of the given items.
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// A string of up to `max_len` characters taken from `alphabet`.
    pub fn string(&mut self, alphabet: &[char], max_len: usize) -> String {
        let len = self.below(max_len + 1);
        (0..len).map(|_| *self.choose(alphabet)).collect()
    }

    /// A string made of up to `max_parts` pieces taken from `pieces`.
    pub fn join(&mut self, pieces: &[&str], max_parts: usize) -> String {
        let count = self.below(max_parts + 1);
        (0..count).map(|_| *self.choose(pieces)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_is_reproducible() {
        let mut first = Rng::new(42);
        let mut second = Rng::new(42);
        let a: Vec<u64> = (0..5).map(|_| first.next_u64()).collect();
        let b: Vec<u64> = (0..5).map(|_| second.next_u64()).collect();
        assert_eq!(a, b);
        assert!(Rng::new(7).below(3) < 3);
        let text = Rng::new(9).string(&['a', 'b'], 8);
        assert!(text.len() <= 8 && text.chars().all(|c| c == 'a' || c == 'b'));
    }
}
