//! The record that carries the newest of the activity history across
//! System OFF (see [`crate::system_off`]) and a crash: its format, which
//! minutes go in it, and which of the saved records a boot restores.
//!
//! Hardware-free, so it's tested on the host (`host-tests/snapshot`).
//!
//! The record holds the minutes that aren't in the activity log yet (see
//! [`crate::spill`], which has every minute a fetch could have sent):
//! those a fetch would hold back for now, a night under way above all
//! (see [`crate::sleep_hold`]), and the bucket under way. Their kinds are
//! still being revised, minute by minute, which is why they're rewritten
//! whole rather than appended. (Up to 0.11.3-rc.4 there was no log, and
//! the record held everything the phone hadn't fetched, up to the day
//! the history held then: 8.6 KB rewritten every two minutes by the time the
//! phone had been away for a day, and nothing older.)
//!
//! There are two kinds of record, in the same format:
//!
//! - **The planned record**, one, written right before the watch resets
//!   on purpose (an update, System OFF), in place of the last.
//! - **Checkpoints** (see below), written every
//!   [`CHECKPOINT_MINUTES`] while the clock runs, in slots taken in turn,
//!   for a reset nobody planned: a crash, or the watchdog. RAM can't carry
//!   the history across one, as the bootloader, which runs at every
//!   reset, overwrites all but the top few KB of our statics (see
//!   `memory.x`), and the history alone is 5.2 KB.
//!
//! ## Checkpoints
//!
//! A checkpoint is the planned record's save, every [`CHECKPOINT_MINUTES`],
//! into the next of [`CHECKPOINT_SLOTS`] slots. A boot restores the newest
//! usable record of them all ([`newer_checkpoint`]), or the planned record
//! if no checkpoint is newer.
//!
//! - A save cut short (by the crash itself, say) leaves its slot without a
//!   record, as the header goes last; the slot before still holds the
//!   previous checkpoint, a few minutes older.
//! - Each save erases only the sectors its record needs ([`erase_bytes`]):
//!   one, unless a fetch is holding back more than ~11 hours (a watch
//!   left lying still, taken for asleep). With 32 slots, each sector is
//!   erased at most every 64 minutes, 23 times a day: a flash rated for
//!   100,000 erases lasts 12 years. By day a record is a few dozen
//!   bytes, and the erase (~50 ms) is nearly all a save costs.
//!
//!   TODO: by day, several checkpoints would fit one sector, one after
//!   the other, for an erase every hour or so instead of every two
//!   minutes; a boot would then look for records within a slot too.
//! - What's in a checkpoint is what the history held (minus what the log
//!   has): a crash that scribbled over the history before the checkpoint
//!   puts back the same scribbles, no worse than the history the firmware
//!   was already using. Every 6-byte sample is a valid one (plain integers,
//!   and flags whose unknown values read as unknown), and the history files
//!   any minute, so nothing restored can index out of bounds; what's
//!   checked is that the record is whole (its checksum), sensibly long, and
//!   from this side of the last planned reset and within a day of the
//!   clock ([`newer_checkpoint`]: a day, though the history holds less,
//!   as what it can't hold of a record goes into the log).
//! - A checkpoint is never retired: the next one supersedes it, holding
//!   whatever it held that hasn't gone into the log since.
//!
//! The record is a 32-byte header followed by one 6-byte sample per minute
//! (the in-RAM [`crate::activity::Sample`], verbatim) from the first minute
//! after the log's last to the newest (one with none starts just after
//! the newest: see [`empty_from`]):
//!
//! | Offset | Size | Field |
//! |---:|---:|---|
//! | 0 | 4 | magic, `urH1` |
//! | 4 | 4 | first minute (UTC minutes since 1970) |
//! | 8 | 2 | minutes |
//! | 10 | 2 | reserved (erased) |
//! | 12 | 4 | today's local date (days since 1970, or [`UNKNOWN_DAY`]) |
//! | 16 | 4 | today's steps |
//! | 20 | 4 | checksum (FNV-1a over the samples, then bytes 4..20) |
//! | 24 | 4 | consumed: erased while fresh, 0 once restored and synced |
//! | 28 | 4 | synced: `sync` if the phone's copy ends just before the first minute, else erased |
//!
//! The header is written last, so a save cut short leaves no valid record.
//! The consumed word is cleared in place (NOR flash can always clear bits),
//! so retiring a record costs no erase.
//!
//! The synced word says the watch knew where the phone's copy of the
//! history ended, from a fetch it answered (see
//! `system_off::synced_through`), so the next fetch can start at the
//! record's first minute. (Since the log, that's noted there, at each
//! fetch, and a boot goes by the later of the two: the word matters for
//! a record an older build saved before an update, and for an older
//! build restoring this one's.) Without it, the phone may already have some of
//! the record's minutes, and the next fetch starts after what the phone
//! asks for (see [`crate::gb_buckets::start`]). It came after the rest,
//! which is why it's outside the checksum: older builds restore a record
//! with it, and a record from one, which leaves it erased, reads as not
//! knowing (as it didn't: those builds sent a sample a minute, which
//! Gadgetbridge may hold). Only the exact word counts, so a stray change
//! to it only ever forgets.

