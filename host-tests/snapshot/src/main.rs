//! Host tests for the firmware's `snapshot` module: the record that carries
//! unsynced activity across System OFF. `cargo test` runs them.

#[path = "../../../src/snapshot.rs"]
#[allow(dead_code)]
mod snapshot;

fn main() {
    println!("run `cargo test`");
}

#[cfg(test)]
mod tests {
    use super::snapshot::*;

    fn header() -> Header {
        Header {
            first_minute: 29_500_000,
            minutes: 240,
            day: 20_400,
            steps: 8_123,
            checksum: 0,
            synced: true,
        }
    }

    /// A record as `system_off::save` writes it: samples, then the header
    /// sealed with their checksum.
    fn record(samples: &[u8]) -> Vec<u8> {
        let mut header = header();
        header.minutes = (samples.len() / SAMPLE_BYTES) as u16;
        header.checksum = header.expected_checksum(checksum(CHECKSUM_SEED, samples));
        let mut bytes = header.to_bytes().to_vec();
        bytes.extend_from_slice(samples);
        bytes
    }

    /// What `system_off::restore` does with a record: the header and
    /// samples if they check out.
    fn read(bytes: &[u8]) -> Option<(Header, &[u8])> {
        let header = Header::from_bytes(bytes[..HEADER_BYTES].try_into().unwrap())?;
        let samples = &bytes[HEADER_BYTES..][..usize::from(header.minutes) * SAMPLE_BYTES];
        // In chunks, as it's read back.
        let hash = samples.chunks(36).fold(CHECKSUM_SEED, checksum);
        (header.expected_checksum(hash) == header.checksum).then_some((header, samples))
    }

    fn samples(minutes: usize) -> Vec<u8> {
        (0..minutes * SAMPLE_BYTES)
            .map(|i| (i * 7 % 251) as u8)
            .collect()
    }

    #[test]
    fn a_record_reads_back() {
        let samples = samples(240);
        let bytes = record(&samples);
        let (header, read_samples) = read(&bytes).expect("a valid record");
        assert_eq!(header.first_minute, 29_500_000);
        assert_eq!(header.minutes, 240);
        assert_eq!(header.day, 20_400);
        assert_eq!(header.steps, 8_123);
        assert_eq!(read_samples, &samples[..]);
    }

    #[test]
    fn steps_without_history_read_back() {
        let mut header = header();
        header.minutes = 0;
        header.day = UNKNOWN_DAY;
        header.checksum = header.expected_checksum(CHECKSUM_SEED);
        let bytes = header.to_bytes();
        let (read_header, samples) = read(&bytes).expect("a valid record");
        assert_eq!(read_header.day, UNKNOWN_DAY);
        assert!(samples.is_empty());
    }

    #[test]
    fn erased_flash_is_no_record() {
        let bytes = vec![0xFF; MAX_BYTES];
        assert_eq!(read(&bytes), None);
    }

    #[test]
    fn a_consumed_record_is_no_record() {
        // As `system_off::retire` leaves it: the consumed word cleared in
        // place, everything else untouched.
        let mut bytes = record(&samples(10));
        let at = CONSUMED_OFFSET as usize;
        // Programming NOR flash only clears bits.
        for (byte, consumed) in bytes[at..at + 4].iter_mut().zip(CONSUMED.to_le_bytes()) {
            *byte &= consumed;
        }
        assert_eq!(read(&bytes), None);
    }

    #[test]
    fn a_corrupt_record_is_rejected() {
        let mut bytes = record(&samples(100));
        bytes[HEADER_BYTES + 321] ^= 0x10;
        assert_eq!(read(&bytes), None);
        // In the header's fields too.
        let mut bytes = record(&samples(100));
        bytes[16] ^= 0x01; // steps
        assert_eq!(read(&bytes), None);
    }

    #[test]
    fn an_unfinished_save_is_no_record() {
        // The header goes last: cut off before it, the samples are there
        // but the header is still erased.
        let mut bytes = record(&samples(50));
        bytes[..HEADER_BYTES].fill(0xFF);
        assert_eq!(read(&bytes), None);
    }

    #[test]
    fn too_many_minutes_is_nonsense() {
        let mut header = header();
        header.minutes = MAX_MINUTES as u16 + 1;
        assert_eq!(Header::from_bytes(&header.to_bytes()), None);
    }

