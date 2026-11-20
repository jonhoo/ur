//! Heart rate, from the HRS3300 optical (PPG) sensor.
//!
//! The sensor's green LED is the single most power-hungry thing on the
//! watch short of the backlight and radio (12.5–40 mA while lit), so it
//! isn't run continuously. Instead, a measurement *session* runs:
//!
//! - in the background, when [`crate::heart_schedule`] says one is due
//!   (every 2–20 minutes, depending on movement and recent readings), once
//!   the wrist has been still for a few seconds after that (see [`CALM`]),
//!   or two minutes on if it isn't
//!   ([`heart_schedule::Schedule::start`]), or, when no reading has come
//!   for 20 minutes, as soon as it's still, due or not
//!   ([`heart_schedule::Schedule::seek`]), for up to [`SESSION_LIMIT`] or
//!   until it has a reading (typically ~11 s), carrying on through
//!   movement (see `heart_schedule`'s "Trying through movement"), unless
//!   it's plainly getting nowhere ([`heart_schedule::hopeless`]); awake,
//!   every other round of tries (a scheduled session and its retries)
//!   starts with a brighter LED (see [`Arm`]), an experiment (and the
//!   logging build logs the accelerometer's samples through every awake
//!   session, see `trace_accel`), and
//! - while the clock face is showing (see
//!   [`crate::heart_reading::set_watching`]), continuously,
//!   since then someone is looking at the number, and
//! - while Gadgetbridge's live activity asks for it (see [`crate::live`]
//!   and [`crate::heart_reading::set_live`]), continuously too, for as long
//!   as it asks, and
//! - through a workout (see [`crate::workout`], and "Through a workout"
//!   below), continuously as well: a moving wrist reads in its pauses,
//!   and only a sensor that's already on catches those;
//!
//! but never on the charger (see `heart_schedule`'s "The charger"): no
//! session starts while its power is present, and one under way stops
//! within a second of it arriving.
//!
//! Between sessions the sensor is fully off: no LED, no oscillator. (Should
//! switching it off fail, that's tried again every second until it works.)
//!
//! The signal processing lives in [`crate::ppg`], which knows nothing about
//! the hardware; this module drives the sensor, keeps its signal in range
//! ([`LEVELS`]), vetoes measurements while the wrist moves (using
//! [`activity::movement`]), and publishes readings for the clock face
//! and the per-minute history Gadgetbridge fetches, through
//! [`crate::heart_reading`].

#[cfg(feature = "field-log")]
use core::sync::atomic::{AtomicU8, Ordering};

use defmt::{debug, info, trace, warn};
use embassy_futures::select::select;
use embassy_time::{Duration, Instant, Ticker, Timer, with_timeout};
use embedded_hal_async::i2c::I2c;

use crate::activity;
// What the rest of the firmware reads and sets lives there, free of
// hardware, so the simulator runs it too.
use crate::heart_reading::{WAKE, on_charger, publish};
pub use crate::heart_reading::{wake, worn};
use crate::heart_schedule::{self, Arm, Before, Charger, Outcome, Schedule, Start};
use crate::i2c::Device;
use crate::ppg::{Consensus, Conversions, Ppg, Rejection};

// ==============================================================================
// Scheduling
// ==============================================================================

/// Whether to measure continuously, rather than when the schedule says:
/// someone's waiting for a reading (looking at the clock face, or at
/// Gadgetbridge's live chart), or a workout is under way.
fn watching() -> bool {
    crate::heart_reading::watching() || held()
}

/// Whether a workout, and nothing else, keeps the sensor on.
fn held() -> bool {
    !crate::heart_reading::watching() && activity::workout().is_some()
}

// ==============================================================================
// Through a workout
// ==============================================================================
//
// The schedule is built for a wrist at rest: it waits for calm, and backs
// off when a session fails. Exercise is neither calm nor short, so on the
// sealed watch (2026-11-07 to 20) sessions during walks read 4 times in
// 36, and nothing at 110 BPM or more ever reached the phone (see
// `workout`'s docs for the numbers that follow).
//
// A moving wrist can't be read (walking swings the light level by 30-700
// counts against a pulse of ~5, at rhythms the pulse shares), but it
// pauses: at a crossing, between sets, when the walk ends. A reading
// takes ~14 s of calm with the sensor already on (two windows on a calm
// wrist, see `ppg::Consensus`), and ~18 s from a session that must first
// see three calm polls, then warm up. The logs' walks held 18 calm
// stretches of 14 s and their efforts 90; 8 and 57 of those lasted 18 s.
// So through a workout the sensor simply stays on, as it does while
// someone looks at the clock face, and takes every pause there is, the
// first minutes of recovery included (a workout only ends some minutes
// after the wrist lets up).
//
// What it costs is LED time: ~68 minutes a day on those logs (117 on the
// days of a trip, 280 on the busiest), ~3.2 mAh at the ~2.8 mA a session
// draws, some 2% of the battery, which the user would rather spend than
// lose the readings (see docs/power-plan.md).
// It isn't charged to the schedule's budget beyond one session's worth
// (`heart_schedule::SESSION_RESERVE`): the budget is there to bound what
// a day of failing tries can cost, and a workout bounds itself (it needs
// steps or hard movement to go on). A watch that isn't on a wrist (in a
// bag, on a walk) ends its session within seconds for want of skin, and
// looks again every [`WORKOUT_RETRY`].
//
// Two things keep hundreds of windows on a moving wrist from making a
// wrong reading where a 30 s session's eleven didn't: the windows that
// agree must be recent ([`AGREE_WITHIN`]), and one at the arm's swing is
// left out ([`crate::workout::swing`]).
//
// In the logging build, such a session's samples are logged for its first
// [`WORKOUT_TRACE_CHECKS`] seconds only: an hour of them would be ~330 KB
// of a ~3 MB log that holds four or five days. Each window's verdict (at `debug`)
// and every reading are logged throughout.

/// How long after a workout's session found no skin (or lost the sensor)
/// to look again, while the workout lasts. A watch really off the wrist
/// costs a few seconds of LED each time; one that only shifted (the
/// sealed watch said "not on a wrist" twice mid-walk, 2026-11-20) is
/// measured again for the rest of its workout.
const WORKOUT_RETRY: u8 = 120;

/// Level checks (seconds) of a workout's session whose samples the
/// logging build logs (see "Through a workout" above).
///
/// TODO: with a reference worn alongside (a chest strap, or the Garmin
/// again), whole walks of samples would be what a motion-tolerant
/// estimator is developed on; log them all then.
const WORKOUT_TRACE_CHECKS: u8 = 120;

/// Windows (one every ~2 s) without an accepted one after which the
/// accepted ones so far are forgotten, for agreeing with later ones (see
/// `ppg::Consensus::push`, which keeps its three latest however old).
///
/// A session's length of them: the rule that three accepted windows which
/// agree make a reading was tuned on sessions of 30 s, where it reads a
/// moving wrist to within a mean 4 BPM. A session that stays on for a
/// workout, or a long look at the clock face, sees hundreds of windows,
/// and three stray ones minutes apart agreeing by chance (within 6 BPM,
/// of a band 160 wide) is no longer rare.
const AGREE_WITHIN: u8 = (SESSION_LIMIT.as_secs() / 2) as u8;

/// Level checks (seconds) since a session's last reading after which it
/// no longer ends as having read (see [`Ended::Faded`]): a session's
/// length.
const FRESH_CHECKS: u8 = SESSION_LIMIT.as_secs() as u8;

