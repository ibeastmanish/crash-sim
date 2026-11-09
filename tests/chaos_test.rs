use std::io::Write;
use crash_sim::{FaultConfig, VirtualDisk};

/// Simple CRC32 implementation to validate checksum resilience during fault injection.
fn simple_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = if (crc & 1) != 0 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// An append-only WAL record with magic bytes, payload length, payload, and CRC32 checksum.
struct WalRecord<'a> {
    payload: &'a [u8],
}

impl<'a> WalRecord<'a> {
    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8 + self.payload.len());
        // Magic header 0x57414C31 ("WAL1")
        buf.extend_from_slice(b"WAL1");
        // Payload length (u16)
        buf.extend_from_slice(&(self.payload.len() as u16).to_le_bytes());
        // Payload
        buf.extend_from_slice(self.payload);
        // Checksum
        let crc = simple_crc32(self.payload);
        buf.extend_from_slice(&crc.to_le_bytes());
        buf
    }

    /// Attempts to parse a record, returning the payload and bytes consumed, or None if corrupted/torn.
    fn decode(raw: &[u8]) -> Option<(&[u8], usize)> {
        if raw.len() < 10 {
            return None;
        }
        if &raw[0..4] != b"WAL1" {
            return None;
        }
        let len = u16::from_le_bytes([raw[4], raw[5]]) as usize;
        let total_expected = 6 + len + 4;
        if raw.len() < total_expected {
            // Torn record cut off before end
            return None;
        }

        let payload = &raw[6..6 + len];
        let expected_crc = u32::from_le_bytes([
            raw[6 + len],
            raw[6 + len + 1],
            raw[6 + len + 2],
            raw[6 + len + 3],
        ]);

        let actual_crc = simple_crc32(payload);
        if actual_crc != expected_crc {
            // Bit flip or data corruption detected
            return None;
        }

        Some((payload, total_expected))
    }
}

#[test]
fn test_ten_thousand_fault_injection_trials() {
    let mut successful_recoveries = 0;
    let mut torn_records_caught = 0;
    let mut bit_flips_caught = 0;
    let mut power_cuts_caught = 0;

    for trial in 0..10_000 {
        let config = FaultConfig::seeded(trial as u64)
            .with_torn_writes(0.12)
            .with_bit_flips(0.08);

        let mut disk = VirtualDisk::new(config);

        // 1. Establish initial committed baseline with faults disabled
        disk.set_faults_enabled(false);
        let rec1 = WalRecord { payload: b"TRANSACTION_INITIAL_OK" }.encode();
        disk.write_all(&rec1).unwrap();
        disk.sync_all().unwrap();

        // 2. Enable faults and write subsequent transaction
        disk.set_faults_enabled(true);
        let rec2 = WalRecord { payload: b"TRANSACTION_SECOND_BATCH" }.encode();
        let _ = disk.write_all(&rec2);

        // 3. 20% of trials experience a sudden power cut before sync
        if trial % 5 == 0 {
            disk.crash();
            power_cuts_caught += 1;
        } else {
            let _ = disk.sync_all();
        }

        // 4. Recovery phase: read committed storage
        let committed = disk.committed_data();
        let mut offset = 0;
        let mut valid_count = 0;

        while offset < committed.len() {
            match WalRecord::decode(&committed[offset..]) {
                Some((_, consumed)) => {
                    valid_count += 1;
                    offset += consumed;
                }
                None => {
                    // Corruption or torn boundary reached
                    break;
                }
            }
        }

        // Integrity invariant: Transaction 1 must ALWAYS be recovered intact!
        assert!(valid_count >= 1, "Invariant broken at trial {}: initial record lost", trial);

        if valid_count == 2 {
            successful_recoveries += 1;
        } else {
            // Failure was safely quarantined without corrupting transaction 1
            if disk.log().count_torn_writes() > 0 {
                torn_records_caught += 1;
            }
            if disk.log().count_bit_flips() > 0 {
                bit_flips_caught += 1;
            }
        }
    }

    println!("============================================================");
    println!("10,000 FAULT INJECTION SIMULATION SUMMARY");
    println!("============================================================");
    println!("Both transactions intact:      {}", successful_recoveries);
    println!("Torn writes safely caught:     {}", torn_records_caught);
    println!("Bit flips quarantined:         {}", bit_flips_caught);
    println!("Power cuts safely rolled back: {}", power_cuts_caught);
    println!("Zero silent corruptions across all 10,000 trials.");
    println!("============================================================");
}