    /// Whether the phone's copy ends just before the record is its own
    /// word, outside the checksum: a record from an older build, which
    /// leaves it erased, reads as not knowing.
    #[test]
    fn the_synced_word_reads_back() {
        let bytes = record(&samples(10));
        assert_eq!(&bytes[28..32], b"sync");
        assert!(read(&bytes).expect("a valid record").0.synced);
        let mut older = bytes.clone();
        older[28..32].fill(0xFF);
        assert!(!read(&older).expect("still valid").0.synced);
        // Anything but the word is not knowing.
        older[28..32].copy_from_slice(b"synd");
        assert!(!read(&older).expect("still valid").0.synced);
    }

    #[test]
    fn without_a_sync_the_whole_day_is_saved() {
        let newest = 29_500_000;
        assert_eq!(
            unsynced_from(newest, None),
            (newest - MAX_MINUTES as i64 + 1, false)
        );
    }

    #[test]
    fn after_a_sync_from_where_the_next_fetch_starts() {
        let newest = 29_500_000;
        // Synced an hour ago: the next fetch sends from just after that.
        assert_eq!(
            unsynced_from(newest, Some(newest - 60)),
            (newest - 59, true)
        );
        // Synced long ago: never more than the history holds.
        assert_eq!(
            unsynced_from(newest, Some(newest - 5000)),
            (newest - MAX_MINUTES as i64 + 1, true)
        );
    }

    #[test]
    fn a_sync_past_the_newest_minute_saves_the_whole_day() {
        // The phone fetched up to minute 29_500_600 while its clock was
        // ten hours ahead; put right, the history dropped those minutes,
        // and holds new ones the phone hasn't had.
        let newest = 29_500_000;
        assert_eq!(
            unsynced_from(newest, Some(newest + 600)),
            (newest - MAX_MINUTES as i64 + 1, false)
        );
    }

    #[test]
    fn a_full_day_fits() {
        assert_eq!(MAX_BYTES, 32 + 1440 * 6);
        let bytes = record(&samples(MAX_MINUTES));
        assert!(read(&bytes).is_some());
        assert!(MAX_BYTES as u32 <= SLOT_BYTES);
    }

    // ==========================================================================
    // Checkpoints, and which record a boot restores
    // ==========================================================================

    /// A record as `system_off::save` writes it, saved as of `as_of` (the
    /// minute after the history's newest), with `minutes` of samples, or
    /// none (then starting at `as_of`, as `empty_from` has it).
    fn saved(as_of: u32, minutes: usize) -> Vec<u8> {
        let samples = samples(minutes);
        let mut header = header();
        header.first_minute = as_of - minutes as u32;
        header.minutes = minutes as u16;
        header.checksum = header.expected_checksum(checksum(CHECKSUM_SEED, &samples));
        let mut bytes = header.to_bytes().to_vec();
        bytes.extend_from_slice(&samples);
        bytes
    }

    /// What `system_off::restore` picks, from the planned record and the
    /// checkpoints' slots (each as the flash holds it), with the clock at
    /// `now` if known: the slot of the newest checkpoint that's usable and
    /// checks out, else the planned record if it's fresh and checks out.
    #[derive(Debug, PartialEq)]
    enum Restored {
        Checkpoint(usize),
        Planned,
        Nothing,
    }

    fn boot(planned: &[u8], slots: &[Vec<u8>], now: Option<i64>) -> Restored {
        let first_32 = |bytes: &[u8]| -> [u8; HEADER_BYTES] {
            let mut header = [0xFF; HEADER_BYTES];
            let n = bytes.len().min(HEADER_BYTES);
            header[..n].copy_from_slice(&bytes[..n]);
            header
        };
        let floor = as_of_any(&first_32(planned));
        let mut best: Option<(usize, u32)> = None;
        for (slot, bytes) in slots.iter().enumerate() {
            if let Some(header) = Header::from_bytes(&first_32(bytes))
                && newer_checkpoint(header.as_of(), best.map(|b| b.1), floor, now)
                && read(bytes).is_some()
            {
                best = Some((slot, header.as_of()));
            }
        }
        match best {
            Some((slot, _)) => Restored::Checkpoint(slot),
            None if planned.len() >= HEADER_BYTES && read(planned).is_some() => Restored::Planned,
            None => Restored::Nothing,
        }
    }