/// How long a background session may try before giving up. A clean
/// signal on a calm wrist gives a reading after ~10.5 s (half a second
/// warming up and settling, an 8 s window, then another 2 s later that
/// agrees with it; see `ppg::Consensus`); this leaves room for a few
/// rejected windows, or the gain adjusting.
///
/// Not longer: in field logs (195 readings), 88% came by 14 s, and of the
/// sessions still going at 20 s, fewer than 2% a second read after that.
/// A still wrist's signal comes and goes over minutes rather than
/// seconds, so the LED does more good in another session soon after (see
/// [`heart_schedule::CALM_RETRY_INTERVAL`]) than in a longer one.
///
/// Nor on a busy wrist, which was tried: logging builds from 0.11.0 to
/// 0.11.3-rc.2 let an awake session that reached 30 s on a busy wrist
/// without a reading go on to 90 s, on half the rounds of tries. On the
/// sealed watch (2026-11-11 to 16) 58 sessions went on, and the time past
/// 30 s bought 12 readings for ~3,300 s of LED, 278 s each, where a fresh
/// 30 s session on a busy wrist cost 125 s a reading; replayed without
/// it, those logs read 1,016 times rather than 1,028 for 9% less LED, and
/// 5 of the 12 lost were more than 8 BPM off a Garmin. So the experiment
/// is over, and its LED goes to more tries instead (see
/// [`heart_schedule::OVERDUE_RETRY`]).
///
/// A background session runs all of it however the wrist moves, unless
/// it reads, finds no skin, or has had no sign of a pulse from a busy
/// wrist by 19 s (see [`heart_schedule::hopeless`]): a busy wrist may
/// settle within it, and its samples under movement, in a field log, are
/// the data a motion-tolerant estimator needs (see `heart_schedule`'s
/// "Trying through movement").
const SESSION_LIMIT: Duration = Duration::from_secs(30);

/// Wrist movement (see [`activity::movement`]) above which the PPG can't be
/// trusted: moving the sensor against the skin changes the light far more
/// than the pulse does.
///
/// With the accelerometer read correctly (0.8.0 on): ~30 lying still,
/// 2000–7000 when handled. Earlier field logs, on a corrupted movement
/// measure, vetoed 98% of on-wrist time, including every window that
/// passed the signal checks, though those matched a Garmin worn alongside
/// to a median +0.4 BPM.
///
/// Also what [`activity::active_polls`] counts as vigorous movement, for
/// the schedule.
///
/// TODO: tune from a 0.8.x field log with walking in it.
pub const MOTION_LIMIT: u16 = 1500;

/// Wrist movement (see [`activity::movement`]) at or below which a
/// background session may start, once it has held for
/// [`heart_schedule::START_CALM_POLLS`] polls. A due session waits for
/// that for a while, as a busy wrist rarely reads, but not indefinitely
/// (see [`heart_schedule::CALM_WAIT`]). Above it, the wrist counts as busy
/// for [`heart_schedule::calm_throughout`].
///
/// On the wrist the pulse is tiny (±5 counts on ~3000), and sitting at a
/// desk, a hand's small movements make bursts ten times that every few
/// seconds, so no window gets the ~12 s of calm a reading needs. In a
/// field log compared against a Garmin (0.8.13, 14 sessions), all four
/// readings came once movement had dropped to 30–130; sessions spent at
/// 300–2500 throughout never read. Two sessions at 75–240 failed too:
/// calm is necessary, not sufficient.
pub const CALM: u16 = 150;

/// Accelerometer polls in a row (2 s each) without steps, up to now, for
/// two strongly periodic windows in a row to make a reading by themselves
/// (see `ppg::Consensus::push_strong`): half a minute, a whole session's
/// length, so none of what either window saw was walked through.
///
/// A walking arm swings at half the step cadence, 50-60 times a minute,
/// which is a plausible pulse, and as periodic as one: on one walk,
/// sessions read 58-60 BPM at a high window quality against a Garmin's
/// 97, the PPG having locked onto the swing. Three accepted windows can
/// agree on that too, as they always could; but the strong pair is new,
/// was only ever seen to help on wrists that weren't stepping (all 9
/// readings it added in the replay of the sealed watch's logs, see
/// `ppg::STRONG_CORRELATION`), and those logs hold too little walking to
/// say what it reads there. So where it's untested, and the likeliest
/// wrong reading is a strongly periodic one, it's left out.
///
/// TODO: judge it on walks recorded against a reference at 1 s (the
/// logging build's `accel:` lines give the cadence to compare with), and
/// then either drop this or reject windows at the wrist's own rhythm.
const STRONG_STEPLESS_POLLS: u8 = 15;

/// How often to look again, while waiting for a background session to
/// fall due, at whether movement has brought it forward. Precision isn't
/// needed (the session waits for a calm wrist anyway), and waking for it
/// is cheap but not free.
const RECHECK_SECS: u32 = 30;

/// Seconds of movement readings kept (one per level check, i.e. a second),
/// to judge a whole window: an analysis window is ~8 s.
const MOTION_HISTORY: usize = 8;

// ==============================================================================
// The HRS3300
// ==============================================================================
//
// Register facts are from the HRS3300 datasheet (TianYiHeXin), and from how
// InfiniTime's and wasp-os's drivers configure it on this watch, which
// fills in what the datasheet leaves vague.

const ADDRESS: u8 = 0x44;
const REG_ID: u8 = 0x00;
/// HEN (bit 7: measure), HWT (bits 6:4: wait between conversions), and
/// PDRIVE[1] (bit 3: LED current, high bit).
const REG_ENABLE: u8 = 0x01;
/// The results: 0x08..=0x0F hold both channels, bits scattered across
/// registers (see `Hrs3300::sample`).
const REG_DATA: u8 = 0x08;
/// PDRIVE[0] (bit 6: LED current, low bit) and PON (bit 5: oscillator on).
const REG_PDRIVER: u8 = 0x0C;
/// ADC resolution: HRS channel in bits 7:4, ALS in bits 3:0.
const REG_RES: u8 = 0x16;
/// HRS gain, in bits 4:2.
const REG_HGAIN: u8 = 0x17;

const CHIP_ID: u8 = 0x21;

const ENABLE_HEN: u8 = 0x80;
/// Wait 12.5 ms between conversions (HWT = 0b110). With 25 ms conversions
/// that's the 37.5 ms period [`ppg::NOMINAL_PERIOD_US`] assumes, ~27 Hz:
/// fine resolution of the beat period (180 BPM is 9 samples), with the LED
/// off a third of the time. (Whether the LED is off during the wait isn't
/// documented.)
const ENABLE_HWT_12_5_MS: u8 = 0x60;
const ENABLE_PDRIVE1: u8 = 0x08;
const PDRIVER_PDRIVE0: u8 = 0x40;
const PDRIVER_PON: u8 = 0x20;
/// The datasheet says these bits are reserved but also recommends 0x8 in
/// them. InfiniTime found that 0xF gives a steady LED drive through each
/// conversion where 0x8 modulates it, so we follow that.
const PDRIVER_LOW_BITS: u8 = 0x0F;
/// 14 bits for both channels: 25 ms per HRS conversion, the datasheet's
/// characterised setting. (InfiniTime uses 15 bits, which takes about twice
/// as long; 14 is plenty for a pulse that's ~1% of the level.)
const RES_14_BIT: u8 = 0x66;
const FULL_SCALE: u32 = (1 << 14) - 1;

/// Far longer than any transfer here takes (8 bytes at 250 kHz: ~0.4 ms).
const I2C_TIMEOUT: Duration = Duration::from_millis(50);

/// How often to look for a new conversion: several times per 37.5 ms
/// period, so each is dated to within a few ms (see [`Conversions`]).
const POLL: Duration = Duration::from_millis(10);

/// One LED current and gain setting.
#[derive(Clone, Copy, defmt::Format)]
struct Level {
    /// PDRIVE: 0..=3 for 12.5, 20, 30, 40 mA.
    drive: u8,
    /// HGAIN: 0..=3 for ×1, ×2, ×4, ×8.
    gain: u8,
}

/// Signal levels, weakest first: [`measure`] steps through these to keep
/// the reflected light well within the ADC's range. Gain goes up first,
/// since it costs no power, while LED current is most of the sensor's
/// draw. The bottom is what InfiniTime runs at fixed (12.5 mA, ×1), so it's
/// known to work on most wrists; the remembered level carries over between
/// sessions.
///
/// These only follow the light level, which on a wrist sits at the second
/// (12.5 mA ×2: 1,600-4,000) whatever the pulse. A background session on
/// the bright arm starts with the drive alone raised, keeping the gain
/// (see [`heart_schedule::bright_start`]).
///
/// TODO: gain ×64 (HGAIN 0b100) exists too; try it if ×8 at 40 mA is ever
/// not enough.
const LEVELS: [Level; 7] = [
    Level { drive: 0, gain: 0 },
    Level { drive: 0, gain: 1 },
    Level { drive: 0, gain: 2 },
    Level { drive: 1, gain: 2 },
    Level { drive: 2, gain: 2 },
    Level { drive: 3, gain: 2 },
    Level { drive: 3, gain: 3 },
];