/// The header's size, before the samples.
pub const HEADER_BYTES: usize = 32;
/// One minute's sample.
pub const SAMPLE_BYTES: usize = 6;
/// The most minutes a record holds: a day, which was the whole in-RAM
/// history up to 0.11.3-rc.4, whose records hold all of it that the phone
/// hadn't fetched. A boot must still read those (after an update from
/// such a build: see `system_off::restore`, which moves what the history
/// no longer has room for into the activity log), so this stays, though
/// what's saved now is at most the history's 14½ hours
/// (`minute_ring::MINUTES`), and nearly always far less.
pub const MAX_MINUTES: usize = 24 * 60;
/// The whole record at its largest.
pub const MAX_BYTES: usize = HEADER_BYTES + MAX_MINUTES * SAMPLE_BYTES;

const MAGIC: u32 = u32::from_le_bytes(*b"urH1");
/// Where the consumed word is, from the record's start.
pub const CONSUMED_OFFSET: u32 = 24;
/// What the consumed word becomes once the record has done its job.
pub const CONSUMED: u32 = 0;
/// Where the synced word is, from the record's start.
const SYNCED_OFFSET: usize = 28;
/// The synced word, when the phone's copy ends just before the first
/// minute.
const SYNCED: u32 = u32::from_le_bytes(*b"sync");
/// An erased word: a fresh record's consumed word.
const ERASED: u32 = u32::MAX;

/// Today's date when the clock wasn't known (steps can still have been
/// counted then).
pub const UNKNOWN_DAY: i32 = i32::MIN;

/// FNV-1a's offset basis: where [`checksum`] starts, over the samples.
pub const CHECKSUM_SEED: u32 = 0x811C_9DC5;

/// FNV-1a, continued over `bytes`: enough to tell a record from leftover
/// or half-written flash, which is all it's for.
pub fn checksum(mut hash: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    hash
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// The first sample's UTC minute since 1970 (32 bits last until the
    /// year 10136).
    pub first_minute: u32,
    pub minutes: u16,
    /// Local days since 1970, or [`UNKNOWN_DAY`].
    pub day: i32,
    pub steps: u32,
    pub checksum: u32,
    /// Whether the phone's copy of the history ends just before
    /// `first_minute`, as far as the watch knew (see the module docs).
    pub synced: bool,
}

impl Header {
    /// The record's checksum, given the samples' (see [`checksum`]): that,
    /// continued over the header's own fields. (Samples first, so a writer
    /// can fill in the header after them.)
    pub fn expected_checksum(&self, samples: u32) -> u32 {
        checksum(samples, &self.to_bytes()[4..20])
    }

