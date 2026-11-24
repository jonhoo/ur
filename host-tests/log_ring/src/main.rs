//! Host tests for the firmware's `log_ring` module: where the field log's
//! sectors are in the SPI flash. `cargo test` runs them.

// Used only by the tests (and the proofs).
#[path = "../../../src/log_ring.rs"]
#[allow(dead_code)]
mod log_ring;

// The ring's neighbour below, for checking that no sector reaches it.
#[path = "../../../src/dfu_slot.rs"]
#[allow(dead_code)]
mod dfu_slot;

fn main() {
    println!("run `cargo test`");
}

/// Whether `range` lies within one of the ring's two stretches: what every
/// sector the log erases, programs or reads must do.
#[cfg(any(test, kani))]
fn in_ring(range: core::ops::Range<u32>) -> bool {
    use log_ring::{FIRST, SECOND};
    range.start <= range.end
        && ((FIRST.start <= range.start && range.end <= FIRST.end)
            || (SECOND.start <= range.start && range.end <= SECOND.end))
}

/// The ring as it was up to 0.11.2: 512 sectors, the top 2 MiB.
#[cfg(any(test, kani))]
const OLD_SECTORS: u32 = 512;
#[cfg(any(test, kani))]
const OLD_START: u32 = 0x20_0000;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::log_ring::*;
    use super::{OLD_SECTORS, OLD_START, dfu_slot, in_ring};

    #[test]
    fn the_ring_has_745_sectors() {
        assert_eq!(SECTORS, 745);
    }

    /// A watch updated from a build with the old ring finds each of its
    /// sectors where that build wrote it.
    #[test]
    fn the_first_512_sectors_are_where_the_whole_ring_was() {
        for index in 0..OLD_SECTORS {
            assert_eq!(sector_address(index), OLD_START + index * SECTOR);
        }
        assert_eq!(sector_address(0), 0x20_0000);
        assert_eq!(sector_address(511), 0x3F_F000);
    }

    #[test]
    fn the_rest_fill_the_space_below_the_checkpoints() {
        assert_eq!(sector_address(512), 0x0B_4000);
        assert_eq!(sector_address(513), 0x0B_5000);
        assert_eq!(sector_address(744), 0x19_C000);
        assert_eq!(sector_address(744) + SECTOR, SECOND.end);
    }

    /// No two indices share a sector, each sector lies whole within a
    /// stretch, and between them they cover both stretches.
    #[test]
    fn every_sector_is_its_own_and_within_the_ring() {
        let mut seen = BTreeSet::new();
        for index in 0..SECTORS {
            let address = sector_address(index);
            assert!(address.is_multiple_of(SECTOR));
            assert!(in_ring(address..address + SECTOR), "sector {index}");
            assert!(seen.insert(address), "sector {index} shares {address:#x}");
        }
        let bytes = (FIRST.end - FIRST.start) + (SECOND.end - SECOND.start);
        assert_eq!(seen.len() as u32 * SECTOR, bytes);
    }

    /// The flash's other tenants (see `spi_flash`'s map), as numbers of
    /// this test's own rather than the firmware's constants (which
    /// `field_log`, `dfu` and `system_off` check at compile time): the
    /// bootloader's logo and recovery firmware, MCUBoot's secondary slot,
    /// the crash checkpoints and the System OFF record. No sector of the
    /// ring overlaps any of them.
    #[test]
    fn no_sector_touches_the_slot_or_the_saved_activity() {
        assert_eq!(dfu_slot::SLOT_START + dfu_slot::SLOT_SIZE, 0x0B_4000);
        let others = [
            0x00_0000..0x04_0000u32,
            dfu_slot::SLOT_START..dfu_slot::SLOT_START + dfu_slot::SLOT_SIZE,
            0x19_D000..0x1F_D000,
            0x1F_D000..0x20_0000,
        ];
        for index in 0..SECTORS {
            let sector = sector_address(index)..sector_address(index) + SECTOR;
            for other in &others {
                assert!(
                    sector.end <= other.start || other.end <= sector.start,
                    "sector {index} ({sector:#x?}) overlaps {other:#x?}"
                );
            }
            assert!(sector.end <= 0x40_0000, "past the chip's end");
        }
    }

    /// An index past the ring (a slip in the arithmetic, a fetcher's
    /// request) counts round it rather than reaching anything else.
    #[test]
    fn an_index_past_the_ring_stays_in_it() {
        for index in [SECTORS, SECTORS + 1, 1024, 0xFFFF, u32::MAX - 1, u32::MAX] {
            let address = sector_address(index);
            assert!(in_ring(address..address + SECTOR), "index {index}");
            assert_eq!(address, sector_address(index % SECTORS));
        }
    }

    #[test]
    fn the_writer_goes_round_every_sector_in_order() {
        assert_eq!(next(510), 511);
        assert_eq!(next(511), 512);
        assert_eq!(next(743), 744);
        assert_eq!(next(744), 0);
        let mut index = 0;
        for expected in (0..SECTORS).chain(0..SECTORS) {
            assert_eq!(index, expected);
            index = next(index);
        }
    }

    #[test]
    fn the_size_query_is_no_sector() {
        assert!(u32::from(SIZE_QUERY) >= SECTORS);
    }

    // ==========================================================================
    // The writer, across an update from the old ring
    // ==========================================================================

    const MAGIC: u32 = u32::from_le_bytes(*b"urL1");

    /// The flash as `field_log` sees it: each sector's first two words
    /// (its magic and sequence number, if it's the log's), by address.
    /// With the writer's side of it: `newest` and `open`, as `field_log`
    /// has them, over this instead of the chip.
    struct Flash {
        headers: BTreeMap<u32, (u32, u32)>,
        /// Every address erased, in order.
        erased: Vec<u32>,
    }

    impl Flash {
        /// A watch that ran the old firmware until its ring had wrapped,
        /// its newest sector `newest` (numbered `sequence`), and
        /// InfiniTime before that: what's now the second stretch holds
        /// `stale` at the start of each sector.
        fn updated_from_the_old_ring(newest: u32, sequence: u32, stale: (u32, u32)) -> Self {
            let mut headers = BTreeMap::new();
            for index in 0..OLD_SECTORS {
                // Sectors after the newest are a lap older.
                let behind = (newest + OLD_SECTORS - index) % OLD_SECTORS;
                headers.insert(OLD_START + index * SECTOR, (MAGIC, sequence - behind));
            }
            let mut address = SECOND.start;
            while address < SECOND.end {
                headers.insert(address, stale);
                address += SECTOR;
            }
            Self {
                headers,
                erased: Vec::new(),
            }
        }

        /// `field_log::newest`.
        fn newest(&self) -> Option<(u32, u32)> {
            let mut newest: Option<(u32, u32)> = None;
            for index in 0..SECTORS {
                let Some(&(magic, sequence)) = self.headers.get(&sector_address(index)) else {
                    continue;
                };
                if magic != MAGIC {
                    continue;
                }
                if newest.is_none_or(|(_, newest)| sequence > newest) {
                    newest = Some((index, sequence));
                }
            }
            newest
        }

        /// `field_log::open`.
        fn open(&mut self, index: u32, sequence: u32) {
            let address = sector_address(index);
            self.erased.push(address);
            self.headers.insert(address, (MAGIC, sequence));
        }

        /// `field_log::sector_info`, as far as a fetcher's order goes: the
        /// sequence numbers of the sectors present, by index.
        fn present(&self) -> Vec<(u32, u32)> {
            (0..SECTORS)
                .filter_map(|index| match self.headers.get(&sector_address(index)) {
                    Some(&(MAGIC, sequence)) => Some((index, sequence)),
                    _ => None,
                })
                .collect()
        }
    }

    /// A boot of the new firmware on such a watch carries on from the old
    /// ring's newest sector, whatever the second stretch holds: zeros,
    /// erased flash, or a number larger than any of ours where a sequence
    /// number would be.
    #[test]
    fn the_writer_carries_on_from_the_old_ring() {
        for stale in [(0, 0), (u32::MAX, u32::MAX), (0x7572_4C31, u32::MAX)] {
            for newest in [0, 100, 510, 511] {
                let flash = Flash::updated_from_the_old_ring(newest, 9000, stale);
                assert_eq!(flash.newest(), Some((newest, 9000)));
                // None of the stale sectors pass for log, to a fetcher.
                assert_eq!(flash.present().len() as u32, OLD_SECTORS);
            }
        }
    }

    /// From there the writer fills the rest of the first stretch, then the
    /// second (erasing each sector of it before its first use), and only
    /// then comes back to the old ring's oldest sectors: so nothing of the
    /// old log is lost before the ring is full, the sequence numbers run on
    /// in index order, and every erase is of one of the ring's sectors.
    #[test]
    fn the_old_log_is_kept_until_the_ring_is_full() {
        let newest = 100;
        let mut flash = Flash::updated_from_the_old_ring(newest, 9000, (0xDEAD_BEEF, u32::MAX));
        let (mut sector, mut sequence) = flash.newest().expect("the old ring's");
        // One lap less the old log still held after the first sector the
        // new firmware opens (which is the old ring's oldest).
        for opened in 1..=SECTORS {
            sector = next(sector);
            sequence += 1;
            flash.open(sector, sequence);
            // A fetch reads the sectors present from the one after the
            // newest, round the ring: their numbers must only go up.
            let present = flash.present();
            let start = present
                .iter()
                .position(|&(index, _)| index == sector)
                .expect("just opened");
            let in_order: Vec<u32> = present[start + 1..]
                .iter()
                .chain(&present[..=start])
                .map(|&(_, sequence)| sequence)
                .collect();
            assert!(in_order.windows(2).all(|pair| pair[0] + 1 == pair[1]));
            // The old log shrinks only by the sectors reused: those after
            // the old newest first, then (a lap on) from index 0.
            let old = present.iter().filter(|&&(_, s)| s <= 9000).count() as u32;
            let reused_old = if opened <= OLD_SECTORS - 1 - newest {
                opened
            } else if opened <= SECTORS - 1 - newest {
                OLD_SECTORS - 1 - newest
            } else {
                opened - (SECTORS - OLD_SECTORS)
            };
            assert_eq!(old, OLD_SECTORS - reused_old, "after {opened} sectors");
        }
        assert_eq!(sector, newest);
        assert_eq!(flash.present().len() as u32, SECTORS);
        // Every sector was erased once, each within the ring.
        assert_eq!(flash.erased.len() as u32, SECTORS);
        for &address in &flash.erased {
            assert!(in_ring(address..address + SECTOR));
        }
        assert_eq!(
            flash.erased.iter().collect::<BTreeSet<_>>().len() as u32,
            SECTORS
        );
    }
}