/// Any sample in a second above this, and the ADC is (nearly) clipping:
/// step down.
const SATURATED: u32 = FULL_SCALE * 9 / 10;
/// A second's average below this, and the pulse is down to a few counts:
/// step up. A step at most doubles the signal, so this can't bounce
/// straight back into `SATURATED`.
///
/// On the wrist, 12.5 mA ×2 reads 2000–3500, and every window that passed
/// the signal checks in a day's field log was at that level. At
/// `FULL_SCALE / 8` (2047), contact wobble kept bumping sessions to ×4,
/// which restarts the analysis and never yielded a window.
const WEAK: u32 = FULL_SCALE / 16;
/// At the lowest current (×1), a mean below this is no skin: off the wrist
/// read 73–223, on it 1076–1631. Scaled by the gain at higher gains.
///
/// TODO: check in daylight; these were indoors and on a charger.
const NO_SKIN: u32 = FULL_SCALE / 32;
/// Below this even at the top level, almost no light is coming back: the
/// watch isn't being worn.
///
/// TODO: check against real off-wrist readings, which depend on ambient
/// light reaching the sensor.
const NO_CONTACT: u32 = FULL_SCALE / 64;
/// Samples per signal-level check: about a second.
const LEVEL_CHECK_SAMPLES: u32 = 27;

// ==============================================================================
// What the numbers above promise each other
// ==============================================================================
//
// Checked when compiling, so they cost the watch nothing. This task's logic
// is tangled up with the sensor and the clock, where no host test or proof
// reaches it and a release build has no debug assertions, so what can be
// said of its constants is said here. (What it computes is checked in
// `heart_schedule` and `ppg`, which the host tests run.)

// `LEVELS` is weakest first, a step at a time: each raises either the gain
// by one setting (twice the signal) or the drive by one (at most twice the
// current, see `WEAK`), and both stay settings: two bits each in the
// registers `Hrs3300::on` packs them into, and `DRIVE_CURRENT`'s index.
const _: () = {
    let mut i = 0;
    while i < LEVELS.len() {
        let level = LEVELS[i];
        assert!(level.drive <= 3 && level.gain <= 3);
        if i > 0 {
            let weaker = LEVELS[i - 1];
            let gain_up = level.drive == weaker.drive && level.gain == weaker.gain + 1;
            let drive_up = level.gain == weaker.gain && level.drive == weaker.drive + 1;
            assert!(gain_up || drive_up);
            let current = heart_schedule::DRIVE_CURRENT;
            assert!(current[level.drive as usize] <= 2 * current[weaker.drive as usize]);
        }
        i += 1;
    }
    // Sessions start at the second level, and the lowest drive is where
    // "no skin" is told from skin.
    assert!(LEVELS.len() > 1 && LEVELS[0].drive == 0 && LEVELS[1].drive == 0);
};
// Stepping up from a weak signal can't land in saturation, nor the other
// way round, or the level would bounce between two for the whole session.
const _: () = assert!(NO_CONTACT < NO_SKIN && NO_SKIN < WEAK && 2 * WEAK < SATURATED);
// A bright start's predicted peak stays clear of where the level check
// steps back down.
const _: () = assert!(heart_schedule::BRIGHT_PEAK_LIMIT < SATURATED);
// The budget's reserve is a whole session (see `SESSION_RESERVE`).
const _: () = assert!(SESSION_LIMIT.as_secs() == heart_schedule::SESSION_RESERVE as u64);
// A session gives up for want of a window only once several could have
// come (the first takes a window's worth of samples, the rest 2 s each),
// and before its time is up.
const _: () = assert!(
    heart_schedule::HOPELESS_CHECKS as u64 * LEVEL_CHECK_SAMPLES as u64
        >= crate::ppg::WINDOW as u64 + 5 * 2 * LEVEL_CHECK_SAMPLES as u64
);
const _: () = assert!((heart_schedule::HOPELESS_CHECKS as u64) < SESSION_LIMIT.as_secs());
// A level check is a second of samples at the sensor's nominal rate, with
// room in `measure`'s `second` for the few more a late poll completes; and
// a second's sum of 18-bit samples fits its `u32`.
const _: () = assert!(LEVEL_CHECK_SAMPLES == 1_000_000u32.div_ceil(crate::ppg::NOMINAL_PERIOD_US));
const _: () = assert!(LEVEL_CHECK_SAMPLES as usize + 8 <= SECOND_SAMPLES);
// The movement kept, a reading a second, spans one analysis window.
const _: () = assert!(
    MOTION_HISTORY as u64
        == (crate::ppg::WINDOW as u64 * crate::ppg::NOMINAL_PERIOD_US as u64).div_ceil(1_000_000)
);
// The sensor is polled several times a conversion (see `Conversions`).
const _: () = assert!(POLL.as_micros() * 3 <= crate::ppg::NOMINAL_PERIOD_US as u64);
// The schedule's buckets are the ones the history is sent in.
const _: () = assert!(heart_schedule::BUCKET as i64 == crate::gb_buckets::MINUTES * 60);
// The calm a session waits for is stricter than what vetoes a window.
const _: () = assert!(CALM < MOTION_LIMIT);

/// Room for a second's samples, for the field log (see `measure`).
const SECOND_SAMPLES: usize = 40;

struct Hrs3300 {
    i2c: Device,
    /// Whether to keep transfer failures out of the log: while setup is
    /// retried slowly, only some attempts log (see [`crate::backoff`]).
    quiet: bool,
}

impl Hrs3300 {
    /// Writes one register. Every transfer has a timeout: a wedged bus must
    /// not wedge this task (or, via the shared bus, touch and steps).
    async fn write(&mut self, register: u8, value: u8) -> Result<(), ()> {
        match with_timeout(I2C_TIMEOUT, self.i2c.write(ADDRESS, &[register, value])).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                if !self.quiet {
                    warn!("heart: write to {=u8:#04x} failed: {}", register, e);
                }
                crate::i2c::reset_controller().await;
                Err(())
            }
            Err(_) => {
                if !self.quiet {
                    warn!("heart: write to {=u8:#04x} timed out", register);
                }
                crate::i2c::reset_controller().await;
                Err(())
            }
        }
    }

    async fn read(&mut self, register: u8, buffer: &mut [u8]) -> Result<(), ()> {
        match with_timeout(
            I2C_TIMEOUT,
            self.i2c.write_read(ADDRESS, &[register], buffer),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                if !self.quiet {
                    warn!("heart: read of {=u8:#04x} failed: {}", register, e);
                }
                crate::i2c::reset_controller().await;
                Err(())
            }
            Err(_) => {
                if !self.quiet {
                    warn!("heart: read of {=u8:#04x} timed out", register);
                }
                crate::i2c::reset_controller().await;
                Err(())
            }
        }
    }

    /// Checks the sensor is there, and turns it off (whatever the previous
    /// firmware, e.g. InfiniTime, left it doing).
    async fn init(&mut self) -> Result<(), ()> {
        let mut id = [0];
        self.read(REG_ID, &mut id).await?;
        if id[0] != CHIP_ID {
            // Only the datasheet vouches for this value, and the sensor
            // answered at its address, so carry on regardless.
            warn!("heart: unexpected sensor ID {=u8:#04x}", id[0]);
        }
        self.off().await
    }

    /// Starts measuring at `level` (or switches to it).
    async fn on(&mut self, level: Level) -> Result<(), ()> {
        self.write(REG_RES, RES_14_BIT).await?;
        self.write(REG_HGAIN, level.gain << 2).await?;
        // PDRIVE's two bits live in different registers.
        let pdrive0 = (level.drive & 1) * PDRIVER_PDRIVE0;
        let pdrive1 = (level.drive >> 1 & 1) * ENABLE_PDRIVE1;
        self.write(REG_PDRIVER, PDRIVER_PON | pdrive0 | PDRIVER_LOW_BITS)
            .await?;
        self.write(REG_ENABLE, ENABLE_HEN | ENABLE_HWT_12_5_MS | pdrive1)
            .await
    }

    /// Stops measuring, and stops the oscillator too (PON off), as
    /// InfiniTime does, for the lowest idle current.
    async fn off(&mut self) -> Result<(), ()> {
        self.write(REG_ENABLE, ENABLE_HWT_12_5_MS).await?;
        self.write(REG_PDRIVER, 0).await
    }

    /// [`Hrs3300::off`], tried up to `tries` times; whether it worked.
    /// Every failed transfer resets the bus controller (see
    /// [`Hrs3300::write`]), so the next try may well work. Only the first
    /// try logs its failure.
    async fn off_tries(&mut self, tries: u8) -> bool {
        let quiet = self.quiet;
        let mut off = false;
        for _ in 0..tries {
            off = self.off().await.is_ok();
            if off {
                break;
            }
            self.quiet = true;
        }
        self.quiet = quiet;
        off
    }

    /// The latest conversion: (HRS, ALS) channel values.
    ///
    /// One burst read across all eight result registers, since the bits of
    /// each value are scattered among them (the datasheet's tables 4 and 5).
    /// The sensor may update between bytes of the burst; [`Conversions`]
    /// copes with the occasional torn value that results.
    async fn sample(&mut self) -> Result<(u32, u32), ()> {
        let mut r = [0u8; 8];
        self.read(REG_DATA, &mut r).await?;
        let at = |register: u8| u32::from(r[usize::from(register - REG_DATA)]);
        // HRS (CH0): 0x09 holds bits 15:8, 0x0A bits 7:4 (in its low
        // nibble), 0x0F bits 3:0 (low nibble) and 17:16 (bits 5:4).
        let hrs =
            at(0x09) << 8 | (at(0x0A) & 0x0F) << 4 | (at(0x0F) & 0x0F) | (at(0x0F) & 0x30) << 12;
        // ALS (CH1): 0x0D holds bits 17:11, 0x08 bits 10:3, 0x0E bits 2:0.
        let als = (at(0x0D) & 0x7F) << 11 | at(0x08) << 3 | (at(0x0E) & 0x07);
        Ok((hrs, als))
    }
}

