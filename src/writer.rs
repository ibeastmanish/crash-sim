//! Fault-injecting writer wrapper for intercepting and perturbing I/O streams.

use std::io::{self, Error, ErrorKind, Write};

use crate::config::FaultConfig;
use crate::prng::SimpleRng;
use crate::record::{FaultEvent, FaultLog};

/// A transparent wrapper around any standard `std::io::Write` target that injects
/// simulated disk failures, torn writes, bit flips, and I/O exceptions.
pub struct FaultWriter<W: Write> {
    inner: W,
    config: FaultConfig,
    rng: SimpleRng,
    log: FaultLog,
    total_bytes_written: usize,
    total_bytes_flushed: usize,
    enabled: bool,
}

impl<W: Write> FaultWriter<W> {
    /// Creates a new `FaultWriter` wrapping the given output destination with the specified configuration.
    pub fn new(inner: W, config: FaultConfig) -> Self {
        let rng = SimpleRng::new(config.seed);
        Self {
            inner,
            config,
            rng,
            log: FaultLog::new(),
            total_bytes_written: 0,
            total_bytes_flushed: 0,
            enabled: true,
        }
    }

    /// Temporarily pauses or resumes fault injection without resetting internal stream state.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns a reference to the fault audit log.
    pub fn log(&self) -> &FaultLog {
        &self.log
    }

    /// Consumes the wrapper and returns the underlying writer.
    pub fn into_inner(self) -> W {
        self.inner
    }

    /// Returns an immutable reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    /// Returns a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Returns the cumulative number of bytes committed through the writer.
    pub fn total_bytes_written(&self) -> usize {
        self.total_bytes_written
    }

    /// Returns the offset up to which data has been explicitly flushed.
    pub fn total_bytes_flushed(&self) -> usize {
        self.total_bytes_flushed
    }
}

impl<W: Write> Write for FaultWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return self.inner.write(buf);
        }

        if !self.enabled {
            let written = self.inner.write(buf)?;
            self.total_bytes_written += written;
            return Ok(written);
        }

        // 1. Simulate transient I/O exceptions
        if self.config.io_error_rate > 0.0 && self.rng.next_f64() < self.config.io_error_rate {
            self.log.record(FaultEvent::IoError {
                stream_offset: self.total_bytes_written,
                error_description: "Simulated unrecoverable media read/write fault".to_string(),
            });
            return Err(Error::new(
                ErrorKind::Other,
                "Simulated unrecoverable media read/write fault",
            ));
        }

        // 2. Simulate torn writes
        let mut target_len = buf.len();
        if self.config.torn_write_rate > 0.0 && self.rng.next_f64() < self.config.torn_write_rate {
            if buf.len() > 1 {
                target_len = self.rng.gen_range(1, buf.len());
                self.log.record(FaultEvent::TornWrite {
                    stream_offset: self.total_bytes_written,
                    attempted_bytes: buf.len(),
                    written_bytes: target_len,
                });
            }
        }

        let slice = &buf[..target_len];

        // 3. Simulate bit flip corruption
        if self.config.bit_flip_rate > 0.0 && self.rng.next_f64() < self.config.bit_flip_rate {
            let byte_idx = self.rng.gen_range(0, slice.len());
            let bit_idx = (self.rng.next_u8() % 8) as u8;
            let original = slice[byte_idx];
            let corrupted = original ^ (1 << bit_idx);

            let mut corrupted_buf = slice.to_vec();
            corrupted_buf[byte_idx] = corrupted;

            self.log.record(FaultEvent::BitFlip {
                stream_offset: self.total_bytes_written,
                byte_index: byte_idx,
                bit_index: bit_idx,
                original_byte: original,
                corrupted_byte: corrupted,
            });

            let written = self.inner.write(&corrupted_buf)?;
            self.total_bytes_written += written;
            return Ok(written);
        }

        let written = self.inner.write(slice)?;
        self.total_bytes_written += written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()?;
        self.total_bytes_flushed = self.total_bytes_written;
        self.log.record(FaultEvent::SyncBarrier {
            flushed_offset: self.total_bytes_flushed,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_write_pass_through() {
        let cursor = Vec::new();
        let config = FaultConfig::seeded(1234);
        let mut writer = FaultWriter::new(cursor, config);

        let data = b"Hello crash resilience";
        let written = writer.write(data).unwrap();
        assert_eq!(written, data.len());

        writer.flush().unwrap();
        assert_eq!(writer.into_inner(), data);
    }

    #[test]
    fn test_torn_write_injection() {
        let cursor = Vec::new();
        let config = FaultConfig::seeded(777).with_torn_writes(1.0); // 100% torn writes
        let mut writer = FaultWriter::new(cursor, config);

        let payload = vec![0xAA; 128];
        let written = writer.write(&payload).unwrap();

        assert!(written < payload.len());
        assert_eq!(writer.log().count_torn_writes(), 1);
    }

    #[test]
    fn test_bit_flip_injection() {
        let cursor = Vec::new();
        let config = FaultConfig::seeded(888).with_bit_flips(1.0); // 100% bit flip
        let mut writer = FaultWriter::new(cursor, config);

        let payload = vec![0x00; 64];
        let written = writer.write(&payload).unwrap();
        assert_eq!(written, payload.len());

        let output = writer.into_inner();
        assert!(output.iter().any(|&b| b != 0x00));
    }
}