    /// Flash as erased, a slot's worth.
    fn erased() -> Vec<u8> {
        vec![0xFF; SLOT_BYTES as usize]
    }

    /// The planned record, as `system_off::retire` leaves it once the
    /// phone has fetched it.
    fn consumed(mut bytes: Vec<u8>) -> Vec<u8> {
        let at = CONSUMED_OFFSET as usize;
        bytes[at..at + 4].copy_from_slice(&CONSUMED.to_le_bytes());
        bytes
    }

    const NOW: i64 = 29_500_000;

    #[test]
    fn a_record_is_as_of_the_minute_after_its_newest() {
        let bytes = saved(NOW as u32 + 1, 300);
        let (header, _) = read(&bytes).expect("a valid record");
        assert_eq!(header.as_of(), NOW as u32 + 1);
        // With nothing to save, it starts there, and says so all the same.
        assert_eq!(empty_from(Some(NOW)), NOW as u32 + 1);
        let bytes = saved(empty_from(Some(NOW)), 0);
        assert_eq!(
            read(&bytes).expect("a valid record").0.as_of(),
            NOW as u32 + 1
        );
        // Before the clock was ever known: nothing to say.
        assert_eq!(empty_from(None), 0);
    }

    #[test]
    fn the_planned_records_time_is_read_once_it_is_retired() {
        let bytes = saved(NOW as u32, 60);
        let header: [u8; HEADER_BYTES] = bytes[..HEADER_BYTES].try_into().unwrap();
        assert_eq!(as_of_any(&header), Some(NOW as u32));
        let retired: [u8; HEADER_BYTES] = consumed(bytes)[..HEADER_BYTES].try_into().unwrap();
        assert_eq!(Header::from_bytes(&retired), None, "no longer restored");
        assert_eq!(as_of_any(&retired), Some(NOW as u32), "but still dated");
        assert_eq!(as_of_any(&[0xFF; HEADER_BYTES]), None);
        assert_eq!(as_of_any(&[0; HEADER_BYTES]), None);
    }

    #[test]
    fn checkpoints_take_the_slots_in_turn() {
        let due: Vec<i64> = (NOW..NOW + 200).filter(|&m| checkpoint_due(m)).collect();
        assert_eq!(due.len(), 200 / CHECKPOINT_MINUTES as usize);
        for pair in due.windows(2) {
            assert_eq!(pair[1] - pair[0], CHECKPOINT_MINUTES);
            let (a, b) = (checkpoint_slot(pair[0]), checkpoint_slot(pair[1]));
            assert_eq!(
                (a + 1) % CHECKPOINT_SLOTS,
                b,
                "the next slot, never the same"
            );
        }
        // Each slot once in a round.
        let mut round: Vec<u32> = due[..CHECKPOINT_SLOTS as usize]
            .iter()
            .map(|&m| checkpoint_slot(m))
            .collect();
        round.sort();
        assert_eq!(round, (0..CHECKPOINT_SLOTS).collect::<Vec<_>>());
        // A minute later in the same pair goes to the same slot, as the
        // record it saves is the same one, newer.
        assert_eq!(checkpoint_slot(due[0]), checkpoint_slot(due[0] + 1));
        // And before 1970, for a clock that's never been right.
        assert!(checkpoint_slot(-7) < CHECKPOINT_SLOTS);
    }

    #[test]
    fn a_save_erases_only_the_sectors_it_needs() {
        assert_eq!(erase_bytes(0), 4096);
        // A sector holds 677 minutes after the header: 11 hours.
        assert_eq!(erase_bytes(677), 4096);
        assert_eq!(erase_bytes(678), 8192);
        assert_eq!(erase_bytes(MAX_MINUTES as u16), SLOT_BYTES);
        assert_eq!(SLOT_BYTES, 3 * 4096);
    }