// ==============================================================================
// The task
// ==============================================================================

/// Switches the sensor off for System OFF (see [`crate::system_off`]),
/// through the bus `system_off` holds, whatever the heart-rate task was
/// doing: the sensor keeps its state while the nRF52 is off, so a session
/// cut short would otherwise leave the LED lit until the battery is flat.
/// Each write is tried a few times, with the bus recovered between tries
/// (see [`crate::i2c::write_held`]).
pub async fn switch_off(twim: &mut embassy_nrf::twim::Twim<'static>) {
    for write in [[REG_ENABLE, ENABLE_HWT_12_5_MS], [REG_PDRIVER, 0]] {
        if !crate::i2c::write_held(twim, ADDRESS, &write, I2C_TIMEOUT).await {
            warn!("heart: switching the sensor off failed");
        }
    }
}

#[embassy_executor::task]
pub async fn heart_task(i2c: Device) {
    // Stay off the bus while it's busiest: touch configures itself right
    // at boot, and the accelerometer's setup (from 200 ms) holds the bus
    // for about a third of a second.
    Timer::after_millis(1000).await;
    let mut hrs = Hrs3300 { i2c, quiet: false };
    // For as long as it takes, ever more slowly: giving up would leave a
    // sealed watch without heart rate until its next reboot (see
    // `crate::backoff`).
    let mut backoff = crate::backoff::Backoff::new();
    loop {
        hrs.quiet = !backoff.loud();
        if hrs.init().await.is_ok() {
            break;
        }
        let retry = backoff.failed();
        if retry.log {
            warn!(
                "heart: sensor setup failed ({=u16}x); retrying in {=u32} ms",
                retry.failures, retry.wait_ms
            );
        }
        Timer::after_millis(u64::from(retry.wait_ms)).await;
    }
    hrs.quiet = false;
    info!("heart: sensor ready");

    // The level the last *successful* session ended at: the wrist (and the
    // sensor's oscillator) don't change much between sessions. A failed
    // session (off the wrist, typically) climbs to the top level, 40 mA, and
    // starting every later session there would triple the LED's cost.
    // Starting at ×2 (12.5 mA): where on-wrist readings settled anyway, so
    // the first second isn't spent climbing to it.
    let mut good_level = 1;
    // The light on skin: the last second's peak of the last session that
    // didn't end off the wrist, scaled to 12.5 mA ×1 (0: none yet), to
    // tell whether a bright arm's start would saturate. Unknown, it starts
    // bright anyway, and steps back down if it saturates.
    let mut skin_peak = 0u32;
    // When to measure in the background. Its first session is due at once,
    // so the clock face has a number soon after boot.
    let mut schedule = Schedule::new();
    let mut conversions = Conversions::new();
    // Whether the sensor may still be on, its LED lit (12.5–40 mA):
    // switching it off after the last session failed, likeliest after one
    // that lost the sensor mid-session. Left so, it would stay lit until
    // the next session, up to an hour away when sessions keep failing, or
    // for as long as someone watched. So it's tried again every second
    // until it works (see `retry_off`): a failed try is cheap, at most a
    // timed-out transfer and a controller reset.
    let mut maybe_lit = false;

    loop {
        // Wait until a background session is due and the wrist is still,
        // or until someone looks at the clock face. Movement can bring the
        // session forward while we wait, so the due time is looked at
        // again every `RECHECK_SECS` while the wrist moves (while it's
        // still, nothing can, and `activity` wakes us when it starts
        // moving); once due, every 3 s, as movement is only updated with
        // each read of the accelerometer, and at once when it sees the
        // wrist go still. (A wake left over from during the last session is
        // stale: everything is looked at afresh.) A due session waits for
        // calm only so long, though (see `heart_schedule::Schedule::start`),
        // counted from when it was first found due.
        let mut due_since = None;
        let due = loop {
            WAKE.reset();
            if maybe_lit {
                maybe_lit = !retry_off(&mut hrs).await;
            }
            // Never on the charger, whoever's watching. The UI loop wakes
            // this when its power comes or goes; the timer is only a
            // backstop.
            let charger = match schedule.charger(uptime(), on_charger()) {
                Charger::Away => None,
                Charger::Arrived => {
                    info!("heart: on the charger");
                    // Not worn, for sleep tracking and the temperature:
                    // no session will say so while it charges.
                    crate::heart_reading::set_worn(Some(false));
                    Some(RECHECK_SECS)
                }
                Charger::On => Some(RECHECK_SECS),
                // A wobbly cradle's contact back: logged, so that every
                // "off the charger" has its "on" before it.
                Charger::Back => {
                    info!("heart: back on the charger");
                    Some(RECHECK_SECS)
                }
                Charger::Left(wait) => {
                    info!("heart: off the charger");
                    Some(wait)
                }
                Charger::Settling(wait) => Some(wait),
            };
            if let Some(wait) = charger {
                due_since = None;
                let wait = if maybe_lit { 1 } else { wait };
                select(Timer::after_secs(u64::from(wait)), WAKE.wait()).await;
                continue;
            }
            if watching() {
                break None;
            }
            let now = uptime();
            // Where the phone's 5-minute buckets fall, on the schedule's
            // clock, once the watch knows the time: asleep, a session is
            // timed to finish in its bucket's last minute (see
            // `heart_schedule::in_phase`). Worked out afresh at each look,
            // so a clock the phone has since corrected is followed.
            let bucket = crate::clock::now_utc_seconds().map(|utc| {
                (i64::from(now) - utc).rem_euclid(i64::from(heart_schedule::BUCKET)) as u32
            });
            let (at, why) =
                schedule.due_in_phase(activity::active_polls(), activity::asleep(), bucket);
            let wait = if now < at {
                due_since = None;
                // Not due, but a reading may be late enough that a calm
                // moment is worth taking by itself (see `heart_schedule`'s
                // "Seeking calm"). `activity` wakes this at the poll that
                // makes the wrist calm, so the look costs nothing until
                // then; if the wrist is calm already as the seeking
                // starts, the wait below ends then.
                let seek = schedule.seek(activity::asleep());
                if seek.is_some_and(|from| now >= from)
                    && activity::calm_polls() >= heart_schedule::START_CALM_POLLS
                {
                    break Some((heart_schedule::Why::Calm, Start::Calm, 0));
                }
                let wait = match seek {
                    Some(from) if from > now => (at - now).min(from - now),
                    _ => at - now,
                };
                if activity::still() {
                    wait
                } else {
                    wait.min(RECHECK_SECS)
                }
            } else {
                let since = *due_since.get_or_insert(now);
                // How long it waited for calm, for the log.
                let waited = (now - since).min(255) as u8;
                match schedule.start(now, since, activity::calm_polls()) {
                    // A calm start reads far more often, and a moment's
                    // calm at a desk rarely lasts long enough.
                    Start::Wait => 3,
                    start @ Start::Calm => break Some((why, start, waited)),
                    start @ Start::Busy => {
                        // So a field log can tell these sessions apart
                        // (the `due` line below says so too; this one is
                        // what analyses of earlier logs look for).
                        info!("heart: busy start ({=u8} s)", waited);
                        break Some((why, start, waited));
                    }
                }
            };
            // Back within a second while the sensor may be on, to try
            // switching it off again.
            let wait = if maybe_lit { 1 } else { wait };
            select(Timer::after_secs(u64::from(wait)), WAKE.wait()).await;
        };
        let background = due.is_some();
        let asleep = activity::asleep();
        let mut level = good_level;
        let base = LEVELS[level].drive;
        let budget = schedule.budget(uptime());
        // The round of tries and its arm (a watched session is in neither),
        // and the drive it starts at.
        let (round, arm) = match due {
            Some(_) => schedule.begin(asleep),
            None => (0, Arm::Dim),
        };
        let drive = match arm {
            Arm::Bright => {
                let peak = (skin_peak << LEVELS[level].gain)
                    * heart_schedule::DRIVE_CURRENT[usize::from(base)]
                    / heart_schedule::DRIVE_CURRENT[0];
                heart_schedule::bright_start(base, peak, budget).unwrap_or(base)
            }
            Arm::Dim | Arm::Asleep => base,
        };
        if let Some((why, start, waited)) = due {
            let (awake, asleep) = schedule.resting();
            // Everything a field log needs to judge the schedule by,
            // without another watch to compare with: why the session was
            // due, whether it found the calm it waited for and how long
            // that took, and how long ago the last reading was (half an
            // hour or more: overdue). How it ended, and its LED time, are
            // on the line that ends it. The first part is as it has been
            // since 0.10, for what reads older logs.
            info!(
                "heart: due ({}); budget {} s, resting {}/{}; round {} {} drive {}; {=str} start after {=u8} s, {=u16} min since a reading",
                why,
                budget,
                awake,
                asleep,
                round,
                arm,
                drive,
                if start == Start::Calm { "calm" } else { "busy" },
                waited,
                (schedule.since_reading(uptime()) / 60).min(u32::from(u16::MAX)) as u16
            );
        }
        // A workout's session (or one looked at during a workout): said
        // here, as it has no `due` line, and ends on a line of its own.
        let workout = activity::workout();
        if let Some(kind) = workout {
            info!("heart: workout session ({})", kind);
        }
        // Movement from here on counts towards the next session.
        let before = Before {
            active_polls: activity::take_active_polls(),
            // A workout's reading is never a resting one, steps or none
            // (see `heart_schedule::RESTING_STEPLESS_POLLS`).
            stepless_polls: if workout.is_some() {
                0
            } else {
                activity::stepless_polls()
            },
            asleep,
        };

        let started = Instant::now();
        crate::power::count(crate::power::Count::HeartSessions);
        // Asleep, the wrist's movement is turning over, which isn't what
        // the experiment is after: no samples (~50 KB of log a night).
        #[cfg(feature = "field-log")]
        if !asleep {
            ACCEL.store(ACCEL_SESSION, Ordering::Relaxed);
        }
        // What the session spends of the budget, at 12.5 mA's rate (see
        // `heart_schedule::budget_seconds`): a bright start costs more.
        let mut spent = 0;
        let mut peak = 0;
        let outcome = measure(
            &mut hrs,
            &mut level,
            drive,
            &mut conversions,
            // Awake, and the schedule's: such a session ends early if it's
            // getting nowhere (see `heart_schedule::hopeless`).
            background && !asleep,
            &mut spent,
            &mut peak,
        )
        .await;
        // The accelerometer's next samples still overlap the session.
        #[cfg(feature = "field-log")]
        if !asleep {
            ACCEL.store(ACCEL_TAIL, Ordering::Relaxed);
        }
        if matches!(
            outcome,
            Ok(Ended::Measured(_) | Ended::Faded | Ended::Charger(Some(_)))
        ) {
            good_level = level;
        }
        // (Not the light on a cradle, or in the hand that put it there.)
        if !matches!(
            outcome,
            Ok(Ended::NoContact | Ended::TooBright | Ended::Charger(_)) | Err(())
        ) && peak > 0
        {
            skin_peak = peak;
        }
        // A few tries at once, then every second (see `maybe_lit`). Only
        // the first try's failed transfer is logged, then this line, and
        // one more when it works, however many tries that takes.
        //
        // The power meter counts the LED off from here regardless (see
        // `LedMeter`): it's a fault's cost, not a session's.
        maybe_lit = !hrs.off_tries(3).await;
        if maybe_lit {
            warn!("heart: couldn't turn the sensor off; retrying every second");
        }
        // Rounded up: the budget should err on the side of caution.
        let led_seconds = started.elapsed().as_millis().div_ceil(1000) as u32;
        // A session that a workout was part of, at its start or its end:
        // its length says how long the sensor was on for it.
        let workout = workout.is_some() || activity::workout().is_some();
        if workout {
            info!("heart: workout session over after {} s", led_seconds);
        }
        match outcome {
            Ok(Ended::Measured(_) | Ended::Faded) => {}
            // A clear line, for field-log analyses to leave it out (see
            // `scripts/ppg_sessions.py`).
            Ok(Ended::Charger(_)) => {
                info!("heart: on the charger; stopped after {} s", led_seconds)
            }
            Ok(Ended::TimedOut { calm: false }) => {
                info!("heart: no reading after {} s", led_seconds)
            }
            Ok(Ended::TimedOut { calm: true }) => {
                info!("heart: no reading after {} s; wrist calm", led_seconds)
            }
            // "No reading after", like the two above, so that whatever
            // counts those counts this; and how busy, of its 19 checks.
            Ok(Ended::Hopeless { busy }) => info!(
                "heart: no reading after {} s; stopped early (busy {}/{})",
                led_seconds,
                busy,
                heart_schedule::HOPELESS_CHECKS
            ),
            Ok(Ended::NoContact) => info!("heart: not on a wrist (after {} s)", led_seconds),
            Ok(Ended::TooBright) => info!("heart: too much light (after {} s)", led_seconds),
            Err(()) => warn!("heart: lost the sensor mid-session"),
        }
        // A pulse means skin. No light coming back, or ambient light
        // flooding in, means the sensor isn't against anything. Light
        // without a pulse proves nothing: a desk reflects the LED too (the
        // dev kit lying on one times out exactly like a busy wrist).
        // At the lowest drive, `spent` is `led_seconds`, short of the
        // moments around the LED's switching on and off.
        let spent = spent.max(led_seconds);
        // A workout's session costs the budget one session's worth,
        // however long it ran (see "Through a workout").
        let spent = if workout {
            spent.min(heart_schedule::SESSION_RESERVE)
        } else {
            spent
        };
        // A session that read, though not lately, is no failure of the
        // schedule's either, should the schedule have started it (and a
        // look at the clock face, or a workout, then kept it going).
        let background = background && !matches!(outcome, Ok(Ended::Faded));
        let (worn, outcome) = match outcome {
            // What it read before the charger came stands.
            Ok(Ended::Measured(bpm) | Ended::Charger(Some(bpm))) => {
                (Some(true), Outcome::Reading(bpm))
            }
            Ok(Ended::NoContact | Ended::TooBright) => (Some(false), Outcome::OffWrist),
            // Skin, as a pulse was read; but no reading to tell the
            // schedule of now (not as a background session's, see above,
            // so this counts as neither a measurement nor a failure).
            Ok(Ended::Faded) => (Some(true), Outcome::NoReading),
            Ok(Ended::TimedOut { calm: true }) => (None, Outcome::CalmNoReading),
            Ok(Ended::TimedOut { calm: false } | Ended::Hopeless { .. }) | Err(()) => {
                (None, Outcome::NoReading)
            }
            // Without a reading, it's nothing to the schedule but its LED
            // time (see `Schedule::cut_short`), which also hears of the
            // charger, so the wait below won't take this for an arrival.
            Ok(Ended::Charger(None)) => {
                crate::heart_reading::set_worn(Some(false));
                schedule.cut_short(uptime(), spent);
                continue;
            }
        };
        crate::heart_reading::set_worn(worn);
        schedule.ended(uptime(), spent, outcome, background, before);

        // A session only ends while someone's watching if there's no
        // signal to be had (off the wrist, typically); don't start another
        // until they look away, or Gadgetbridge stops asking.
        //
        // Deliberately no retrying while Gadgetbridge asks, in case the
        // watch goes on a wrist meanwhile: not measuring off the wrist is
        // what's wanted, and reopening the chart tries again.
        //
        // On the charger, the wait below looks after it instead.
        //
        // A workout does look again, every `WORKOUT_RETRY`, for as long as
        // nobody's watching: nobody is there to look away, and a watch
        // that shifted on the wrist mid-walk is worth another try.
        let mut waited = 0u8;
        while watching() && !on_charger() {
            Timer::after_secs(1).await;
            if maybe_lit {
                maybe_lit = !retry_off(&mut hrs).await;
            }
            waited = waited.saturating_add(1);
            if held() && waited >= WORKOUT_RETRY {
                break;
            }
        }
    }
}

