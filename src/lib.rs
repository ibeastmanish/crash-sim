//! # crash-sim
//!
//! Deterministic disk fault-injection and chaos-testing harness for write-ahead logs (WAL),
//! embedded databases, and storage persistence engines in Rust.
//!
//! ## Highlights
//!
//! - **Torn Write Simulation:** Cuts off write operations mid-buffer or across sector boundaries.
//! - **Bit Flip Injection:** Flips isolated bits in payloads to stress-test CRC32 and checksum verification.
//! - **Power Cut Emulation:** Drops uncommitted dirty pages via `VirtualDisk` to ensure atomic recovery.
//! - **Deterministic Reproducibility:** Zero-dependency pseudo-random generator supports fixed seeds across trial matrices.
//! - **Zero External Runtime Dependencies:** Lightweight and suitable for high-frequency unit and integration loops.
//!
//! ## Quick Start
//!
//! ```rust
//! use std::io::Write;
//! use crash_sim::{FaultConfig, FaultWriter};
//!
//! let mut destination = Vec::new();
//! let config = FaultConfig::seeded(42).with_torn_writes(0.5);
//!
//! let mut writer = FaultWriter::new(&mut destination, config);
//! let payload = b"CRITICAL_WAL_RECORD_HEADER";
//! let _ = writer.write(payload);
//!
//! // Inspect recorded faults
//! for event in writer.log().events() {
//!     println!("Recorded fault event: {:?}", event);
//! }
//! ```
//!
//! ## Crash Emulation with VirtualDisk
//!
//! ```rust
//! use std::io::Write;
//! use crash_sim::{FaultConfig, VirtualDisk};
//!
//! let mut disk = VirtualDisk::new(FaultConfig::default());
//!
//! // Step 1: Write and commit initial record
//! disk.write_all(b"COMMIT_POINT_A").unwrap();
//! disk.sync_all().unwrap();
//!
//! // Step 2: Write subsequent dirty batch without fsync
//! disk.write_all(b"_DIRTY_UNSYNCED_DATA").unwrap();
//!
//! // Step 3: Trigger sudden power disconnect
//! disk.crash();
//!
//! // Only committed data survives
//! assert_eq!(disk.committed_data(), b"COMMIT_POINT_A");
//! ```

pub mod config;
pub mod prng;
pub mod record;
pub mod vfs;
pub mod writer;

pub use config::FaultConfig;
pub use prng::SimpleRng;
pub use record::{FaultEvent, FaultLog};
pub use vfs::VirtualDisk;
pub use writer::FaultWriter;
