//! System OFF: the nRF52832's deepest sleep, where nothing runs and only a
//! pin (or a reset) wakes the chip, which then boots from scratch. The
//! watch goes there for two reasons ([`Reason`]):
//!
//! - **The battery is critically low** (see [`crate::battery`]): rather
//!   than run on until it browns out, losing the day's activity the phone
//!   hasn't fetched yet, the watch saves that and switches itself off.
//!   Plugging in the charger wakes it.
//! - **Storage mode**, chosen on the info screen (see [`crate::ui`]), for a
//!   watch put away for weeks: the same, but the button wakes it too.
//!
//! Switching off ([`shut_down`]) shows why for a few seconds, then, in the
//! dark:
//!
//! 1. The history the phone hasn't fetched, and today's steps, go into a
//!    record in the SPI flash (format in [`crate::snapshot`]), written by
//!    the activity task (see [`serve`]). The field log is written out too.
//! 2. Everything on the board that would keep drawing current is put to
//!    sleep: the heart-rate sensor (its LED, if a measurement was running),
//!    the accelerometer, the touch controller, the display, the SPI flash.
//! 3. The wake pins are armed, and the chip goes off.
//!
//! Waking is a reset: the bootloader runs, then ur boots as after any
//! reset, [`restore`]s the record into the history, and the phone fetches
//! it with the rest at its next sync, after which the record is retired
//! ([`on_synced`]), so it can't be restored twice. The clock doesn't run
//! while off, so the time is unknown until the phone sets it (see
//! [`crate::clock::forget`]).
//!
//! The same record carries the activity across a deliberate reset, such
//! as installing an update ([`save_before_reset`]): the history lives in
//! RAM that every reset loses, and today's steps in RAM that survives an
//! ordinary reset but not, it turns out, the bootloader swapping in an
//! update (a field log's update lost the day's 202 steps).
//!
//! And, every couple of minutes, checkpoints of it carry it across a
//! reset nobody planned: a crash, or the watchdog ([`checkpoint`]; see
//! [`crate::snapshot`]). A boot restores whichever record is newest. (A
//! hard fault on a sealed watch, mid-night, once lost the night's sleep
//! and five hours of heart rate and steps.)
//!
//! Neither kind of record holds all the phone hasn't fetched, only its
//! newest part: the minutes a fetch would hold back for now (a night
//! under way; see [`crate::sleep_hold`]) and the bucket under way. The
//! rest, every minute a fetch could have sent, is in the activity log
//! (see [`crate::spill`]), which this module also keeps ([`serve`]): it
//! appends those minutes as they become fetchable, notes each fetch the
//! phone completes, and takes back what a clock set back undoes. A boot
//! reads the log back into the history, then the newest record on top;
//! and a fetch reads the log itself, so that what the phone didn't fetch
//! for days still reaches it ([`log_open`], [`log_bucket`]).
//!
//! What the watch draws while off, roughly (E: estimate, D: datasheet,
//! M: measured by others; nothing here is measured on this watch):
//!
//! | | µA | |
//! |---|---:|---|
//! | nRF52832 in System OFF, no RAM retention | 0.3 | D |
//! | Button powered, so it can wake the watch (storage mode only) | 34 | M (PINE64 wiki) |
//! | Display controller asleep | ~10 | E |
//! | Accelerometer suspended, touch controller in deep sleep | a few | E |
//! | SPI flash in deep power-down | 1 | D |
//! | Battery voltage divider | 2 | D |
//! | Regulator, charger, leakage | ~15 | E |
//!
//! So ~30 µA off a critically low battery, whose ~4 mAh then last days
//! before the cell's protector cuts it off, and ~65 µA in storage mode:
//! about 40% of a full charge over six weeks. The button costs half of
//! that, which is why a critically low battery doesn't keep it powered:
//! only the charger can help an empty watch, and the button would run it
//! down to the protector twice as fast.
//!
//! No RAM is kept while off. Retention would cost next to nothing (~25 nA
//! for the 4 KiB section holding `memory.x`'s retained block: Nordic gives
//! 0.3 µA without, 0.7 µA with all 64 KiB), but what that block holds is
//! no use after System OFF: the clock would be stale, today's steps are in
//! the record anyway, and a breadcrumb only matters after a crash.

use core::ops::Range;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use defmt::{debug, info, warn};
use embassy_nrf::pac;
use embassy_nrf::wdt::WatchdogHandle;
use embassy_time::{Duration, Timer};

use crate::canvas::Canvas;
use crate::display::Display;
use crate::snapshot::{self, HEADER_BYTES, Header, SAMPLE_BYTES};
use crate::spi_flash::{self, FLASH};
use crate::{Backlight, activity, clock, field_log, gb_buckets, heart, i2c, spi, spill, touch};

/// Why the watch is switching off.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Reason {
    BatteryEmpty,
    Storage,
}

/// Where the record lives in the SPI flash: three sectors just below the
/// field log's first stretch, in space the bootloader doesn't use (see
/// [`crate::spi_flash`]).
pub(crate) const RECORD: Range<u32> = 0x1F_D000..0x20_0000;
const _: () = assert!(snapshot::MAX_BYTES as u32 <= RECORD.end - RECORD.start);

/// Where the checkpoints are (see [`crate::snapshot`]'s Checkpoints): their
/// slots, one after the other, just below the planned record, in space
/// the bootloader doesn't use (see [`crate::spi_flash`]), and just above
/// the activity log.
pub(crate) const CHECKPOINTS: Range<u32> = 0x1B_D000..0x1F_D000;