/// One more, quiet, try at switching off a sensor that may still be on
/// (see `maybe_lit` in [`heart_task`]); whether it worked, which is
/// logged.
async fn retry_off(hrs: &mut Hrs3300) -> bool {
    hrs.quiet = true;
    let off = hrs.off().await.is_ok();
    hrs.quiet = false;
    if off {
        info!("heart: sensor off at last");
    }
    off
}

/// Seconds since boot, the schedule's clock.
fn uptime() -> u32 {
    Instant::now().as_secs() as u32
}

/// Accounts the LED's on-time to its drive current, for [`crate::power`]
/// and the schedule's budget (`spent`, see
/// [`heart_schedule::budget_seconds`]): on from creation until dropped.
struct LedMeter<'a> {
    drive: u8,
    /// When the LED went on at `drive`.
    since: Instant,
    /// The budget spent at earlier drives.
    spent: &'a mut u32,
}

impl<'a> LedMeter<'a> {
    fn new(drive: u8, spent: &'a mut u32) -> Self {
        crate::power::on(Self::meter(drive));
        Self {
            drive,
            since: Instant::now(),
            spent,
        }
    }

    fn switch(&mut self, drive: u8) {
        crate::power::off(Self::meter(self.drive));
        crate::power::on(Self::meter(drive));
        *self.spent = self.spent();
        self.drive = drive;
        self.since = Instant::now();
    }