/// Proofs, for `cargo kani`: what the tests above check for the ring's
/// indices, for every `u32`. Each is plain arithmetic, without loops.
#[cfg(kani)]
mod proofs {
    use super::log_ring::*;
    use super::{OLD_SECTORS, OLD_START, dfu_slot, in_ring};

    /// For every index, in the ring or not: its sector starts on a sector
    /// boundary and lies whole within one of the ring's two stretches (so
    /// erasing it, or programming or reading anywhere in it, touches
    /// nothing else in the flash), which are clear of MCUBoot's secondary
    /// slot below and end within the chip.
    #[kani::proof]
    fn every_sector_stays_in_the_ring() {
        let index: u32 = kani::any();
        let address = sector_address(index);
        assert!(address.is_multiple_of(SECTOR));
        assert!(in_ring(address..address + SECTOR));
        assert!(dfu_slot::SLOT_START + dfu_slot::SLOT_SIZE <= address);
        assert!(address + SECTOR <= 0x40_0000);
        // Not in the crash checkpoints or the System OFF record, which
        // lie between the stretches (`system_off` checks where).
        assert!(address + SECTOR <= 0x19_D000 || 0x20_0000 <= address);
        kani::cover!(
            address + SECTOR == SECOND.end,
            "the last below the checkpoints"
        );
        kani::cover!(index >= SECTORS, "an index past the ring");
    }

