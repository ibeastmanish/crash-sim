//! Configuration profiles for storage fault injection and chaos simulations.

/// Configuration parameters governing the frequency and type of injected disk faults.
#[derive(Debug, Clone)]
pub struct FaultConfig {
    /// Probability of a write being torn midway through execution (0.0 to 1.0).
    pub torn_write_rate: f64,
    /// Probability of a bit flip corruption occurring in written bytes (0.0 to 1.0).
    pub bit_flip_rate: f64,
    /// Probability of an arbitrary I/O error being returned (0.0 to 1.0).
    pub io_error_rate: f64,
    /// Sector size in bytes used for block alignment calculations (typically 512 or 4096).
    pub sector_size: usize,
    /// Random seed for deterministic reproducibility. If None, a default seed is used.
    pub seed: u64,
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self {
            torn_write_rate: 0.0,
            bit_flip_rate: 0.0,
            io_error_rate: 0.0,
            sector_size: 4096,
            seed: 0x1337_CAFE_BEEF,
        }
    }
}

impl FaultConfig {
    /// Creates a default configuration with a specified deterministic seed.
    pub fn seeded(seed: u64) -> Self {
        Self {
            seed,
            ..Default::default()
        }
    }

    /// Creates an aggressive chaos profile suited for stress-testing recovery loops.
    pub fn extreme_chaos(seed: u64) -> Self {
        Self {
            torn_write_rate: 0.15,
            bit_flip_rate: 0.05,
            io_error_rate: 0.05,
            sector_size: 4096,
            seed,
        }
    }

    /// Sets the probability of torn writes (0.0 to 1.0).
    pub fn with_torn_writes(mut self, rate: f64) -> Self {
        self.torn_write_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Sets the probability of bit flips (0.0 to 1.0).
    pub fn with_bit_flips(mut self, rate: f64) -> Self {
        self.bit_flip_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Sets the probability of transient I/O errors (0.0 to 1.0).
    pub fn with_io_errors(mut self, rate: f64) -> Self {
        self.io_error_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Sets the sector size boundary in bytes.
    pub fn with_sector_size(mut self, bytes: usize) -> Self {
        self.sector_size = if bytes == 0 { 512 } else { bytes };
        self
    }
}