    #[test]
    fn a_crash_restores_the_newest_checkpoint() {
        // Three checkpoints, two minutes apart, in slots 4..7; the
        // planned record from an update hours before, already retired.
        let mut slots = vec![erased(); CHECKPOINT_SLOTS as usize];
        slots[4] = saved(NOW as u32 - 3, 200);
        slots[5] = saved(NOW as u32 - 1, 202);
        slots[6] = saved(NOW as u32 + 1, 204);
        let planned = consumed(saved(NOW as u32 - 600, 30));
        assert_eq!(boot(&planned, &slots, Some(NOW)), Restored::Checkpoint(6));
        // Wherever it is in the round.
        slots.swap(6, 0);
        assert_eq!(boot(&planned, &slots, Some(NOW)), Restored::Checkpoint(0));
        // Or after a power-on, with the clock unknown.
        assert_eq!(boot(&planned, &slots, None), Restored::Checkpoint(0));
    }

    #[test]
    fn a_save_cut_short_falls_back_to_the_one_before() {
        let mut slots = vec![erased(); CHECKPOINT_SLOTS as usize];
        slots[4] = saved(NOW as u32 - 3, 200);
        slots[5] = saved(NOW as u32 - 1, 202);
        // The crash came mid-save: erased, samples written, no header yet.
        let mut partial = saved(NOW as u32 + 1, 204);
        partial[..HEADER_BYTES].fill(0xFF);
        slots[6] = partial;
        assert_eq!(boot(&[], &slots, Some(NOW)), Restored::Checkpoint(5));
        // Or mid-erase: the header's sector erased, the rest still the
        // slot's old record.
        let mut half_erased = saved(NOW as u32 - 63, 1000);
        half_erased[..4096].fill(0xFF);
        slots[6] = half_erased;
        assert_eq!(boot(&[], &slots, Some(NOW)), Restored::Checkpoint(5));
    }

    #[test]
    fn a_corrupt_checkpoint_falls_back_to_an_older_one() {
        let mut slots = vec![erased(); CHECKPOINT_SLOTS as usize];
        slots[4] = saved(NOW as u32 - 3, 200);
        slots[5] = saved(NOW as u32 - 1, 202);
        slots[5][HEADER_BYTES + 100] ^= 0x04;
        assert_eq!(boot(&[], &slots, Some(NOW)), Restored::Checkpoint(4));
        // Its header's fields are covered too: a first minute that changed
        // would put every sample under the wrong minute.
        slots[4][5] ^= 0x01;
        assert_eq!(boot(&[], &slots, Some(NOW)), Restored::Nothing);
    }

    #[test]
    fn another_version_is_no_checkpoint() {
        let mut slots = vec![erased(); CHECKPOINT_SLOTS as usize];
        slots[5] = saved(NOW as u32 - 1, 202);
        // A later layout, under a magic of its own, even with a checksum
        // that would match ours: not read.
        let mut newer = saved(NOW as u32 + 1, 204);
        newer[..4].copy_from_slice(b"urH2");
        slots[6] = newer;
        assert_eq!(boot(&[], &slots, Some(NOW)), Restored::Checkpoint(5));
        // Nor a header that claims more minutes than a record holds,
        // whatever its checksum says.
        let mut long = saved(NOW as u32 + 1, 204);
        long[8..10].copy_from_slice(&(MAX_MINUTES as u16 + 1).to_le_bytes());
        slots[6] = long;
        assert_eq!(boot(&[], &slots, Some(NOW)), Restored::Checkpoint(5));
    }

    #[test]
    fn a_power_on_with_nothing_saved_starts_clean() {
        // Erased flash, and what the free space held before (InfiniTime's
        // file system, say): nothing restored.
        let slots = vec![erased(); CHECKPOINT_SLOTS as usize];
        assert_eq!(boot(&erased(), &slots, None), Restored::Nothing);
        let noise: Vec<Vec<u8>> = (0..CHECKPOINT_SLOTS as usize)
            .map(|slot| {
                (0..SLOT_BYTES as usize)
                    .map(|i| (i * 31 + slot * 7) as u8)
                    .collect()
            })
            .collect();
        assert_eq!(boot(&noise[0], &noise, None), Restored::Nothing);
        let zeros = vec![vec![0; SLOT_BYTES as usize]; CHECKPOINT_SLOTS as usize];
        assert_eq!(boot(&zeros[0], &zeros, None), Restored::Nothing);
    }

