//! Audit log and event definitions for tracking injected storage faults.

/// A discrete fault event injected during an I/O operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultEvent {
    /// A torn write occurred, writing only a partial subset of the requested buffer.
    TornWrite {
        stream_offset: usize,
        attempted_bytes: usize,
        written_bytes: usize,
    },
    /// A bit flip corruption was injected into the written payload.
    BitFlip {
        stream_offset: usize,
        byte_index: usize,
        bit_index: u8,
        original_byte: u8,
        corrupted_byte: u8,
    },
    /// A simulated simulated I/O error was returned to the caller.
    IoError {
        stream_offset: usize,
        error_description: String,
    },
    /// An explicit flush or sync barrier was recorded.
    SyncBarrier {
        flushed_offset: usize,
    },
    /// A simulated power cut occurred, discarding uncommitted dirty bytes.
    PowerCut {
        stream_offset: usize,
        discarded_bytes: usize,
    },
}

/// An audit log storing all fault events injected during a test session.
#[derive(Debug, Default, Clone)]
pub struct FaultLog {
    events: Vec<FaultEvent>,
}

impl FaultLog {
    /// Creates a new empty fault log.
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Records an event in the log.
    pub fn record(&mut self, event: FaultEvent) {
        self.events.push(event);
    }

    /// Returns a slice of all recorded fault events.
    pub fn events(&self) -> &[FaultEvent] {
        &self.events
    }

    /// Returns the total number of faults recorded.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns true if no faults were recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Clears the recorded events.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Returns the count of torn write events recorded.
    pub fn count_torn_writes(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, FaultEvent::TornWrite { .. }))
            .count()
    }

    /// Returns the count of bit flip events recorded.
    pub fn count_bit_flips(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, FaultEvent::BitFlip { .. }))
            .count()
    }

    /// Returns the count of simulated power cuts.
    pub fn count_power_cuts(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, FaultEvent::PowerCut { .. }))
            .count()
    }
}