    /// The budget spent so far.
    fn spent(&self) -> u32 {
        let millis = self.since.elapsed().as_millis() as u32;
        *self.spent + heart_schedule::budget_seconds(self.drive, millis)
    }

    fn meter(drive: u8) -> crate::power::Meter {
        use crate::power::Meter;
        [
            Meter::HeartLed0,
            Meter::HeartLed1,
            Meter::HeartLed2,
            Meter::HeartLed3,
        ][usize::from(drive & 3)]
    }
}

impl Drop for LedMeter<'_> {
    fn drop(&mut self) {
        crate::power::off(Self::meter(self.drive));
        *self.spent = self.spent();
    }
}

/// Why a session ended.
enum Ended {
    /// We have a reading (and nobody's watching for more): the last, in
    /// BPM.
    Measured(u8),
    /// A session that was watched ended more than [`FRESH_CHECKS`] after
    /// its last reading: it read (so there's skin), but what it read is no
    /// longer the heart rate now. The readings themselves were published
    /// as they came; this only keeps the schedule from taking a workout's
    /// reading of twenty minutes ago for one made as the session ends.
    Faded,
    /// No reading within [`SESSION_LIMIT`]; `calm`
    /// if the wrist was calm through most of it (see
    /// [`heart_schedule::calm_throughout`]).
    TimedOut { calm: bool },
    /// No reading, and no sign of one, on a wrist `busy` at this many of
    /// its first [`heart_schedule::HOPELESS_CHECKS`] level checks: stopped
    /// there rather than run out the time (see [`heart_schedule::hopeless`]).
    Hopeless { busy: u8 },
    /// Almost no light comes back even at the highest level.
    NoContact,
    /// The charger's power arrived (see [`heart_reading::on_charger`]);
    /// with the session's reading, if it had one by then.
    ///
    /// [`heart_reading::on_charger`]: crate::heart_reading::on_charger
    Charger(Option<u8>),
    /// The ADC clips even at the lowest level: ambient light is getting in.
    TooBright,
}