    #[test]
    fn checkpoints_older_than_the_planned_record_are_of_the_run_before() {
        let mut slots = vec![erased(); CHECKPOINT_SLOTS as usize];
        slots[9] = saved(NOW as u32 - 300, 500);
        // An update since (its record restored and fetched, so retired):
        // the checkpoint's minutes are the phone's.
        let planned = consumed(saved(NOW as u32 - 100, 600));
        assert_eq!(boot(&planned, &slots, Some(NOW)), Restored::Nothing);
        // System OFF since, its record not fetched yet: that's restored.
        let planned = saved(NOW as u32 - 100, 600);
        assert_eq!(boot(&planned, &slots, Some(NOW)), Restored::Planned);
        // A checkpoint since the update wins over its record, which it
        // holds the rest of.
        slots[10] = saved(NOW as u32 - 50, 650);
        assert_eq!(boot(&planned, &slots, Some(NOW)), Restored::Checkpoint(10));
        // One of the same minute as the planned record is no newer.
        slots[10] = saved(NOW as u32 - 100, 600);
        assert_eq!(boot(&planned, &slots, Some(NOW)), Restored::Planned);
    }

    #[test]
    fn a_checkpoint_out_of_the_clocks_reach_is_ignored() {
        let mut slots = vec![erased(); CHECKPOINT_SLOTS as usize];
        slots[3] = saved(NOW as u32 - 1, 100);
        // Ahead of the resumed clock: not from before this reset.
        slots[4] = saved(NOW as u32 + 10, 100);
        assert_eq!(boot(&[], &slots, Some(NOW)), Restored::Checkpoint(3));
        // The minute after the newest, the clock a moment behind it.
        slots[4] = saved(NOW as u32 + 2, 100);
        assert_eq!(boot(&[], &slots, Some(NOW)), Restored::Checkpoint(4));
        // Over a day old: none of it fits in the history.
        let old = NOW as u32 - MAX_MINUTES as u32;
        let slots = vec![saved(old, 100), saved(old + 1, 100)];
        assert_eq!(boot(&[], &slots, Some(NOW)), Restored::Checkpoint(1));
        assert_eq!(boot(&[], &slots[..1], Some(NOW)), Restored::Nothing);
        // Unless the clock isn't known.
        assert_eq!(boot(&[], &slots[..1], None), Restored::Checkpoint(0));
    }

    #[test]
    fn a_tie_keeps_the_first() {
        assert!(newer_checkpoint(100, None, None, None));
        assert!(!newer_checkpoint(100, Some(100), None, None));
        assert!(newer_checkpoint(101, Some(100), None, None));
        assert!(!newer_checkpoint(100, None, Some(100), None));
        assert!(newer_checkpoint(101, None, Some(100), None));
    }
}

#[cfg(test)]
mod fuzz;

/// Proofs, for `cargo kani`: what the fuzzer tries for some headers, for
/// every one. The record is all that carries a day's activity across
/// System OFF, so what's written must read back: a header that didn't
/// would lose the day at the next boot, with nothing to say why.
#[cfg(kani)]
mod proofs {
    use super::snapshot::*;

    /// For every header a record can have (any first minute, date, steps
    /// and checksum, up to a day of minutes, and whether the phone's copy
    /// ends just before it): it reads back from its 32 bytes exactly as
    /// it was written, as a fresh record.
    #[kani::proof]
    fn every_header_reads_back_as_written() {
        let header = Header {
            first_minute: kani::any(),
            minutes: kani::any(),
            day: kani::any(),
            steps: kani::any(),
            checksum: kani::any(),
            synced: kani::any(),
        };
        kani::assume(usize::from(header.minutes) <= MAX_MINUTES);
        assert_eq!(Header::from_bytes(&header.to_bytes()), Some(header));
    }

