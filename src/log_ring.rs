//! Where the field log's sectors are in the SPI flash: the arithmetic of
//! its ring, apart from [`crate::field_log`]'s writing and the flash itself.
//!
//! The ring is one run of sector indices over two stretches of the flash
//! that don't touch: the top 2 MiB, which was all of it up to 0.11.2, and
//! (since 0.11.3) the space the bootloader leaves free below our saved
//! activity. Between them, and below the second, are the crash checkpoints,
//! the System OFF record and MCUBoot's secondary slot (see
//! [`crate::spi_flash`] for the map): the log erases a sector every few
//! minutes, for good, so an index that mapped into any of those would in
//! time erase an update half-way in, or the activity a crash should have
//! kept. So every address the log erases, programs or reads comes from
//! [`sector_address`], which is hardware-free, with host tests and proofs
//! (`host-tests/log_ring`) that it stays in the two stretches for every
//! index.

use core::ops::Range;

/// The SPI flash's erase sector, as [`crate::spi_flash`] has it
/// (`field_log` checks they agree): written out here so this module builds
/// without the driver.
pub const SECTOR: u32 = 4096;

/// The ring's first stretch, the top half of the 4 MiB flash: sectors 0 to
/// 511. This was the whole ring up to 0.11.2, and those sectors stay where
/// they were, so a watch updated from such a build keeps its log, and the
/// writer carries on from its newest sector.
pub const FIRST: Range<u32> = 0x20_0000..0x40_0000;

/// The ring's second stretch, sectors 512 and up: from the end of MCUBoot's
/// secondary slot to the start of the crash checkpoints (`field_log` checks
/// both), which nothing else of ours uses. InfiniTime keeps its file system
/// here, so on a watch that ran it these sectors start out holding that:
/// see [`crate::field_log`] for why it's never taken for log.
pub const SECOND: Range<u32> = 0x0B_4000..0x19_D000;

const FIRST_SECTORS: u32 = (FIRST.end - FIRST.start) / SECTOR;
const SECOND_SECTORS: u32 = (SECOND.end - SECOND.start) / SECTOR;

/// How many sectors the ring has: about 3 MB of log. 0.11.3-rc.2 logged
/// 28 KB an hour on a worn watch (32 to 40 by day, ~14 asleep), which
/// would fill it in 4.5 days; without the still wrist's samples through
/// heart rate sessions, 13% of that log (see [`crate::log_pack`] for
/// these and what else keeps it small), it's ~24.5 KB an hour, 5 days.
pub const SECTORS: u32 = FIRST_SECTORS + SECOND_SECTORS;

/// Not a sector: the index a fetcher asks for to learn [`SECTORS`] (see
/// `LogService` in [`crate::ble`]).
pub const SIZE_QUERY: u16 = u16::MAX;

// Both stretches are whole sectors (so erasing one of the ring's sectors
// touches nothing else), and they don't overlap (so no two indices share a
// sector). The first 512 indices are the first stretch's, as they were
// when it was the whole ring.
const _: () = assert!(FIRST.start.is_multiple_of(SECTOR) && FIRST.end.is_multiple_of(SECTOR));
const _: () = assert!(SECOND.start.is_multiple_of(SECTOR) && SECOND.end.is_multiple_of(SECTOR));
const _: () = assert!(SECOND.start < SECOND.end && SECOND.end <= FIRST.start);
const _: () = assert!(FIRST.start < FIRST.end);
const _: () = assert!(FIRST_SECTORS == 512 && SECTORS > 1);
// A fetcher names a sector in 16 bits, and the size query isn't one.
const _: () = assert!(SECTORS <= SIZE_QUERY as u32);
// Every index's sector lies whole within its stretch: checked for each one
// at compile time, as the proofs in `host-tests/log_ring` do for every
// `u32`.
const _: () = {
    let mut index = 0;
    while index < SECTORS {
        let address = sector_address(index);
        assert!(address.is_multiple_of(SECTOR));
        let (stretch, base) = if index < FIRST_SECTORS {
            (FIRST, index)
        } else {
            (SECOND, index - FIRST_SECTORS)
        };
        assert!(stretch.start <= address && address + SECTOR <= stretch.end);
        assert!(address == stretch.start + base * SECTOR);
        index += 1;
    }
};

/// Where sector `index` starts in the SPI flash.
///
/// For every `index`, not only those below [`SECTORS`]: one past the ring
/// counts round it again, so that no slip in a caller's arithmetic (nor a
/// fetcher's request) gets an address outside the ring.
pub const fn sector_address(index: u32) -> u32 {
    let index = index % SECTORS;
    if index < FIRST_SECTORS {
        FIRST.start + index * SECTOR
    } else {
        SECOND.start + (index - FIRST_SECTORS) * SECTOR
    }
}

/// The sector the writer moves on to after `index`: the next index, and
/// from the last back to the first.
// Only the writer moves on, and only field builds have one.
#[allow(dead_code)]
pub const fn next(index: u32) -> u32 {
    (index % SECTORS + 1) % SECTORS
}
