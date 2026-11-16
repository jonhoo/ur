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
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use defmt::{debug, info, warn};
use embassy_nrf::pac;
use embassy_nrf::wdt::WatchdogHandle;
use embassy_time::{Duration, Timer};

use crate::canvas::Canvas;
use crate::display::Display;
use crate::snapshot::{self, HEADER_BYTES, Header, SAMPLE_BYTES};
use crate::spi_flash::{self, FLASH};
use crate::{Backlight, activity, clock, field_log, gb_buckets, heart, i2c, spi, touch};

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
/// the bootloader doesn't use (see [`crate::spi_flash`]), and just above the
/// field log's second stretch.
pub(crate) const CHECKPOINTS: Range<u32> = 0x19_D000..0x1F_D000;
const _: () = assert!(
    CHECKPOINTS.end - CHECKPOINTS.start == snapshot::CHECKPOINT_SLOTS * snapshot::SLOT_BYTES
);
const _: () = assert!(CHECKPOINTS.end <= RECORD.start);
const _: () = assert!(CHECKPOINTS.start.is_multiple_of(spi_flash::SECTOR));
const _: () =
    assert!(crate::dfu_slot::SLOT_START + crate::dfu_slot::SLOT_SIZE <= CHECKPOINTS.start);
const _: () = assert!(snapshot::SECTOR_BYTES == spi_flash::SECTOR);
const _: () = assert!(
    CHUNK >= HEADER_BYTES,
    "headers are staged in a chunk's buffer"
);
// The field log is on both sides of the two (see [`crate::log_ring`]): its
// second stretch ends where the checkpoints start, and its first starts
// where the record ends.
const _: () = assert!(crate::log_ring::SECOND.end <= CHECKPOINTS.start);
const _: () = assert!(RECORD.end <= crate::log_ring::FIRST.start);
const _: () = assert!(RECORD.start.is_multiple_of(spi_flash::SECTOR));

/// Samples go between the history and the flash this many bytes at a time
/// (six minutes, the least that also holds a header): a small buffer, since
/// it sits in a task's future for good, in a build with little RAM to
/// spare. A day's record then takes 240 small writes, about a second.
const CHUNK: usize = 6 * SAMPLE_BYTES;

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
/// boot (or, until it has, the minute before a restored record that knew
/// where the phone's copy ended), or 0 if the watch doesn't know: where
/// the next save can start (see [`snapshot::unsynced_from`]), and the next
/// fetch at the latest (see [`synced_through`]). Put back when the phone
/// sets the clock to before it (see [`on_clock_set`]).
static SYNCED: AtomicU32 = AtomicU32::new(0);

// ==============================================================================
// Saving and restoring
// ==============================================================================