    /// For any 32 bytes of flash (erased, a consumed record, half a
    /// write, anything): they read as a header only with the magic and
    /// an erased consumed word, and then one with no more minutes than a
    /// record holds, so `system_off::restore` never reads samples past
    /// the record's sectors; it's synced only if its synced word says
    /// so; and writing that header gives the same bytes back, bar the
    /// reserved ones, and a synced word that was neither "sync" nor
    /// erased (which reads, and is written, as not synced).
    #[kani::proof]
    fn only_a_fresh_record_within_bounds_is_read() {
        let bytes: [u8; HEADER_BYTES] = kani::any();
        let fresh = bytes[..4] == *b"urH1" && bytes[24..28] == [0xFF; 4];
        match Header::from_bytes(&bytes) {
            Some(header) => {
                assert!(fresh);
                assert!(usize::from(header.minutes) <= MAX_MINUTES);
                assert!(HEADER_BYTES + usize::from(header.minutes) * SAMPLE_BYTES <= MAX_BYTES);
                assert_eq!(header.synced, bytes[28..32] == *b"sync");
                let again = header.to_bytes();
                assert_eq!(again[..10], bytes[..10]);
                assert_eq!(again[12..28], bytes[12..28]);
                if header.synced || bytes[28..32] == [0xFF; 4] {
                    assert_eq!(again[28..32], bytes[28..32]);
                }
            }
            None => {
                let minutes = u16::from_le_bytes([bytes[8], bytes[9]]);
                assert!(!fresh || usize::from(minutes) > MAX_MINUTES);
            }
        }
    }

    /// For any newest minute (to the year 6000) and any minute the phone
    /// has fetched through (or none): the record starts within the
    /// history (the day up to the newest minute), or just after it when
    /// the phone has it all, so it never holds more minutes than a record
    /// can, which `from_bytes` would refuse at the next boot. It starts
    /// where the phone's next fetch will (just after what it has), and
    /// says so, when that's in the history; it holds the whole history,
    /// still saying so, when the phone's copy ends before it; and it
    /// holds the whole history, saying nothing is known, when the phone
    /// has nothing, or has "fetched" a minute the history hasn't reached
    /// (the clock went back since).
    #[kani::proof]
    fn the_record_starts_within_the_history() {
        let newest: i64 = kani::any();
        kani::assume((0..1 << 31).contains(&newest));
        let synced: Option<i64> = kani::any();
        if let Some(synced) = synced {
            kani::assume((0..1 << 32).contains(&synced));
        }
        let (from, known) = unsynced_from(newest, synced);
        let minutes = newest - from + 1;
        assert!(0 <= minutes && minutes <= MAX_MINUTES as i64);
        let oldest = newest - MAX_MINUTES as i64 + 1;
        match synced {
            Some(synced) if synced <= newest => {
                assert!(known);
                assert_eq!(from, (synced + 1).max(oldest));
            }
            _ => {
                assert!(!known);
                assert_eq!(from, oldest);
            }
        }
    }

    /// For any minute the clock can show (to the year 6000, and before
    /// 1970): its checkpoint has a slot, and the next one due goes in the
    /// next slot, never in the one just written, which is what the crash
    /// falls back to if it comes mid-save.
    #[kani::proof]
    fn each_checkpoint_spares_the_one_before() {
        let minute: i64 = kani::any();
        kani::assume((-(1 << 31)..1 << 31).contains(&minute));
        let slot = checkpoint_slot(minute);
        assert!(slot < CHECKPOINT_SLOTS);
        let next = checkpoint_slot(minute + CHECKPOINT_MINUTES);
        assert_eq!(next, (slot + 1) % CHECKPOINT_SLOTS);
        assert_ne!(next, slot);
    }

    /// For any length a record can have: a save erases all of it, in
    /// whole sectors, and nothing past its slot.
    #[kani::proof]
    fn a_save_erases_its_record_within_its_slot() {
        let minutes: u16 = kani::any();
        kani::assume(usize::from(minutes) <= MAX_MINUTES);
        let erased = erase_bytes(minutes);
        assert!(erased as usize >= HEADER_BYTES + usize::from(minutes) * SAMPLE_BYTES);
        assert!(erased <= SLOT_BYTES);
        assert!(erased.is_multiple_of(SECTOR_BYTES));
    }

    /// For any 32 bytes: the planned record's date (`as_of_any`) is a
    /// fresh record's as-of, whether or not it's been retired since, and
    /// nothing for what wouldn't read as a record either way.
    #[kani::proof]
    fn the_planned_record_is_dated_retired_or_not() {
        let bytes: [u8; HEADER_BYTES] = kani::any();
        let mut retired = bytes;
        retired[CONSUMED_OFFSET as usize..][..4].copy_from_slice(&CONSUMED.to_le_bytes());
        assert_eq!(as_of_any(&bytes), as_of_any(&retired));
        if let Some(header) = Header::from_bytes(&bytes) {
            assert_eq!(as_of_any(&bytes), Some(header.as_of()));
        }
    }
}
