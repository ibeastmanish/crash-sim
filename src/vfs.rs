//! In-memory simulated storage device supporting sudden power-cut simulations and fsync rollback.

use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::config::FaultConfig;
use crate::prng::SimpleRng;
use crate::record::{FaultEvent, FaultLog};

/// A crash-aware in-memory block store modeling operating system page cache semantics.
///
/// Distinguishes between dirty (unflushed) data and committed (fsynced) physical storage.
/// Allows simulating abrupt kernel panics and power disconnects by dropping dirty buffers.
#[derive(Debug, Clone)]
pub struct VirtualDisk {
    committed: Vec<u8>,
    dirty: Vec<u8>,
    cursor: usize,
    config: FaultConfig,
    rng: SimpleRng,
    log: FaultLog,
    faults_enabled: bool,
}

impl VirtualDisk {
    /// Creates a new empty virtual disk using the specified fault profile.
    pub fn new(config: FaultConfig) -> Self {
        let rng = SimpleRng::new(config.seed);
        Self {
            committed: Vec::new(),
            dirty: Vec::new(),
            cursor: 0,
            config,
            rng,
            log: FaultLog::new(),
            faults_enabled: true,
        }
    }

    /// Toggles whether fault injection is active for subsequent write or sync operations.
    pub fn set_faults_enabled(&mut self, enabled: bool) {
        self.faults_enabled = enabled;
    }

    /// Simulates a sudden system power cut or kernel crash.
    ///
    /// All uncommitted dirty writes since the last `sync_all` barrier are instantly discarded.
    pub fn crash(&mut self) {
        let discarded = if self.dirty.len() > self.committed.len() {
            self.dirty.len() - self.committed.len()
        } else {
            0
        };

        self.log.record(FaultEvent::PowerCut {
            stream_offset: self.committed.len(),
            discarded_bytes: discarded,
        });

        self.dirty = self.committed.clone();
        if self.cursor > self.dirty.len() {
            self.cursor = self.dirty.len();
        }
    }

    /// Commits all pending dirty pages to persistent physical state, modeling `fsync`.
    pub fn sync_all(&mut self) -> io::Result<()> {
        if self.faults_enabled && self.config.torn_write_rate > 0.0 && self.rng.next_f64() < self.config.torn_write_rate {
            let total_len = self.dirty.len();
            if total_len > self.committed.len() {
                let uncommitted_len = total_len - self.committed.len();
                let torn_commit = self.rng.gen_range(1, uncommitted_len + 1);
                let new_committed_len = self.committed.len() + torn_commit;

                self.committed = self.dirty[..new_committed_len].to_vec();

                self.log.record(FaultEvent::TornWrite {
                    stream_offset: self.committed.len(),
                    attempted_bytes: uncommitted_len,
                    written_bytes: torn_commit,
                });
                return Ok(());
            }
        }

        self.committed = self.dirty.clone();
        self.log.record(FaultEvent::SyncBarrier {
            flushed_offset: self.committed.len(),
        });
        Ok(())
    }

    /// Returns the length of committed data on persistent storage.
    pub fn committed_len(&self) -> usize {
        self.committed.len()
    }

    /// Returns a slice of the committed data currently preserved on simulated storage.
    pub fn committed_data(&self) -> &[u8] {
        &self.committed
    }

    /// Returns a reference to the fault log recorded by this virtual disk.
    pub fn log(&self) -> &FaultLog {
        &self.log
    }
}

impl Write for VirtualDisk {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        // Check for bit-flip fault
        let mut to_write = buf.to_vec();
        if self.faults_enabled && self.config.bit_flip_rate > 0.0 && self.rng.next_f64() < self.config.bit_flip_rate {
            let byte_idx = self.rng.gen_range(0, to_write.len());
            let bit_idx = (self.rng.next_u8() % 8) as u8;
            let original = to_write[byte_idx];
            let corrupted = original ^ (1 << bit_idx);
            to_write[byte_idx] = corrupted;

            self.log.record(FaultEvent::BitFlip {
                stream_offset: self.cursor + byte_idx,
                byte_index: byte_idx,
                bit_index: bit_idx,
                original_byte: original,
                corrupted_byte: corrupted,
            });
        }

        // Expand dirty buffer if cursor is past end
        if self.cursor + to_write.len() > self.dirty.len() {
            self.dirty.resize(self.cursor + to_write.len(), 0);
        }

        self.dirty[self.cursor..self.cursor + to_write.len()].copy_from_slice(&to_write);
        self.cursor += to_write.len();

        Ok(to_write.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // In OS terms, flush flushes userland buffers to page cache.
        // sync_all models the physical persistence barrier.
        Ok(())
    }
}

impl Read for VirtualDisk {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cursor >= self.dirty.len() {
            return Ok(0);
        }
        let available = self.dirty.len() - self.cursor;
        let to_read = available.min(buf.len());
        buf[..to_read].copy_from_slice(&self.dirty[self.cursor..self.cursor + to_read]);
        self.cursor += to_read;
        Ok(to_read)
    }
}

impl Seek for VirtualDisk {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::End(offset) => (self.dirty.len() as i64) + offset,
            SeekFrom::Current(offset) => (self.cursor as i64) + offset,
        };

        if new_pos < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Seek position before origin",
            ));
        }

        self.cursor = new_pos as usize;
        Ok(self.cursor as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_cut_discards_dirty_pages() {
        let mut disk = VirtualDisk::new(FaultConfig::default());

        // Write batch 1 and sync
        disk.write_all(b"BATCH1_PERSISTENT").unwrap();
        disk.sync_all().unwrap();
        assert_eq!(disk.committed_len(), 17);

        // Write batch 2 without sync
        disk.write_all(b"_BATCH2_UNSYNCED").unwrap();
        assert_eq!(disk.dirty.len(), 33);

        // Simulate crash
        disk.crash();

        // Disk should roll back to batch 1
        assert_eq!(disk.committed_data(), b"BATCH1_PERSISTENT");
        assert_eq!(disk.dirty.as_slice(), b"BATCH1_PERSISTENT");
        assert_eq!(disk.log().count_power_cuts(), 1);
    }
}
