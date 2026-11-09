# crash-sim

Deterministic disk fault-injection and chaos-testing harness for write-ahead logs (WAL), embedded databases, and storage persistence engines in Rust.

[![Crates.io](https://img.shields.io/crates/v/crash-sim.svg)](https://crates.io/crates/crash-sim)
[![Documentation](https://docs.rs/crash-sim/badge.svg)](https://docs.rs/crash-sim)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

---

## Overview

When building crash-resilient storage systems, standard integration tests that write data cleanly and call `fsync` are insufficient. Storage engines fail in the wild due to partial writes, power drops during page flush, sector misalignment, and media bit flips.

`crash-sim` provides a zero-dependency, reproducible chaos harness designed to perturb I/O streams and simulate disk-level failure modes before they reach production hardware.

Developed as a companion testing framework for [`dtn-wal`](https://crates.io/crates/dtn-wal) to conduct 10,000-trial automated crash resilience verifications.

---

## Architecture

```
+--------------------------------------------------------------+
|                    Application / Storage Engine              |
+--------------------------------------------------------------+
                               |
                               v
+--------------------------------------------------------------+
|                         crash-sim                            |
|                                                              |
|   +-------------------+  +-------------------+  +----------+  |
|   |    Torn Writes    |  |  Bit Corruption   |  | Power Cut|  |
|   |  (Sector Splits)  |  |   (XOR Flips)     |  | (Rollback|  |
|   +-------------------+  +-------------------+  +----------+  |
|                                                              |
|            Deterministic PRNG (SplitMix64 Engine)            |
|            Audit Event Log (Post-Mortem Replay)              |
+--------------------------------------------------------------+
                               |
                               v
+--------------------------------------------------------------+
|               Physical Storage / In-Memory Page Cache         |
+--------------------------------------------------------------+
```

---

## Features

* **Torn Write Injection:** Truncates writes across configurable sector boundaries (512B / 4096B) to model mid-transfer power loss.
* **Bit Flip Simulation:** Flips individual bits within buffers at specified error rates to validate CRC32, Adler, and BLAKE3 checksum quarantine logic.
* **Kernel Page Cache & Power Cut Emulation (`VirtualDisk`):** Separates dirty in-memory buffers from persistent fsynced blocks, allowing instant `crash()` calls that drop un-synced dirty data.
* **Deterministic Chaos:** Zero external dependencies. Uses an embedded SplitMix64 generator accepting explicit seeds for 100% reproducible debugging.
* **Full Audit Logging:** Detailed trace log captures byte offsets, original values, and corrupted bytes for post-mortem analysis.

---

## Installation

Add `crash-sim` to your `Cargo.toml`:

```toml
[dev-dependencies]
crash-sim = "0.1.0"
```

Or via `cargo`:

```bash
cargo add --dev crash-sim
```

---

## Usage Examples

### 1. Intercepting I/O with `FaultWriter`

Wrap any standard `std::io::Write` destination (such as a file, cursor, or socket) to inject failures:

```rust
use std::io::Write;
use crash_sim::{FaultConfig, FaultWriter};

fn main() -> std::io::Result<()> {
    let mut storage = Vec::new();

    // Configure 10% torn writes and 5% bit flips with a fixed seed
    let config = FaultConfig::seeded(1337)
        .with_torn_writes(0.10)
        .with_bit_flips(0.05);

    let mut writer = FaultWriter::new(&mut storage, config);

    let record = b"CRITICAL_TRANSACTION_PAYLOAD";
    let _ = writer.write(record)?;
    writer.flush()?;

    // Review audit log
    for event in writer.log().events() {
        println!("Fault injected: {:?}", event);
    }

    Ok(())
}
```

### 2. Simulating Sudden Power Disconnects with `VirtualDisk`

```rust
use std::io::Write;
use crash_sim::{FaultConfig, VirtualDisk};

fn main() -> std::io::Result<()> {
    let mut disk = VirtualDisk::new(FaultConfig::default());

    // 1. Write baseline record and fsync
    disk.write_all(b"TRANSACTION_1_COMMITTED")?;
    disk.sync_all()?;
    assert_eq!(disk.committed_len(), 23);

    // 2. Write uncommitted data to dirty page cache
    disk.write_all(b"_TRANSACTION_2_UNSYNCED")?;

    // 3. Simulate power disconnect / kernel panic
    disk.crash();

    // 4. Recovery: only committed data survived
    assert_eq!(disk.committed_data(), b"TRANSACTION_1_COMMITTED");
    println!("Power cut survived with zero corruption!");

    Ok(())
}
```

---

## Testing & Verification

Run the full test suite including the 10,000-trial simulation harness:

```bash
cargo test -- --nocapture
```

Sample output:

```text
============================================================
10,000 FAULT INJECTION SIMULATION SUMMARY
============================================================
Both transactions intact:      6572
Torn writes safely caught:     889
Bit flips quarantined:         789
Power cuts safely rolled back: 2000
Zero silent corruptions across all 10,000 trials.
============================================================
```

---

## License

Dual-licensed under either:

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
* MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