/// One measurement session: turn the sensor on, sample until we have a
/// reading (and keep going while someone's watching), adjusting the level
/// as needed. It starts at `drive`, the level's or a bright arm's (see
/// [`Arm`]). `stop_early`: whether it may end before its time if it's getting nowhere
/// (see [`heart_schedule::hopeless`]), which an awake background session
/// may. `spent` is set to what the session cost the budget, and `skin_peak` to
/// its last second's peak, scaled to 12.5 mA ×1.
async fn measure(
    hrs: &mut Hrs3300,
    level: &mut usize,
    drive: u8,
    conversions: &mut Conversions,
    stop_early: bool,
    spent: &mut u32,
    skin_peak: &mut u32,
) -> Result<Ended, ()> {
    // The LED's drive: `LEVELS[*level]`'s, unless the session started
    // brighter (see `heart_schedule::bright_start`), keeping the gain.
    let mut drive = drive;
    let at = |level: usize, drive: u8| Level {
        drive,
        gain: LEVELS[level].gain,
    };
    hrs.on(at(*level, drive)).await?;
    // Ends the LED's on-time when the session returns, however it returns.
    let mut led = LedMeter::new(drive, spent);
    conversions.restart();
    let mut ppg = Ppg::new();
    let mut consensus = Consensus::new();
    let started = Instant::now();
    // The latest reading, once there is one, and how many level checks
    // (seconds) ago it came; saturating.
    let mut measured = None;
    let mut since_read = 0u8;
    // Windows since the last accepted one; saturating (see `AGREE_WITHIN`).
    let mut unaccepted = 0u8;
    // Whether a second's samples go in the log: not late in a workout's
    // session (see `WORKOUT_TRACE_CHECKS`), unless someone looks at the
    // clock face, which brings them back for as long as they look. (Asked
    // afresh each time rather than kept: this task's state is static RAM.)
    let traced = |checks: u8| !(held() && checks >= WORKOUT_TRACE_CHECKS);
    // For judging whether the wrist was calm through the session: the level
    // checks so far, and how many found it busy.
    let (mut checks, mut busy) = (0u8, 0u8);
    // Whether any window so far was accepted or faint: a sign of a pulse,
    // without which a busy wrist's session ends early (see
    // `heart_schedule::hopeless`).
    let mut hopeful = false;
    // This window's movement, a reading a second (the accelerometer
    // reports every 2 s, so about four distinct values).
    let mut movement = heapless::HistoryBuf::<u16, MOTION_HISTORY>::new();
    // This second's signal, for the level check.
    let (mut peak, mut sum, mut count) = (0u32, 0u32, 0u32);
    // And the samples themselves, for the field log at `trace`: replaying
    // real wrists through `ppg` (host-tests/ppg) is how its limits get
    // tuned. About 50 bytes a second while measuring, with the level check
    // (see `trace_level_samples`). (Room for a few more
    // than `LEVEL_CHECK_SAMPLES`, as one poll can complete several.)
    let mut second = heapless::Vec::<u16, SECOND_SAMPLES>::new();

    let mut ticker = Ticker::every(POLL);
    loop {
        ticker.next().await;
        let now = Instant::now();
        let elapsed = now - started;
        // On the charger: off at once, whatever the session was doing (the
        // UI loop looks at the charger at least once a second). The last
        // part-second goes into the log, as for any end.
        if on_charger() {
            if traced(checks) {
                trace_samples(&second);
            }
            return Ok(Ended::Charger(measured));
        }
        let ended = if watching() {
            None
        } else if let Some(bpm) = measured {
            Some(if since_read < FRESH_CHECKS {
                Ended::Measured(bpm)
            } else {
                Ended::Faded
            })
        } else if elapsed >= SESSION_LIMIT {
            // Out of time, and that's the end of it. (Up to 0.11.3-rc.2 a
            // session on a calm wrist with a faint pulse went on here for
            // 15 s with a brighter LED, once or twice: 1 of 12 such phases
            // read. See `heart_schedule::bright_start`.)
            Some(Ended::TimedOut {
                calm: heart_schedule::calm_throughout(busy, checks),
            })
        } else {
            None
        };
        if let Some(ended) = ended {
            // The last part-second too, which holds the end of the last
            // window: without it, a replay is a window short.
            if traced(checks) {
                trace_samples(&second);
            }
            return Ok(ended);
        }

        let (raw, ambient) = hrs.sample().await?;
        let mut verdict = None;
        conversions.poll(raw, now.as_micros(), |sample| {
            peak = peak.max(sample);
            sum += sample;
            count += 1;
            // Full only if conversions stalled and then caught up, which
            // the replay can live without.
            let _ = second.push(sample as u16);
            if let Some(v) = ppg.push(sample) {
                verdict = Some(v);
            }
        });

        // Once a second: is the signal within range? If not, change level
        // and start over (samples at different levels aren't comparable).
        if count >= LEVEL_CHECK_SAMPLES {
            let moved = activity::movement();
            movement.write(moved);
            checks = checks.saturating_add(1);
            since_read = since_read.saturating_add(1);
            if moved > CALM {
                busy = busy.saturating_add(1);
            }
            // A workout's session stops logging its samples after a
            // while (and the accelerometer's with them, in the logging
            // build), and says so once, so that what reads the log knows
            // the session went on. (`checks` stops counting at 255, so it
            // passes the limit once.)
            let traced = traced(checks);
            if checks == WORKOUT_TRACE_CHECKS && !traced {
                info!("heart: workout session; samples no longer logged");
            }
            // A workout is awake, so its session's accelerometer samples
            // were wanted (see `heart_task`).
            #[cfg(feature = "field-log")]
            if activity::workout().is_some() {
                ACCEL.store(
                    if traced { ACCEL_SESSION } else { ACCEL_IDLE },
                    Ordering::Relaxed,
                );
            }
            let mean = sum / count;
            // A dev kit can be told that its desk is skin (see
            // `debug::skin_forced`): as far as the level goes, the light
            // is then always enough, neither stepping up nor ending the
            // session for no skin. Half the ADC's range is above `WEAK`
            // and every no-skin threshold, scaled to any drive at the
            // gains it applies to (at most 6,540, at ×4 and 40 mA). (The
            // saturation check stays: it goes by the peak.)
            #[cfg(feature = "debug-input")]
            let mean = if crate::debug::skin_forced() {
                mean.max(FULL_SCALE / 2)
            } else {
                mean
            };
            #[cfg(not(feature = "field-log"))]
            if traced {
                debug!(
                    "heart: level {}, mean {}, peak {}, ambient {}, period {} µs",
                    at(*level, drive),
                    mean,
                    peak,
                    ambient,
                    conversions.period_us()
                );
                trace_samples(&second);
            }
            #[cfg(feature = "field-log")]
            if traced {
                trace_level_samples(at(*level, drive), ambient, conversions.period_us(), &second);
            }
            second.clear();
            let before = (*level, drive);
            if peak >= SATURATED {
                if drive != LEVELS[*level].drive {
                    // Too bright after all: back to the session's level,
                    // for good.
                    drive = LEVELS[*level].drive;
                } else if *level == 0 {
                    return Ok(Ended::TooBright);
                } else {
                    *level -= 1;
                }
            } else if LEVELS[*level].drive == 0
                && mean * heart_schedule::DRIVE_CURRENT[0]
                    < (NO_SKIN << LEVELS[*level].gain)
                        * heart_schedule::DRIVE_CURRENT[usize::from(drive)]
            {
                // Off the wrist: little light comes back at the lowest
                // current (scaled to a brighter one); skin reflects
                // several times more.
                return Ok(Ended::NoContact);
            } else if mean < WEAK && *level + 1 < LEVELS.len() {
                *level += 1;
            } else if mean < NO_CONTACT {
                return Ok(Ended::NoContact);
            }
            if *level != before.0 {
                // Any brighter drive was the old level's.
                drive = LEVELS[*level].drive;
            }
            *skin_peak = (peak >> LEVELS[before.0].gain) * heart_schedule::DRIVE_CURRENT[0]
                / heart_schedule::DRIVE_CURRENT[usize::from(before.1)];
            (peak, sum, count) = (0, 0, 0);
            // Getting nowhere on a busy wrist: stop here, and leave the
            // LED time to a try at a better moment. Looked at once, at
            // this check, and after the checks above, so that a session
            // off the wrist still ends as that. (This second's samples are
            // logged already; there's no part-second to add.)
            if stop_early
                && checks == heart_schedule::HOPELESS_CHECKS
                && !hopeful
                && measured.is_none()
                && !watching()
                && heart_schedule::hopeless(busy, checks)
            {
                return Ok(Ended::Hopeless { busy });
            }
            if (*level, drive) != before {
                debug!("heart: switching to level {}", at(*level, drive));
                hrs.on(at(*level, drive)).await?;
                led.switch(drive);
                conversions.restart();
                ppg.reset();
                // Windows either side aren't in a row, to pair.
                consensus.reject();
                continue;
            }
        }

        let Some(verdict) = verdict else {
            continue;
        };
        // Accepted windows only agree with recent ones (see
        // `AGREE_WITHIN`): after a session's length without one, start
        // over. (Counted here, before this window is judged; an accepted
        // one resets it below.)
        unaccepted = unaccepted.saturating_add(1);
        if unaccepted > AGREE_WITHIN {
            consensus.expire();
            unaccepted = 0;
        }
        // A faint window (see `Rejection::Faint`) only counts on a calm
        // wrist, alongside the next; the movement vetoes below can't apply
        // to one, as calm is stricter than either.
        let calm = movement.iter().all(|&m| m <= CALM);
        let (estimate, faint) = match verdict {
            Ok(estimate) => (estimate, false),
            Err(Rejection::Faint(estimate)) => (estimate, true),
            Err(why) => {
                debug!("heart: window rejected: {}", why);
                consensus.reject();
                continue;
            }
        };
        let bpm = estimate.bpm_at(conversions.period_us());
        if faint {
            hopeful = true;
            debug!(
                "heart: window faint: {} BPM (quality {}, calm {})",
                bpm, estimate.quality, calm
            );
            if let Some(bpm) = consensus.push_faint(bpm, calm, activity::asleep()) {
                measured = Some(read(bpm, measured, started, at(*level, drive), conversions));
                since_read = 0;
            }
            continue;
        }
        // Only sustained movement vetoes a window: most of its readings
        // high, not one poll's spike (which used to veto the next 11 s).
        let moving = movement.iter().filter(|&&m| m > MOTION_LIMIT).count();
        if moving * 2 > movement.len() {
            debug!(
                "heart: window rejected: wrist moving ({} BPM, {}/{})",
                bpm,
                moving,
                movement.len()
            );
            consensus.reject();
            continue;
        }
        // Nor a slow one while the wrist was merely busy, awake: that's
        // more likely the sensor shifting than a resting pulse.
        if crate::ppg::slow_while_busy(bpm, movement.as_slice(), CALM, activity::asleep()) {
            debug!("heart: window rejected: slow while busy ({} BPM)", bpm);
            consensus.reject();
            continue;
        }
        // Nor one at the arm's swing, on a walk (see `workout::swing`).
        if crate::workout::swing(
            bpm,
            activity::workout(),
            activity::stepless_polls(),
            STRONG_STEPLESS_POLLS,
        ) {
            debug!("heart: window rejected: walking ({} BPM)", bpm);
            consensus.reject();
            continue;
        }
        hopeful = true;
        unaccepted = 0;
        debug!(
            "heart: window {} BPM (quality {}, drift {})",
            bpm, estimate.quality, estimate.drift
        );
        // Both ways to agree are told of the window. A strong pair only
        // counts on a wrist that isn't stepping (see
        // `STRONG_STEPLESS_POLLS`), and the usual agreement comes first.
        let strong = consensus
            .push_strong(bpm, estimate.quality)
            .filter(|_| activity::stepless_polls() >= STRONG_STEPLESS_POLLS);
        let usual = consensus.push(bpm, calm);
        if usual.is_none() && strong.is_some() {
            // So a field log says which readings the rule made.
            debug!("heart: strong pair");
        }
        if let Some(bpm) = usual.or(strong) {
            measured = Some(read(bpm, measured, started, at(*level, drive), conversions));
            since_read = 0;
        }
    }
}