/// Where the activity log is (see [`crate::spill`]): 32 sectors, 11 days
/// of minutes, just above the field log's second stretch. Up to
/// 0.11.3-rc.4 the checkpoints had these sectors too, in slots of three:
/// a week of minutes needs 21, and the checkpoints, now that they only
/// hold what a fetch holds back, fit slots of two, as many as before (so
/// each sector's wear is what it was). The field log keeps its 745.
pub(crate) const SPILL: Range<u32> = 0x19_D000..0x1B_D000;
const LOG: spill::Ring = spill::Ring {
    start: SPILL.start,
    sectors: (SPILL.end - SPILL.start) / spi_flash::SECTOR,
};
const _: () = assert!(spill::SECTOR == spi_flash::SECTOR);
const _: () = assert!(SPILL.start.is_multiple_of(spi_flash::SECTOR));
const _: () = assert!(SPILL.end == CHECKPOINTS.start && LOG.sectors == 32);
// A week of minutes, with a note of a fetch every bucket (six entries to
// five minutes), and the oldest sector about to be erased.
const _: () = assert!((LOG.sectors - 1) * (spill::SLOTS as u32 - 2) * 5 / 6 >= 7 * 24 * 60);
const _: () = assert!(
    CHECKPOINTS.end - CHECKPOINTS.start == snapshot::CHECKPOINT_SLOTS * snapshot::SLOT_BYTES
);
const _: () = assert!(CHECKPOINTS.end <= RECORD.start);
const _: () = assert!(CHECKPOINTS.start.is_multiple_of(spi_flash::SECTOR));
const _: () = assert!(crate::dfu_slot::SLOT_START + crate::dfu_slot::SLOT_SIZE <= SPILL.start);
const _: () = assert!(snapshot::SECTOR_BYTES == spi_flash::SECTOR);
const _: () = assert!(
    CHUNK >= HEADER_BYTES,
    "headers are staged in a chunk's buffer"
);
// The field log is on both sides of the three (see [`crate::log_ring`]):
// its second stretch ends where the activity log starts, and its first
// starts where the record ends.
const _: () = assert!(crate::log_ring::SECOND.end <= SPILL.start);
const _: () = assert!(RECORD.end <= crate::log_ring::FIRST.start);
const _: () = assert!(RECORD.start.is_multiple_of(spi_flash::SECTOR));

/// Samples go between the history and the flash this many bytes at a time
/// (six minutes, the least that also holds a header): a small buffer, since
/// it sits in a task's future for good, in a build with little RAM to
/// spare. The largest record then takes 145 small writes, about half a
/// second.
const CHUNK: usize = 6 * SAMPLE_BYTES;

/// The most minutes the activity log takes in at once, every couple of
/// minutes: a write each, a millisecond or two, with the activity task
/// (alone) waiting. A night that's just been settled is in within a few
/// minutes; a planned save takes whatever there is.
const SPILL_AT_ONCE: i64 = 6 * 60;
const _: () = assert!(SPILL_AT_ONCE % gb_buckets::MINUTES == 0);

/// The most minutes a checkpoint holds: its slot's worth, less a bucket
/// (it starts at a bucket's start). More than a fetch ever holds back,
/// so more than is ever outside the log, unless the log can't be written.
const CHECKPOINT_ROOM: usize =
    (snapshot::SLOT_BYTES as usize - HEADER_BYTES) / SAMPLE_BYTES - gb_buckets::MINUTES as usize;
const _: () = assert!(CHECKPOINT_ROOM as i64 > crate::sleep_hold::CAP + 2 * gb_buckets::MINUTES);

/// The activity log's end, as the activity task keeps it (in its future,
/// so in static RAM: 16 bytes) between the calls here that need it.
pub type Log = spill::Writer;

/// Whether [`SYNCED`] has changed since the log last noted it (see
/// [`keep`]).
static NOTE: AtomicBool = AtomicBool::new(false);

/// The first minute the log should let go of at [`keep`]'s next run and
/// take in again, or 0: see [`relog_from`].
static RELOG: AtomicU32 = AtomicU32::new(0);

/// Has the log take the minutes from `minute` on back, and in again as
/// the history has them now, at [`serve`]'s next run: for minutes changed
/// after they could be fetched, as a workout's first are, marked as
/// exercise only once it's recognised (see `activity`), and the minutes
/// a clock set back files again (`activity::History::at`). The log may
/// have them already, as they were, and a fetch reads the log.
///
/// What the phone has fetched by now stays as it got it: this is for the
/// phone that hasn't.
pub fn relog_from(minute: i64) {
    let minute = minute32(minute).max(1);
    let _ = RELOG.try_update(Ordering::Relaxed, Ordering::Relaxed, |from| {
        (from == 0 || minute < from).then_some(minute)
    });
    checkpoint();
}

/// A minute as the log has them: UTC minutes since 1970, in 32 bits.
pub fn minute32(minute: i64) -> u32 {
    minute.clamp(0, i64::from(u32::MAX)) as u32
}

// Straight through to the driver, with no `async fn` of their own in
// between: each would be a future around the driver's, and these are
// awaited from the Bluetooth task, where every byte of future counts.
impl spill::Flash for spi_flash::SpiFlash {
    type Error = spi_flash::Error;

    fn read(&mut self, address: u32, data: &mut [u8]) -> impl Future<Output = ()> {
        spi_flash::SpiFlash::read(self, address, data)
    }