    pub fn to_bytes(self) -> [u8; HEADER_BYTES] {
        let mut bytes = [0xFF; HEADER_BYTES];
        bytes[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.first_minute.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.minutes.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.day.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.steps.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.checksum.to_le_bytes());
        if self.synced {
            bytes[SYNCED_OFFSET..][..4].copy_from_slice(&SYNCED.to_le_bytes());
        }
        bytes
    }

    /// A fresh record's header, or `None` for anything else: erased flash,
    /// a consumed record, or nonsense. The samples' checksum is still the
    /// caller's to check.
    pub fn from_bytes(bytes: &[u8; HEADER_BYTES]) -> Option<Self> {
        let word = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().expect("4 bytes"));
        if word(0) != MAGIC || word(CONSUMED_OFFSET as usize) != ERASED {
            return None;
        }
        let header = Self {
            first_minute: word(4),
            minutes: u16::from_le_bytes([bytes[8], bytes[9]]),
            day: word(12) as i32,
            steps: word(16),
            checksum: word(20),
            synced: word(SYNCED_OFFSET) == SYNCED,
        };
        (usize::from(header.minutes) <= MAX_MINUTES).then_some(header)
    }
}

/// The first minute worth saving, given the newest minute in the
/// history, the last minute the activity log is complete up to (if it
/// holds any; see [`crate::spill`]), and how many minutes the record has
/// room for.
///
/// The log has every minute a fetch could have sent, so the record starts
/// after it: with what a fetch holds back, and the bucket under way. A
/// log that ends after the newest minute says nothing of the minutes the
/// history holds now, as the clock has gone back since (see
/// [`crate::minute_ring`]); nor does no log. Then all of the history
/// goes in the record, as before there was a log, as far as there's room.
pub fn unlogged_from(newest: i64, logged: Option<i64>, room: usize) -> i64 {
    let oldest = newest - room as i64 + 1;
    match logged.filter(|&logged| logged <= newest) {
        Some(logged) => (logged + 1).max(oldest),
        None => oldest,
    }
}

impl Header {
    /// The minute the record was saved as of: the one after the newest
    /// minute the history had then. Every record runs to that minute (an
    /// empty one starts there; see [`empty_from`]), so this orders them by
    /// when they were saved, as far as the clock knew.
    pub fn as_of(&self) -> u32 {
        self.first_minute.saturating_add(u32::from(self.minutes))
    }
}

/// Where a record with no minutes starts, given the history's newest
/// minute: just after it, so that its [`Header::as_of`] still says when it
/// was saved, and it outranks every older record. (Before the clock was
/// ever known there's no newest minute, and it starts at 0.)
pub fn empty_from(newest: Option<i64>) -> u32 {
    newest.map_or(0, |newest| {
        (newest + 1).clamp(0, i64::from(u32::MAX)) as u32
    })
}

/// The as-of minute (see [`Header::as_of`]) of any record with the magic
/// and a sensible length, fresh or consumed: for the planned record, which
/// says, even after it's done its job, when the last planned reset was.
pub fn as_of_any(bytes: &[u8; HEADER_BYTES]) -> Option<u32> {
    let mut fresh = *bytes;
    let consumed = CONSUMED_OFFSET as usize;
    fresh[consumed..consumed + 4].copy_from_slice(&ERASED.to_le_bytes());
    Header::from_bytes(&fresh).map(|header| header.as_of())
}

// ==============================================================================
// Checkpoints (see the module docs)
// ==============================================================================