/// Writes a record at `start` (the planned record's, or a checkpoint's
/// slot): today's steps, and the history from where the phone's next
/// fetch would start. Failures are logged; there's nothing else to do
/// about them with the battery running out, and a checkpoint's next try is
/// minutes away.
async fn save(start: u32) {
    // From a bucket's start, so that the history restored from it holds
    // whole buckets: one cut short would go to the phone as less than it
    // has (see `gb_buckets`). (Where the phone's copy ends is one already.)
    let newest = activity::newest_minute();
    let (from, synced) = newest
        .map(|newest| snapshot::unsynced_from(newest, synced_through()))
        .unwrap_or((0, false));
    // With nothing to save, the record starts after the newest minute, so
    // that it still says when it was saved (see `snapshot::empty_from`).
    let (first, minutes) = activity::span_from(gb_buckets::start_of(from))
        .unwrap_or((i64::from(snapshot::empty_from(newest)), 0));
    // Every local that lives across an await is static RAM, in the
    // activity task's future, so they're kept few and small.
    let (first, minutes) = (first as u32, minutes as u16);

    let mut flash = FLASH.lock().await;
    let flash = flash.as_mut().expect("set at boot");
    let written = async {
        flash.wake().await?;
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
    flash.sleep().await;
    if let Err(e) = written {
        warn!("snapshot: saving at {=u32:#x} failed: {}", start, e);
    }
}

/// Puts back the newest record the phone hasn't fetched: the newest
/// checkpoint, after a crash (or the watchdog), or the planned record,
/// after System OFF or an update (see [`snapshot::newer_checkpoint`]).
/// Call once at boot, from the activity task, before it records anything.
pub async fn restore() {
    // The SPI flash is set up in `main`, after the display.
    while FLASH.lock().await.is_none() {
        Timer::after_millis(20).await;
    }
    let mut flash = FLASH.lock().await;
    let flash = flash.as_mut().expect("set at boot");
    if flash.wake().await.is_err() {
        return;
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
                consume(flash).await;
                None
            }
        }
        (None, None) => None,
    };
    if let Some(start) = chosen
        && let Some(header) = read_header(flash, start, &mut buffer).await
    {
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
    flash.sleep().await;
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
/// none of it may be used before all of it is checked; but a day's record
/// is 8.6 KB: a few tens of ms. (One function for both passes, as two
/// cost flash.)
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
    SYNCED.store(
        through_minute.clamp(0, i64::from(u32::MAX)) as u32,
        Ordering::Relaxed,
    );
    let _ = RECORD_STATE.compare_exchange(RESTORED, RETIRING, Ordering::Relaxed, Ordering::Relaxed);
}

/// Called when the phone sets the clock, with the UTC minute it set it to:
/// what the phone has fetched from that minute on was filed by a clock
/// that was ahead, and the history files those minutes again (see
/// `gb_buckets`' "A clock put right"). After a restore, too: the record
/// may have been saved by a clock that was ahead.
pub fn on_clock_set(minute: i64) {
    let _ = SYNCED.try_update(Ordering::Relaxed, Ordering::Relaxed, |synced| {
        let synced = (synced != 0).then_some(i64::from(synced));
        let synced = gb_buckets::synced_after_set(synced, minute)?;
        Some(synced.clamp(0, i64::from(u32::MAX)) as u32)
    });
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
/// restores it again, and saving, for [`shut_down`] or an update, or a
/// checkpoint (see [`checkpoint`]). Call every few seconds.
///
/// Why there: the futures of SPI flash operations are a hundred-odd bytes,
/// and every task's future is static RAM, which is scarce. The activity
/// task's has room to spare between its polls (its own peak is reading the
/// accelerometer); the UI task's and the Bluetooth task's don't.
pub async fn serve() {
    if RECORD_STATE.load(Ordering::Relaxed) == RETIRING {
        retire().await;
    }
    match SAVE.load(Ordering::Relaxed) {
        SAVE_ASKED => {
            // The new record replaces any restored one, and holds all
            // that's unsynced: nothing to retire after a fetch that ends
            // meanwhile.
            RECORD_STATE.store(NONE, Ordering::Relaxed);
            save(RECORD.start).await;
            SAVE.store(SAVE_DONE, Ordering::Relaxed);
        }
        CHECKPOINT_ASKED => {
            if let Some(newest) = activity::newest_minute() {
                let slot = snapshot::checkpoint_slot(newest);
                save(CHECKPOINTS.start + slot * snapshot::SLOT_BYTES).await;
            }
            // Unless the planned save was asked for meanwhile, which then
            // goes next.
            let _ =
                SAVE.compare_exchange(CHECKPOINT_ASKED, 0, Ordering::Relaxed, Ordering::Relaxed);
        }
        _ => {}
    }
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
        consume(flash).await;
        flash.sleep().await;
        info!("snapshot: the phone has the saved activity; record retired");
    }
}

/// Marks the record consumed: one word cleared in place, no erase.
async fn consume(flash: &mut spi_flash::SpiFlash) {
    let consumed = snapshot::CONSUMED.to_le_bytes();
    let address = RECORD.start + snapshot::CONSUMED_OFFSET;
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