/// Publishes a reading, logging the session's first; returns it, as the
/// session's latest.
fn read(
    bpm: u8,
    measured: Option<u8>,
    started: Instant,
    level: Level,
    conversions: &Conversions,
) -> u8 {
    if measured.is_none() {
        info!(
            "heart: {} BPM after {} s (level {}, conversion period {} µs)",
            bpm,
            started.elapsed().as_secs(),
            level,
            conversions.period_us()
        );
    } else {
        debug!("heart: {} BPM", bpm);
    }
    publish(bpm);
    bpm
}

// ==============================================================================
// The samples through each session (at trace)
// ==============================================================================

/// Logs a part-second of samples (a session's end), for the replay (host-tests/ppg, through
/// `scripts/ppg_sessions.py`).
///
/// A field build packs them as differences (see [`crate::log_pack`]):
/// `heart: packed samples [..] movement M`, which `scripts/expand_log.py`
/// turns back into the `heart: samples [..] movement M` other builds log.
fn trace_samples(second: &[u16]) {
    #[cfg(not(feature = "field-log"))]
    trace!(
        "heart: samples {=[?]} movement {}",
        second,
        activity::movement()
    );
    #[cfg(feature = "field-log")]
    {
        let mut packed = [0u8; crate::log_pack::PACKED_PER_SAMPLE * SECOND_SAMPLES];
        let len = crate::log_pack::pack_samples(second, &mut packed);
        trace!(
            "heart: packed samples {=[u8]} movement {=u16}",
            packed[..len],
            activity::movement()
        );
    }
}

/// Logs a second's level check and its samples in a field build, as one
/// line where other builds log two (`heart: level …` at debug, then
/// `heart: samples …`; `scripts/expand_log.py` turns it back into those):
/// the level as drive/gain, the ambient light, the conversion period, and
/// the samples packed as in [`trace_samples`]. The check's mean and peak
/// are left out: they're the samples' own (their mean, rounded down, and
/// their largest; true of every check in 0.11.1's log), which the expander
/// works out. (Not so on a dev kit told its desk is skin, whose check
/// raises the mean: see `debug::skin_forced`.)
///
/// At trace, with the samples, so a field build that logs this module at
/// debug has neither.
#[cfg(feature = "field-log")]
fn trace_level_samples(level: Level, ambient: u32, period_us: f32, second: &[u16]) {
    let mut packed = [0u8; crate::log_pack::PACKED_PER_SAMPLE * SECOND_SAMPLES];
    let len = crate::log_pack::pack_samples(second, &mut packed);
    trace!(
        "heart: level {=u8}/{=u8}, ambient {=u32}, period {=f32} µs, packed samples {=[u8]} movement {=u16}",
        level.drive,
        level.gain,
        ambient,
        period_us,
        packed[..len],
        activity::movement()
    );
}

// ==============================================================================
// The accelerometer through each session (the logging build)
// ==============================================================================

/// Whether the accelerometer's samples are wanted in the log: not between
/// sessions, through a session, or once more after one (whose last
/// samples are only read then).
#[cfg(feature = "field-log")]
static ACCEL: AtomicU8 = AtomicU8::new(ACCEL_IDLE);
#[cfg(feature = "field-log")]
const ACCEL_IDLE: u8 = 0;
#[cfg(feature = "field-log")]
const ACCEL_SESSION: u8 = 1;
#[cfg(feature = "field-log")]
const ACCEL_TAIL: u8 = 2;

/// Logs the accelerometer's samples from one of `activity`'s polls (every
/// 2 s), if they overlap an awake heart rate session: for replaying the PPG
/// against the wrist's movement offline, to find a busy wrist's rhythm
/// (steps, the arm's swing) and test rejecting what the PPG reads at it
/// (see [`STRONG_STEPLESS_POLLS`] for why). Here rather than in `activity` for the log's
/// `ur::heart=trace` (`scripts/release.sh`); only in the logging build.
///
/// `accel: [x, y, z, x, y, z, …]`: the poll's samples, oldest first, at
/// the FIFO's rate (nominally 12.5 Hz, but by the accelerometer's own
/// clock: 11.94 Hz on the sealed watch, so a replay should measure it from
/// the lines' times and lengths), the newest a moment before the line's
/// time, in 1/64 g (±2 g, as the chip measures, is ±128, so each axis
/// fits a byte), the same as a capture build's `activity: samples` (see
/// `activity::trace_samples`). That's ~84 bytes a poll (rzCOBS-encoded,
/// as 0.11.3-rc.2's log has them), ~42 a second of session, against
/// the ~50 of the PPG's own (see `trace_level_samples`); ~24 samples a
/// line, or fewer when a read failed or the FIFO was flushed. The
/// movement's fundamental (a walk's ~1.8 Hz of steps, or ~0.9 Hz of arm
/// swing) is well below the ~6 Hz this resolves.
///
/// Unless the wrist was at rest through the poll (see
/// [`crate::log_pack::still`]: no axis ranged over a quarter of a g, nor
/// the magnitude over an eighth), which has no rhythm to find. Then the
/// line is `accel: still, N samples within 8 of [x, y, z]` instead: that
/// there was a poll, how many samples it had (for the sample rate, and so
/// that a replay knows it's missing nothing else), and where the wrist
/// lay. That's ~12 bytes rather than ~84, for 56% of the polls in 0.11.3-rc.2's
/// log, 13% of all its bytes. An extractor that reads `accel: [` lines
/// doesn't see these, and should take the gap for stillness, not for a
/// poll that's missing.
#[cfg(feature = "field-log")]
pub fn trace_accel(frames: &[[i16; 3]]) {
    let wanted = match ACCEL.load(Ordering::Relaxed) {
        ACCEL_IDLE => false,
        ACCEL_SESSION => true,
        // The poll after a session: the last it overlaps. (Unless a new
        // session has started meanwhile, which stays wanted.)
        _ => {
            let _ = ACCEL.compare_exchange(
                ACCEL_TAIL,
                ACCEL_IDLE,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            true
        }
    };
    if !wanted {
        return;
    }
    // A FIFO read holds at most 36 frames (`activity::drain_fifo`).
    let mut raw = [0i8; 3 * 36];
    let samples = frames.len().min(36);
    for (out, &axis) in raw.iter_mut().zip(frames[..samples].as_flattened()) {
        *out = (axis >> 4).clamp(-128, 127) as i8;
    }
    let (logged, _) = raw[..3 * samples].as_chunks::<3>();
    match crate::log_pack::still(logged) {
        Some([x, y, z]) => trace!(
            "accel: still, {=u8} samples within 8 of [{=i8}, {=i8}, {=i8}]",
            samples as u8, x, y, z
        ),
        None => trace!("accel: {=[?]}", raw[..3 * samples]),
    }
}