/// How often, in minutes of the clock, the activity is checkpointed (at
/// each minute that's a multiple of it). A crash loses the minutes since
/// the last one: at most this, and the reset's few seconds. Each costs an
/// erase and a write of a sector or so of the SPI flash, ~0.3 µAh: ~10 µA
/// averaged over a day, 1.5% of the watch's ~0.7 mA. The wearer would
/// rather spend the battery than lose readings.
pub const CHECKPOINT_MINUTES: i64 = 2;
/// How many slots the checkpoints take turns in (see the module docs).
pub const CHECKPOINT_SLOTS: u32 = 32;
/// The SPI flash's erase sector (`spi_flash::SECTOR`).
pub const SECTOR_BYTES: u32 = 4096;
/// A slot: two sectors, 22 hours of minutes. (Three, room for the whole
/// history, up to 0.11.3-rc.4, when a checkpoint held all the phone
/// hadn't fetched.)
pub const SLOT_BYTES: u32 = 2 * SECTOR_BYTES;

/// Whether a checkpoint is due as the clock reaches `minute` (UTC minutes
/// since 1970).
pub fn checkpoint_due(minute: i64) -> bool {
    minute.rem_euclid(CHECKPOINT_MINUTES) == 0
}

/// The slot (of [`CHECKPOINT_SLOTS`]) for a checkpoint whose newest minute
/// is `newest`: the next one in turn every [`CHECKPOINT_MINUTES`], so a
/// save never overwrites the checkpoint before it (unless the clock moved
/// back by a multiple of the slots' span, when it's a few minutes older).
/// From the minute rather than a counter, which would take RAM.
pub fn checkpoint_slot(newest: i64) -> u32 {
    newest
        .div_euclid(CHECKPOINT_MINUTES)
        .rem_euclid(i64::from(CHECKPOINT_SLOTS)) as u32
}

/// How much a save of `minutes` erases: the record, to whole sectors.
pub fn erase_bytes(minutes: u16) -> u32 {
    ((HEADER_BYTES + usize::from(minutes) * SAMPLE_BYTES) as u32).next_multiple_of(SECTOR_BYTES)
}

/// Whether a checkpoint saved as of `as_of` (see [`Header::as_of`]), whose
/// samples check out, is a better one to restore than `best` (the as-of
/// of the best found so far, if any):
///
/// - newer than `best`, so the newest wins (a tie keeps the first);
/// - newer than the planned record (`floor`, its as-of, fresh or not; see
///   [`as_of_any`]): the planned record was saved after every checkpoint
///   of the run before it, so an older checkpoint is from before that
///   update or System OFF, which the planned record has carried on (and
///   the phone has fetched, if it's been retired). Unless the planned
///   record is itself dated ahead of a known clock: then it was saved by
///   a clock that was ahead (a phone's that was wrong, since put right),
///   and says nothing of the checkpoints saved since, all of them dated
///   before it. Held to it, none would be restored after a crash until
///   the clock caught up with that date, days perhaps (a dev kit's
///   planned record from a test, 10 days ahead, did just that). The cost
///   of not holding to it: until the run's first checkpoint, a couple of
///   minutes at most, a crash restores one from before that reset, if
///   it's under a day old, whose minutes the log has anyway and whose
///   steps today are from then;
/// - if the clock is known (`now`, UTC minutes since 1970: after a crash,
///   it's resumed), not ahead of it (it was saved before the reset, as of
///   the minute after its newest, and the clock resumes to within about a
///   second of where it was), nor a day old or more: no build's history
///   ever held minutes that old. Before the clock is known (after a
///   power-on), any is, as with the planned record.
pub fn newer_checkpoint(
    as_of: u32,
    best: Option<u32>,
    floor: Option<u32>,
    now: Option<i64>,
) -> bool {
    let as_of_minute = i64::from(as_of);
    // With the checkpoints' own allowance for a clock resumed a little
    // behind.
    let floor = floor.filter(|&floor| now.is_none_or(|now| i64::from(floor) <= now + 2));
    best.is_none_or(|best| as_of > best)
        && floor.is_none_or(|floor| as_of > floor)
        && now.is_none_or(|now| as_of_minute <= now + 2 && as_of_minute > now - MAX_MINUTES as i64)
}
