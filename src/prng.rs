//! Deterministic, zero-dependency pseudo-random number generator for reproducible chaos trials.

/// A lightweight 64-bit SplitMix pseudo-random generator.
///
/// Designed to provide high statistical quality and deterministic sequences
/// without relying on external crates.
#[derive(Debug, Clone)]
pub struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    /// Creates a new generator seeded with the provided value.
    pub const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    /// Generates the next pseudo-random 64-bit unsigned integer.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Generates a pseudo-random floating-point value in the half-open interval `[0.0, 1.0)`.
    pub fn next_f64(&mut self) -> f64 {
        let val = self.next_u64() >> 11;
        (val as f64) * (1.0 / ((1u64 << 53) as f64))
    }

    /// Generates a pseudo-random usize in the range `[min, max)`.
    pub fn gen_range(&mut self, min: usize, max: usize) -> usize {
        if min >= max {
            return min;
        }
        let span = max - min;
        min + ((self.next_u64() as usize) % span)
    }

    /// Generates a random byte.
    pub fn next_u8(&mut self) -> u8 {
        (self.next_u64() & 0xFF) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_sequence() {
        let mut rng1 = SimpleRng::new(1337);
        let mut rng2 = SimpleRng::new(1337);

        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_float_range() {
        let mut rng = SimpleRng::new(42);
        for _ in 0..1000 {
            let f = rng.next_f64();
            assert!(f >= 0.0 && f < 1.0);
        }
    }

    #[test]
    fn test_gen_range() {
        let mut rng = SimpleRng::new(99);
        for _ in 0..1000 {
            let val = rng.gen_range(10, 20);
            assert!(val >= 10 && val < 20);
        }
    }
}