    fn program(
        &mut self,
        address: u32,
        data: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>> {
        spi_flash::SpiFlash::program(self, address, data)
    }

    fn erase(&mut self, address: u32) -> impl Future<Output = Result<(), Self::Error>> {
        spi_flash::SpiFlash::erase(self, address..address + spi_flash::SECTOR)
    }
}

/// How long the switching-off screen shows.
const SHOW_FOR: Duration = Duration::from_secs(3);

/// Asks [`serve`] to save, which then says it has: [`SAVE_ASKED`],
/// [`SAVE_DONE`], or neither; or to save a checkpoint
/// ([`CHECKPOINT_ASKED`], back to neither once saved). The planned save
/// wins: once asked, it's done next, and [`SAVE_DONE`] stays, as the watch
/// resets or switches off right after, so no checkpoint starts in between
/// (which could wake the SPI flash again, and leave it drawing ~14 µA
/// while off).
static SAVE: AtomicU8 = AtomicU8::new(0);
const SAVE_ASKED: u8 = 1;
const SAVE_DONE: u8 = 2;
const CHECKPOINT_ASKED: u8 = 3;

/// Where this boot's restored record is: [`NONE`], [`RESTORED`] (the phone
/// hasn't fetched it yet), or [`RETIRING`] (it has; see [`retire`]).
static RECORD_STATE: AtomicU8 = AtomicU8::new(NONE);
const NONE: u8 = 0;
const RESTORED: u8 = 1;
const RETIRING: u8 = 2;

/// The newest minute (UTC minutes since 1970) the phone has fetched since
/// boot (or, until it has, where [`restore`] found the phone's copy to
/// end: the log's last note of a fetch, or the minute before a restored
/// record that knew, whichever is later), or 0 if the watch doesn't know:
/// what a save says of the phone's copy (see [`save`]), what the log
/// notes for the next boot (see [`NOTE`]), and where the next fetch starts
/// at the latest (see [`synced_through`]). Put back when the phone sets
/// the clock to before it (see [`on_clock_set`]).
static SYNCED: AtomicU32 = AtomicU32::new(0);

// ==============================================================================
// Saving and restoring
// ==============================================================================

/// Writes a record at `start` (the planned record's, or a checkpoint's
/// slot): today's steps, and the history from where the activity log
/// ends (`logged`, its last minute). Failures are logged; there's nothing
/// else to do about them with the battery running out, and a checkpoint's
/// next try is minutes away.
async fn save(flash: &mut spi_flash::SpiFlash, start: u32, logged: Option<u32>) {
    let newest = activity::newest_minute();
    let room = if start == RECORD.start {
        activity::HISTORY_MINUTES
    } else {
        CHECKPOINT_ROOM
    };
    let from = newest.map_or(0, |newest| {
        snapshot::unlogged_from(newest, logged.map(i64::from), room)
    });
    // Whether the phone has all there is before the record, so that a
    // boot that finds no note of it in the log still knows (see
    // `snapshot`): the log couldn't be written, or an older build boots.
    let synced = synced_through()
        .is_some_and(|synced| synced >= from - 1 && newest.is_some_and(|newest| synced <= newest));
    // From a bucket's start, as the planned record always has (older
    // builds, which may yet restore it, send what's restored in whole
    // buckets: see `gb_buckets`); the log has those minutes too, and the
    // restore keeps the log's.
    //
    // With nothing to save, the record starts after the newest minute, so
    // that it still says when it was saved (see `snapshot::empty_from`).
    let (first, minutes) = activity::span_from(gb_buckets::start_of(from))
        .unwrap_or((i64::from(snapshot::empty_from(newest)), 0));
    // Every local that lives across an await is static RAM, in the
    // activity task's future, so they're kept few and small.
    let (first, minutes) = (first as u32, minutes as u16);

    let written = async {
        // Only the sectors the record takes, the header's first: a save
        // cut short here leaves no record, never half of one.
        flash
            .erase(start..start + snapshot::erase_bytes(minutes))
            .await?;
        let mut hash = snapshot::CHECKSUM_SEED;
        let mut buffer = [0u8; CHUNK];
        let mut done = 0;
        while done < usize::from(minutes) {
            let n = (usize::from(minutes) - done).min(CHUNK / SAMPLE_BYTES);
            let bytes = &mut buffer[..n * SAMPLE_BYTES];
            activity::copy_samples(i64::from(first) + done as i64, bytes);
            hash = snapshot::checksum(hash, bytes);
            let address = start + (HEADER_BYTES + done * SAMPLE_BYTES) as u32;
            done += n;
            flash.write(address, bytes).await?;
        }
        // In a block of its own, so the header isn't kept (in the future)
        // across the write.
        {
            let (day, steps) = activity::steps_for_saving();
            let mut header = Header {
                first_minute: first,
                minutes,
                day: day.map_or(snapshot::UNKNOWN_DAY, |day| day as i32),
                steps,
                checksum: 0,
                synced,
            };
            header.checksum = header.expected_checksum(hash);
            // A checkpoint every couple of minutes, all day, would be most
            // of a field log's lines at `info`.
            if start == RECORD.start {
                info!(
                    "snapshot: saving {} minutes of activity from minute {}, and {} steps",
                    minutes, first, steps
                );
            } else {
                debug!(
                    "snapshot: checkpoint of {} minutes from minute {} at {=u32:#x}",
                    minutes, first, start
                );
            }
            // Last, so that a save cut short leaves no record at all.
            buffer[..HEADER_BYTES].copy_from_slice(&header.to_bytes());
        }
        flash.write(start, &buffer[..HEADER_BYTES]).await
    }
    .await;
    if let Err(e) = written {
        warn!("snapshot: saving at {=u32:#x} failed: {}", start, e);
        // With no record written, the checkpoints are all there is: none
        // is retired below for being newer than it.
        return;
    }
    // The record just written is the newest there is: every other was
    // saved before it. But a checkpoint can claim otherwise, saved while
    // the clock was ahead (a phone's that was wrong, since put right): its
    // minutes are in this record's future. A boot goes by the records'
    // dates, so it would restore that one instead:
    //
    // - After System OFF, with the clock unknown, in place of the planned
    //   record, which it would then retire unread.
    // - After a crash, with the clock known, once the clock has all but
    //   caught up with that date (see `snapshot::newer_checkpoint`, which
    //   turns down a checkpoint dated ahead of the clock, give or take a
    //   couple of minutes): in place of the newest checkpoint, and with
    //   it go the minutes since the clock was put right that the log
    //   doesn't have yet, a night under way among them. (A dev kit whose
    //   tests had set its clock a few minutes ahead and back lost a walk
    //   that way, at a reset as the clock passed those minutes.)
    //
    // So any such checkpoint is retired here, at every save: the history
    // files a clock set back at once, and asks for a checkpoint as it
    // does (`relog_from`), so they're gone within seconds of it. (32
    // headers read, ~2 ms with the flash awake anyway, next to the save's
    // own erase of ~50 ms.)
    let as_of = first.saturating_add(u32::from(minutes));
    let mut buffer = [0u8; CHUNK];
    for slot in 0..snapshot::CHECKPOINT_SLOTS {
        let other = CHECKPOINTS.start + slot * snapshot::SLOT_BYTES;
        if other != start
            && let Some(header) = read_header(flash, other, &mut buffer).await
            && snapshot::from_a_clock_ahead(header.as_of(), as_of)
        {
            warn!(
                "snapshot: retiring a checkpoint from minute {}, after now",
                header.as_of()
            );
            consume(flash, other).await;
        }
    }
}

/// Puts back what the flash holds of the history: the activity log,
/// then, for the minutes since, the newest record: the newest checkpoint,
/// after a crash (or the watchdog), or the planned record, after System
/// OFF or an update (see [`snapshot::newer_checkpoint`]). Call once at
/// boot, from the activity task, before it records anything. Returns the
/// log's end, for [`serve`].
pub async fn restore() -> Log {
    let mut log = Log::UNREAD;
    // The SPI flash is set up in `main`, after the display.
    while FLASH.lock().await.is_none() {
        Timer::after_millis(20).await;
    }
    let mut flash = FLASH.lock().await;
    let flash = flash.as_mut().expect("set at boot");
    if flash.wake().await.is_err() {
        return log;
    }

    // The log first, oldest first, so the history ends up with its last
    // 14½ hours, and with the sums of the day before those (each minute
    // filed pushes an older one out, into `aged`): the minutes the phone
    // hasn't fetched, and those it has, which the Today screen and last
    // night's sleep are drawn from all the same. A full log is 128 KB,
    // read in under a second.
    let mut minutes = 0u32;
    let noted;
    (log, noted) = spill::scan(flash, LOG, |minute, sample| {
        activity::restore_samples(i64::from(minute), sample);
        minutes += 1;
    })
    .await;
    if let Some(through) = log.through() {
        info!(
            "spill: the log holds {} minutes up to minute {}; phone has up to {}",
            minutes,
            through,
            noted.unwrap_or(0)
        );
    }

    // One buffer for the headers and the samples, as in `save`.
    let mut buffer = [0u8; CHUNK];
    let header = &mut buffer[..HEADER_BYTES];
    flash.read(RECORD.start, header).await;
    let header: &[u8; HEADER_BYTES] = (&*header).try_into().expect("HEADER_BYTES long");
    // When the last planned reset was, whether or not its record has done
    // its job: the checkpoints from before it are of the run before.
    let floor = snapshot::as_of_any(header);
    let planned = Header::from_bytes(header);

    // The newest checkpoint that checks out, if it's newer.
    let mut best: Option<(u32, u32)> = None;
    for slot in 0..snapshot::CHECKPOINT_SLOTS {
        let start = CHECKPOINTS.start + slot * snapshot::SLOT_BYTES;
        let now = clock::now_utc_seconds().map(|seconds| seconds.div_euclid(60));
        if let Some(header) = read_header(flash, start, &mut buffer).await
            && snapshot::newer_checkpoint(header.as_of(), best.map(|b| b.1), floor, now)
            && checks_out(flash, start, &header, &mut buffer, false).await
        {
            best = Some((start, header.as_of()));
        }
    }

    let chosen = match (best, planned) {
        (Some((start, _)), planned) => {
            // Whatever the planned record held that the phone hasn't
            // fetched, the checkpoint holds too, being newer: so it's
            // retired as soon as `serve` runs, rather than restored again
            // at a later boot.
            if planned.is_some() {
                RECORD_STATE.store(RETIRING, Ordering::Relaxed);
            }
            Some(start)
        }
        (None, Some(header)) => {
            if checks_out(flash, RECORD.start, &header, &mut buffer, false).await {
                RECORD_STATE.store(RESTORED, Ordering::Relaxed);
                Some(RECORD.start)
            } else {
                warn!("snapshot: the saved activity is corrupt; dropped");
                consume(flash, RECORD.start).await;
                None
            }
        }
        (None, None) => None,
    };
    if let Some(start) = chosen
        && let Some(header) = read_header(flash, start, &mut buffer).await
    {
        // A record may hold more than the history does: one saved by a
        // build up to 0.11.3-rc.4 holds all the phone hadn't fetched, up
        // to a day, where the history is now 14½ hours (and the log may
        // be empty: those builds had none). Put back, its oldest minutes
        // would be let go of as its newest went in, with no copy but the
        // record, which is retired at the next fetch. So they go from
        // the record straight into the log, where a fetch reads them:
        // the minutes before the first bucket the history will hold
        // whole, which is where `keep` then carries on from. They're
        // older than any hold lasts (the history is longer than that),
        // so they are what a fetch could send.
        //
        // TODO: this is for the one update from a build without the log
        // (it costs ~0.6 KB of flash); once no watch runs one, a record
        // never holds more than the history, and it can go.
        //
        // (In 32 bits, and few of them: what's live across the awaits
        // here is static RAM, in the activity task's future.)
        let (from, until) = {
            let last = i64::from(header.as_of()) - 1;
            let newest = activity::newest_minute().map_or(last, |newest| newest.max(last));
            let kept_from = gb_buckets::start_of(
                newest - activity::HISTORY_MINUTES as i64 + gb_buckets::MINUTES,
            );
            let from = log.through().map_or(header.first_minute, |logged| {
                header.first_minute.max(logged + 1)
            });
            (from, minute32(kept_from).min(header.as_of()))
        };
        if from < until {
            info!(
                "spill: minutes {}..{} of the saved activity are older than the history holds; into the log",
                from, until
            );
            let mut minute = from;
            while minute < until {
                let sample: &mut [u8; SAMPLE_BYTES] = (&mut buffer[..SAMPLE_BYTES])
                    .try_into()
                    .expect("SAMPLE_BYTES long");
                let at = HEADER_BYTES as u32 + (minute - header.first_minute) * SAMPLE_BYTES as u32;
                flash.read(start + at, sample).await;
                if spill::recorded(sample)
                    && let Err(e) = spill::append(flash, LOG, &mut log, minute, sample).await
                {
                    warn!("spill: appending failed: {}", e);
                    break;
                }
                minute += 1;
            }
            // (Not past a minute that didn't go in: there's no other copy,
            // but a later boot can't put it in either, once newer ones
            // are.)
            if minute == until {
                log.passed(until - 1);
            }
        }
        checks_out(flash, start, &header, &mut buffer, true).await;
        info!(
            "snapshot: restored {} minutes of activity from minute {}, and {} steps ({=str})",
            header.minutes,
            header.first_minute,
            header.steps,
            if start == RECORD.start {
                "saved before the reset"
            } else {
                "a checkpoint"
            }
        );
    }

    // Where the phone's copy ends: the log's last note of a fetch, or
    // what the record knew (`put_back`), whichever is later: an older
    // build's record knows of fetches the log never heard of. `serve`
    // notes it if the log is the one behind.
    let noted = noted.unwrap_or(0);
    if SYNCED.fetch_max(noted, Ordering::Relaxed) > noted {
        NOTE.store(true, Ordering::Relaxed);
    }
    // What the record put back that a fetch could send goes in the log
    // at `serve`'s first run, with a checkpoint, rather than at the next
    // minute that's due: a reset in the middle of a bucket's minutes
    // going in leaves the rest of them for this. (Not here, in this
    // function's future, which is the activity task's largest as it is.)
    checkpoint();
    flash.sleep().await;
    log
}

// ==============================================================================
// The activity log
// ==============================================================================

/// What [`keep`] is to do to the activity log at this run of [`serve`]:
/// worked out once, before anything is written, as a record may have to
/// be saved first (see [`serve`]).
#[derive(Clone, Copy)]
struct Due {
    /// The first minute the log doesn't take in yet: what a fetch would
    /// send now ends here.
    end: i64,
    /// Where the log has to go back to, if it reaches past it: `end`, or
    /// before it, where minutes have changed since they went in.
    back: i64,
    /// What [`RELOG`] asked for (taken from it), to put back should the
    /// log not be taken back after all.
    relog: u32,
}

/// What the log is due (see [`Due`]), with the history's newest minute
/// at `newest`; as of the clock's minute `now`, when it has jumped ahead
/// of that (see `activity::jumps`), else as of `newest`.
fn due(newest: i64, now: Option<i64>) -> Due {
    // What a fetch would send now: the buckets that are over, before any
    // minute held back (see `gb_buckets::Fetch`). The hold is worked out
    // over the history with interrupts off, a millisecond or so; like the
    // minute's sleep review, which is why this is every couple of
    // minutes, not every poll.
    let end = gb_buckets::loggable_end(
        newest,
        activity::gadgetbridge_hold().map(|hold| hold.from),
        now.unwrap_or(newest),
        crate::sleep_hold::CAP,
    );
    // Where the log has to go back to, which is `end` unless minutes
    // before it have changed since they went in (see `relog_from`): those
    // go straight back in, as they are now.
    let relog = RELOG.swap(0, Ordering::Relaxed);
    let back = match relog {
        0 => end,
        from => end.min(gb_buckets::start_of(i64::from(from))),
    };
    Due { end, back, relog }
}

/// Brings the activity log up to date (see [`crate::spill`]), so that it
/// holds what a fetch could send now and no more, as `due` has it: takes
/// back what no longer is that, notes the last fetch, and appends up to
/// `at_once` of the minutes that newly are. With the history's newest
/// minute at `newest`.
///
/// Whatever it takes back must be in a record already: see [`serve`].
async fn keep(
    flash: &mut spi_flash::SpiFlash,
    writer: &mut Log,
    at_once: i64,
    newest: i64,
    due: Due,
) {
    let Due { end, back, relog } = due;

    // The log may reach past that, in three ways, and then lets go of every
    // bucket from there on, to take them in again when they can go (until
    // that's done nothing is appended: samples are in order):
    //
    // - A clock set back: the history has dropped the minutes after the
    //   new time, and files them again from there (see `minute_ring`).
    //   It also asks (`relog_from`, so `back`), for the set-back of
    //   seconds that the clock has made up again by now.
    // - A hold from before the log's end: a wearer gone still, whose last
    //   minutes awake may yet turn out the first of a sleep. Their kinds
    //   are being revised, so the copies in the log would go stale, and a
    //   fetch (or a boot) would get the sleep's start as awake. Or a
    //   whole night: one settled and logged before a reset, which forgot
    //   that it was settled (see `sleep_hold`), is held again at the
    //   boot's first run, until the next minute's review settles it
    //   again or its time is up.
    // - A workout recognised: its first minutes, marked as exercise after
    //   the fact (`back`, above).
    if snapshot::taken_back(writer.through().map(i64::from), back).is_some() {
        let from = minute32(back);
        debug!("spill: taking the log back to minute {}", from);
        if let Err(e) = spill::truncate(flash, LOG, writer, from).await {
            warn!("spill: taking back failed: {}", e);
            // To try again at the next run.
            if relog != 0 {
                RELOG.store(relog, Ordering::Relaxed);
            }
            return;
        }
    }

    // Where the phone's copy ends now, if that's changed: what a boot
    // goes by (see `restore`).
    if NOTE.swap(false, Ordering::Relaxed) {
        let synced = SYNCED.load(Ordering::Relaxed);
        if let Err(e) = spill::synced(flash, LOG, writer, synced).await {
            warn!("spill: noting the fetch failed: {}", e);
            NOTE.store(true, Ordering::Relaxed);
        }
    }

    // And what's new of it goes in, of the buckets the history still has
    // whole.
    let oldest =
        gb_buckets::start_of(newest - activity::HISTORY_MINUTES as i64 + gb_buckets::MINUTES);
    let first = writer
        .through()
        .map_or(oldest, |logged| (i64::from(logged) + 1).max(oldest));
    // Whole buckets only: a fetch reads the log as it is (the flash is
    // locked while this writes, but not between calls).
    let end = end.min(gb_buckets::start_of(first + at_once));
    let mut appended = 0u32;
    for minute in first..end {
        let mut sample = [0; spill::SAMPLE];
        activity::copy_samples(minute, &mut sample);
        // Minutes with nothing recorded take no room (see `spill`).
        if spill::recorded(&sample) {
            if let Err(e) = spill::append(flash, LOG, writer, minute32(minute), &sample).await {
                warn!("spill: appending failed: {}", e);
                return;
            }
            appended += 1;
        }
    }
    if end > first {
        writer.passed(minute32(end - 1));
        debug!(
            "spill: {} minutes into the log, now up to minute {}",
            appended,
            end - 1
        );
    }
}

/// For a fetch of the minutes from `from` on: a reader of the activity
/// log, and a minute that nothing it will read is before (see
/// [`spill::open`]); `None` if the log holds nothing (or can't be read).
/// Called from the Bluetooth task, which then asks for a bucket at a time
/// ([`log_bucket`]), with the flash free in between for the activity
/// task's writes.
///
/// Minutes in 32 bits here and in [`log_bucket`], as the log has them
/// ([`minute32`]): an `async fn` keeps its arguments twice over.
pub async fn log_open(from: u32) -> Option<(spill::Reader, u32)> {
    let mut flash = FLASH.lock().await;
    let flash = flash.as_mut()?;
    let opened = spill::open(flash, LOG, from).await;
    flash.sleep().await;
    opened
}

/// The log's next bucket for a fetch (see [`spill::bucket`]), its
/// minutes' samples in `want.out`.
///
/// After a bucket, the flash is left awake for the next (a wake and a
/// sleep per bucket were a third of each read's time): the fetch reads on
/// until one of these says there's no more, which puts it to sleep, or
/// calls [`log_close`] if it stops before.
pub async fn log_bucket(
    want: &mut spill::Want<'_, { gb_buckets::MINUTES as usize }>,
) -> spill::Read {
    let mut flash = FLASH.lock().await;
    let Some(flash) = flash.as_mut() else {
        return spill::Read::End;
    };
    let read = spill::bucket(flash, LOG, want).await;
    if !matches!(read, spill::Read::Bucket(_)) {
        flash.sleep().await;
    }
    read
}

/// Puts the flash back to sleep after a fetch that stopped reading the
/// log part-way (see [`log_bucket`]).
pub async fn log_close() {
    if let Some(flash) = FLASH.lock().await.as_mut() {
        flash.sleep().await;
    }
}

/// The header of a fresh record at `start` (see [`Header::from_bytes`]),
/// read through `buffer`; its samples aren't checked yet.
async fn read_header(
    flash: &mut spi_flash::SpiFlash,
    start: u32,
    buffer: &mut [u8; CHUNK],
) -> Option<Header> {
    let header = &mut buffer[..HEADER_BYTES];
    flash.read(start, header).await;
    Header::from_bytes((&*header).try_into().expect("HEADER_BYTES long"))
}

/// Whether the samples of the record at `start` match its header's
/// checksum: the whole record, read through `buffer`. And if they do, and
/// `restore`, puts the record back (see [`put_back`]). So a record that
/// checks out is read twice over, once to check and once to restore, as
/// none of it may be used before all of it is checked; but a record is
/// 5.2 KB at the most (8.6 KB, an older build's): a few tens of ms. (One
/// function for both passes, as two cost flash.)
async fn checks_out(
    flash: &mut spi_flash::SpiFlash,
    start: u32,
    header: &Header,
    buffer: &mut [u8; CHUNK],
    restore: bool,
) -> bool {
    let data = start + HEADER_BYTES as u32;
    let length = usize::from(header.minutes) * SAMPLE_BYTES;
    let first = i64::from(header.first_minute);
    let mut hash = snapshot::CHECKSUM_SEED;
    for offset in (0..length).step_by(CHUNK) {
        let chunk = &mut buffer[..CHUNK.min(length - offset)];
        flash.read(data + offset as u32, chunk).await;
        hash = snapshot::checksum(hash, chunk);
        if restore {
            activity::restore_samples(first + (offset / SAMPLE_BYTES) as i64, chunk);
        }
    }
    if restore {
        put_back(header);
    }
    header.expected_checksum(hash) == header.checksum
}

/// Puts back the rest of a record whose samples [`checks_out`] restored:
/// today's steps (unless the reset kept them), and, if it knew, where the
/// phone's copy of the history ends.
fn put_back(header: &Header) {
    let first = i64::from(header.first_minute);
    let day = (header.day != snapshot::UNKNOWN_DAY).then_some(i64::from(header.day));
    activity::restore_steps(day, header.steps);
    // The phone may lack all of it: the record starts where the phone's
    // copy ended, and a fetch held back since then (see `sleep_hold`) may
    // have left the phone asking from much later than that. So, to the
    // watch, the phone has what's before it, and nothing since: if the
    // watch knew that much when it saved it. Otherwise (a record saved
    // before any fetch, or by an older build) the phone may hold some of
    // it already, and the next fetch starts after what the phone asks for
    // (see `gb_buckets::start`).
    if header.synced {
        SYNCED.store(
            (first - 1).clamp(0, i64::from(u32::MAX)) as u32,
            Ordering::Relaxed,
        );
    }
}

/// Called after each activity fetch the phone completes, with the newest
/// minute it got: from then on, the phone has everything a restored record
/// held, so the record can be retired (by [`retire`]).
pub fn on_synced(through_minute: i64) {
    let synced = minute32(through_minute);
    if SYNCED.swap(synced, Ordering::Relaxed) != synced {
        NOTE.store(true, Ordering::Relaxed);
    }
    let _ = RECORD_STATE.compare_exchange(RESTORED, RETIRING, Ordering::Relaxed, Ordering::Relaxed);
}

/// Called when the phone sets the clock, with the UTC minute it set it to:
/// what the phone has fetched from that minute on was filed by a clock
/// that was ahead, and the history files those minutes again (see
/// `gb_buckets`' "A clock put right"). After a restore, too: the record
/// may have been saved by a clock that was ahead.
pub fn on_clock_set(minute: i64) {
    let before = SYNCED.load(Ordering::Relaxed);
    let _ = SYNCED.try_update(Ordering::Relaxed, Ordering::Relaxed, |synced| {
        let synced = (synced != 0).then_some(i64::from(synced));
        let synced = gb_buckets::synced_after_set(synced, minute)?;
        Some(synced.clamp(0, i64::from(u32::MAX)) as u32)
    });
    if SYNCED.load(Ordering::Relaxed) != before {
        NOTE.store(true, Ordering::Relaxed);
    }
}

/// Where the phone's copy of the history ends, as far as the watch knows:
/// the newest minute of the last fetch it completed since boot, or the
/// minute before a restored record that knew; `None` if neither. A fetch
/// sends from there (see `gb_buckets::start`), so that what an earlier one
/// held back (see `sleep_hold`) goes out even though the phone asks from
/// when it last fetched.
pub fn synced_through() -> Option<i64> {
    match SYNCED.load(Ordering::Relaxed) {
        0 => None,
        minute => Some(i64::from(minute)),
    }
}

/// The flash work asked of the activity task: retiring a restored record
/// once the phone has fetched it (see [`on_synced`]), so no later boot
/// restores it again; keeping the activity log (see [`keep`]); and saving,
/// for [`shut_down`] or an update, or a checkpoint (see [`checkpoint`]).
/// Call every few seconds, with what [`restore`] returned.
///
/// Why there: the futures of SPI flash operations are a hundred-odd bytes,
/// and every task's future is static RAM, which is scarce. The activity
/// task's has room to spare between its polls (its own peak is reading the
/// accelerometer); the UI task's and the Bluetooth task's don't.
pub async fn serve(log: &mut Log) {
    if RECORD_STATE.load(Ordering::Relaxed) == RETIRING {
        retire().await;
    }
    let asked = SAVE.load(Ordering::Relaxed);
    let saving = matches!(asked, SAVE_ASKED | CHECKPOINT_ASKED);
    // The clock has jumped far ahead of the history (the phone set it:
    // after the watch was off, say), and the next minute filed would push
    // out of the history minutes that aren't in the log yet (see
    // `activity::jumps`). So the log takes those first, and the minute is
    // filed from here, before anything else files one.
    let jump = clock::now_utc_seconds()
        .map(|seconds| seconds.div_euclid(60))
        .filter(|&minute| activity::jumps(minute));
    // Between saves, the log wants writing only for a fetch to note.
    if !(saving || jump.is_some() || NOTE.load(Ordering::Relaxed)) {
        return;
    }
    {
        let mut flash = FLASH.lock().await;
        let flash = flash.as_mut().expect("set at boot");
        let awake = match flash.wake().await {
            Ok(()) => true,
            Err(e) => {
                warn!("snapshot: the flash didn't wake: {}", e);
                false
            }
        };
        let newest = activity::newest_minute();
        // Where the record goes, if one is asked for (a checkpoint only
        // once there's a history to save).
        let mut record = match asked {
            SAVE_ASKED => Some(RECORD.start),
            CHECKPOINT_ASKED => newest.map(checkpoint_at),
            _ => None,
        };
        if awake {
            if asked == SAVE_ASKED {
                // The new record replaces any restored one, and with
                // the log holds all that's unsynced: nothing to
                // retire after a fetch that ends meanwhile.
                RECORD_STATE.store(NONE, Ordering::Relaxed);
            }
            // Boot couldn't read the log (the flash didn't answer): find
            // its end now, before anything is appended over it.
            if log.is_unread() {
                (*log, _) = spill::scan(flash, LOG, |_, _| ()).await;
            }
        }
        // The log, which is of the history: none without the time.
        if awake && let Some(newest) = newest {
            let due = due(newest, jump);

            // The log first, so that the record starts where it ends.
            // Unless the log is about to let go of minutes (see `keep`):
            // then the record first, from where the log will end. Those
            // minutes are in no record yet, as each starts where the log
            // ended when it was saved, so a reset between the log letting
            // go of them and the record's save would leave them in
            // neither. That's a wearer's last few minutes before going
            // still, mostly, but a whole night at the first run after a
            // reset in the morning (the night was settled and logged, and
            // the reset forgot the settling), and that record takes half
            // a second to write: just when a watch that crashes at every
            // boot would crash again.
            //
            // So whether or not a record was asked for (a fetch to note,
            // or a jump, also bring the log up to date): a checkpoint
            // then. Even after the planned save, when no checkpoint is
            // otherwise started (see `SAVE`): the flash is awake for the
            // log as it is, and is put back to sleep below.
            //
            // (Such a record holds minutes that the log has again by the
            // time `keep` is done, those it appends. A boot reads the log
            // first and keeps its copy of a minute, which is the same.)
            if let Some(until) = snapshot::taken_back(log.through().map(i64::from), due.back) {
                let start = record.take().unwrap_or(checkpoint_at(newest));
                save(flash, start, Some(minute32(until))).await;
            }

            // Before a planned reset, all there is: the record is what
            // an older build would restore, and has only a day. Before a
            // jump too: all that it pushes out.
            let at_once = match (asked, jump) {
                (SAVE_ASKED, _) | (_, Some(_)) => snapshot::MAX_MINUTES as i64,
                (CHECKPOINT_ASKED, None) => SPILL_AT_ONCE,
                _ => 0,
            };
            keep(flash, log, at_once, newest, due).await;
        }
        // With no `await` since the log took them in. And whether or not
        // it could: a flash that doesn't answer mustn't stop the history's
        // clock, and a heart rate waits on this (see
        // `activity::record_heart_rate`).
        if let Some(minute) = jump {
            info!(
                "spill: the clock jumped ahead; the history moves on to minute {}",
                minute
            );
            activity::skip_to(minute);
            // A checkpoint is of the history as it is now, so in this
            // minute's slot: the one worked out above may be the last
            // checkpoint's, which a save cut short would then take with
            // it.
            if asked == CHECKPOINT_ASKED && record.is_some() {
                record = Some(checkpoint_at(minute));
            }
        }
        // (A record saved above, before the log let go, is of just before
        // the jump, as the checkpoint before it was.)
        if awake && let Some(start) = record {
            save(flash, start, log.through()).await;
        }
        flash.sleep().await;
    }
    match asked {
        SAVE_ASKED => SAVE.store(SAVE_DONE, Ordering::Relaxed),
        CHECKPOINT_ASKED => {
            // Unless the planned save was asked for meanwhile, which then
            // goes next.
            let _ =
                SAVE.compare_exchange(CHECKPOINT_ASKED, 0, Ordering::Relaxed, Ordering::Relaxed);
        }
        _ => {}
    }
}

/// Where the checkpoint goes for a history whose newest minute is
/// `newest`: its slot (see [`snapshot::checkpoint_slot`]).
fn checkpoint_at(newest: i64) -> u32 {
    CHECKPOINTS.start + snapshot::checkpoint_slot(newest) * snapshot::SLOT_BYTES
}

/// Has [`serve`] save a checkpoint (see [`crate::snapshot`]) at its next
/// run: call as the clock reaches a minute that's due
/// ([`snapshot::checkpoint_due`]), with the minute before it recorded. Not
/// while a planned save is asked for or done, as the watch is about to
/// reset or switch off.
pub fn checkpoint() {
    let _ = SAVE.compare_exchange(0, CHECKPOINT_ASKED, Ordering::Relaxed, Ordering::Relaxed);
}

async fn retire() {
    RECORD_STATE.store(NONE, Ordering::Relaxed);
    let mut flash = FLASH.lock().await;
    let flash = flash.as_mut().expect("set at boot");
    if flash.wake().await.is_ok() {
        consume(flash, RECORD.start).await;
        flash.sleep().await;
        info!("snapshot: the phone has the saved activity; record retired");
    }
}

/// Marks the record at `start` consumed: one word cleared in place, no
/// erase.
async fn consume(flash: &mut spi_flash::SpiFlash, start: u32) {
    let consumed = snapshot::CONSUMED.to_le_bytes();
    let address = start + snapshot::CONSUMED_OFFSET;
    if let Err(e) = flash.program(address, &consumed).await {
        warn!("snapshot: retiring the record failed: {}", e);
    }
}

/// Has the activity task save the record (see [`serve`]), and waits until
/// it has, or for 5 s; `pet` runs every 100 ms meanwhile.
async fn save_and_wait(mut pet: impl FnMut()) {
    SAVE.store(SAVE_ASKED, Ordering::Relaxed);
    for _ in 0..50 {
        Timer::after_millis(100).await;
        pet();
        if SAVE.load(Ordering::Relaxed) == SAVE_DONE {
            return;
        }
    }
    warn!("snapshot: saving timed out");
}

/// Saves the history the phone hasn't fetched, and today's steps, for the
/// next boot to [`restore`]: call right before a deliberate reset. Takes
/// about a second (the activity task saves at its next poll), up to 5 s.
pub async fn save_before_reset() {
    save_and_wait(|| ()).await;
}

// ==============================================================================
// Switching off
// ==============================================================================

/// Saves what would be lost, shows why the watch is switching off, puts
/// the board to sleep, and enters System OFF. Never returns: the next thing
/// that happens is a boot. Call from the UI loop, which owns the display.
pub async fn shut_down(
    reason: Reason,
    display: &mut Display,
    canvas: &mut Canvas,
    backlight: &mut Backlight,
    panel_awake: bool,
    watchdog: &mut Option<WatchdogHandle>,
) -> ! {
    warn!("system off: {}; saving, then switching off", reason);
    // Why, for a few seconds; the rest happens in the dark. (In this order,
    // the display and canvas aren't held across the save, whose future,
    // like everything here, is part of the UI task's: static RAM.)
    if !panel_awake {
        display.wake().await;
    }
    // The canvas holds why (see `ui::App::render_off`).
    canvas.flush(display).await;
    if !panel_awake {
        display.show().await;
    }
    // Mid, as the screen was before brightness levels: 3 s of it is nothing.
    backlight.set(crate::brightness::Level::MID);
    pet(watchdog);
    Timer::after(SHOW_FOR).await;
    backlight.off();
    display.sleep().await;
    pet(watchdog);

    // The activity task saves (see `serve`), within a poll or two.
    save_and_wait(|| pet(watchdog)).await;
    let button = reason == Reason::Storage;
    info!(
        "system off: switching off; waking on {}",
        if button {
            "the button or charger"
        } else {
            "the charger"
        }
    );
    // The last log line that makes it to a field log.
    field_log::flush().await;
    pet(watchdog);

    // The sensors keep their state while the nRF52 is off, so each is put
    // in its lowest-power state. The bus stays locked from here on, so no
    // other task can start them again in the moments left; so each write
    // recovers the bus itself if it fails, and tries again (see
    // `i2c::write_held`), as `i2c::reset_controller` would wait for this
    // lock.
    {
        let mut twim = i2c::BUS.get().await.lock().await;
        heart::switch_off(&mut twim).await;
        activity::suspend(&mut twim).await;
        touch::deep_sleep(&mut twim).await;
        core::mem::forget(twim);
    }
    // Waits for anything still using the SPI flash (whose erase would carry
    // on, and leave it in standby, ~14 µA more), which then puts it in deep
    // power-down, as every user does when done; then keeps it there by
    // holding it, and the SPI bus too, for good.
    core::mem::forget(FLASH.lock().await);
    core::mem::forget(spi::BUS.get().await.lock().await);
    clock::forget();
    enter(button)
}

/// The UI loop isn't there to feed the watchdog while switching off, which
/// can take most of its 7 s.
fn pet(watchdog: &mut Option<WatchdogHandle>) {
    if let Some(watchdog) = watchdog {
        watchdog.pet();
    }
}

/// Arms the wake-up pins and enters System OFF.
///
/// A pin wakes the chip through its SENSE setting: once any armed pin is
/// at its sensed level, the chip resets. (If one already is, it resets at
/// once, so the charger is only armed when it's unplugged; switching off
/// for an empty battery on the charger resets instead.) GPIO settings
/// and output levels are kept while off, which is what keeps the button
/// powered, and is also why every output must be left in its idle state.
fn enter(button: bool) -> ! {
    use pac::gpio::vals::{Dir, Input, Pull, Sense};

    cortex_m::interrupt::disable();
    let p0 = pac::P0;
    // Backlight (P0.14, P0.22, P0.23) and motor (P0.16) are active low.
    p0.outset().write(|w| {
        for pin in [14, 16, 22, 23] {
            w.set_pin(pin, true);
        }
    });
    // The button only conducts while P0.15 powers it (see `button_task`).
    if button {
        p0.outset().write(|w| w.set_pin(15, true));
    } else {
        p0.outclr().write(|w| w.set_pin(15, true));
    }
    // The charger's power-present line (P0.19) is pulled up externally and
    // goes low with power.
    let unplugged = p0.in_().read().pin(19);
    // Switching off for an empty battery while power has come back (the
    // "battery empty" screen invites plugging in, and switching off takes
    // several seconds after it): the charger can't be armed, as it's
    // already at its level, and nothing else would be, so System OFF would
    // have no way out short of the battery running flat again, months
    // later on a full one. Start afresh on the charger instead: the record
    // is saved, and `Battery::critical` doesn't fire on the charger.
    if !button && !unplugged {
        cortex_m::peripheral::SCB::sys_reset();
    }
    // Nothing else may wake us: embassy's GPIO driver arms SENSE on pins it
    // waits on (the touch interrupt, the button's release). And inputs
    // left connected to floating lines (a sleeping chip's output) can draw
    // current in their input buffers, so they're disconnected, pulls and
    // all.
    for pin in 0..32 {
        p0.pin_cnf(pin).modify(|w| {
            w.set_sense(Sense::Disabled);
            if w.dir() == Dir::Input {
                w.set_input(Input::Disconnect);
                w.set_pull(Pull::Disabled);
            }
        });
    }
    let sense = |pin: usize, level: Sense| {
        p0.pin_cnf(pin).write(|w| {
            w.set_dir(Dir::Input);
            w.set_input(Input::Connect);
            w.set_pull(Pull::Disabled);
            w.set_sense(level);
        })
    };
    if unplugged {
        sense(19, Sense::Low);
    }
    // The button (P0.13) reads high while pressed.
    if button {
        sense(13, Sense::High);
    }
    p0.latch().write_value(pac::gpio::regs::Latch(u32::MAX));

    // No RAM retention while off (the reset default, made explicit): see
    // the module's documentation.
    for block in 0..8 {
        pac::POWER.ram(block).power().modify(|w| {
            w.set_s_retention(0, false);
            w.set_s_retention(1, false);
        });
    }
    pac::POWER.systemoff().write(|w| w.set_systemoff(true));
    // With a debug probe attached since power-up, System OFF is only
    // emulated, and the CPU carries on here. A wake-up pin still resets it;
    // otherwise the watchdog, or the probe, does.
    loop {
        cortex_m::asm::wfe();
    }
}