    /// For every index of the old ring: its sector is where builds up to
    /// 0.11.2 put it.
    #[kani::proof]
    fn the_old_ring_stays_put() {
        let index: u32 = kani::any();
        kani::assume(index < OLD_SECTORS);
        assert_eq!(sector_address(index), OLD_START + index * SECTOR);
        kani::cover!(index == OLD_SECTORS - 1, "the old ring's last");
    }

    /// For every two different indices of the ring: their sectors don't
    /// overlap.
    #[kani::proof]
    fn no_two_indices_share_a_sector() {
        let (a, b): (u32, u32) = kani::any();
        kani::assume(a < SECTORS && b < SECTORS && a != b);
        let (at_a, at_b) = (sector_address(a), sector_address(b));
        assert!(at_a.abs_diff(at_b) >= SECTOR);
        kani::cover!(at_a.abs_diff(at_b) == SECTOR, "neighbours");
        kani::cover!(
            (at_a < FIRST.start) != (at_b < FIRST.start),
            "one in each stretch"
        );
    }

    /// For every index: the next is in the ring, one further on, and from
    /// the last the first.
    #[kani::proof]
    fn next_stays_in_the_ring() {
        let index: u32 = kani::any();
        let after = next(index);
        assert!(after < SECTORS);
        if index < SECTORS - 1 {
            assert_eq!(after, index + 1);
        }
        if index == SECTORS - 1 {
            assert_eq!(after, 0);
        }
        kani::cover!(index == SECTORS - 1, "the last, back to the first");
        kani::cover!(index >= SECTORS, "an index past the ring");
    }
}
