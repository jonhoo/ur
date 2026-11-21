//! Steps and movement, from the BMA421 accelerometer.
//!
//! Two things come out of the accelerometer:
//!
//! - **Steps**, counted by the chip itself. The BMA421 has a small
//!   "feature engine" (a microcontroller programmed by a 6 KB blob Bosch
//!   publishes) that runs a step counter at very low power, so we never see
//!   the raw motion behind it. We read its running count and keep today's
//!   total. It runs Bosch's parameters for a wrist (see
//!   [`STEP_COUNTER_PARAMS`]) but for one (see [`STEP_BUFFER_SIZE`]), and
//!   counts the way their wrist algorithm does: by the arm's swing, two
//!   steps a swing, and only once a few swings in a row have shown it's a
//!   walk, when it adds them all at once. Bosch's wrist set waits for
//!   four swings (8 steps); we ask for three (6 steps). So a walk of
//!   fewer than 6 steps, or a walk without an arm swinging (hands full,
//!   pushing a pram), counts nothing, and stop-and-go walking indoors
//!   loses much of what it does count: with Bosch's four swings, every
//!   run of counting in the field logs started with 8 to 12 steps at
//!   once, and a third of the runs stopped right there.
//! - **Movement**: how much the wrist moved each minute, which is what sleep
//!   tracking runs on. The chip queues accelerometer samples in its FIFO at
//!   12.5 Hz; every few seconds we drain it and add up how far the
//!   acceleration *vector* moved between samples. Using the vector (not just
//!   its length) matters: rolling over in bed changes the direction of
//!   gravity but not its strength.
//!
//! The last sample of each poll also says which way up the watch lies,
//! which tells a desk (see [`crate::desk`]): its minutes are off the
//! wrist, so never sleep.
//!
//! Both are kept per minute, for the last 14½ hours in RAM ([`History`])
//! and the last 11 days in the SPI flash ([`crate::spill`]), which is what
//! Gadgetbridge fetches as activity samples, and today's step total is
//! shown on the clock face. The history also holds each minute's heart
//! rate, recorded by [`crate::heart`], and the heart-rate measurement asks
//! [`movement`] whether the wrist is still enough to trust.
//!
//! The feature engine also watches for motion starting and stopping
//! ("any-motion" and "no-motion"), and raises its interrupt pin (INT1, on
//! P0.08) when the wrist starts moving. [`motion_task`] uses that to stop
//! polling while the wrist is still ([`still`]): the glance detector only
//! needs samples once something moves, and the heart rate's schedule can't
//! change meanwhile. (This config has no other gestures: no taps, and no
//! wrist-turn, which is why glances are recognised in [`crate::glance`].)

use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering};

use defmt::{debug, info, warn};
use embassy_futures::select::{Either, select};
use embassy_nrf::gpio::Input;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Timer, with_timeout};
use embedded_hal_async::i2c::I2c;

use crate::i2c::Device;
use crate::step_counter::{Read, StepCounter};
use crate::{aged, clock, desk, minute_ring, sleep, sleep_hold, stages, workout};

// ==============================================================================
// The BMA421
// ==============================================================================
//
// Register addresses and sequences follow Bosch's BMA4 SensorAPI (bma4.c,
// bma423.c), as used by InfiniTime for this chip.

const ADDRESS: u8 = 0x18;
const REG_CHIP_ID: u8 = 0x00;
/// The current acceleration, x/y/z (LSB first), 12 bits left-justified.
const REG_ACC_DATA: u8 = 0x12;
/// Which feature interrupts fired since it was last read (which clears
/// it, and the latched pin): the `INT_*` bits.
const REG_INT_STATUS_0: u8 = 0x1C;
const REG_STEP_COUNT: u8 = 0x1E;
const REG_FIFO_LENGTH: u8 = 0x24;
const REG_FIFO_DATA: u8 = 0x26;
const REG_INTERNAL_STATUS: u8 = 0x2A;
const REG_ACC_CONF: u8 = 0x40;
const REG_ACC_RANGE: u8 = 0x41;
const REG_FIFO_DOWNSAMPLING: u8 = 0x45;
const REG_FIFO_CONFIG_1: u8 = 0x49;
const REG_INT1_IO_CTRL: u8 = 0x53;
const REG_INT_LATCH: u8 = 0x55;
/// Which feature interrupts drive INT1 and INT2 (the same bits as
/// `INT_STATUS_0`). A feature's status is only set while it's mapped to
/// one of them.
const REG_INT1_MAP: u8 = 0x56;
const REG_INT2_MAP: u8 = 0x57;
const REG_INIT_CTRL: u8 = 0x59;
const REG_ASIC_LSB: u8 = 0x5B;
const REG_ASIC_MSB: u8 = 0x5C;
const REG_FEATURES: u8 = 0x5E;
const REG_POWER_CONF: u8 = 0x7C;
const REG_POWER_CTRL: u8 = 0x7D;
const REG_CMD: u8 = 0x7E;
const CMD_FIFO_FLUSH: u8 = 0xB0;

const CHIP_ID: u8 = 0x11;
/// Far longer than any transfer here takes (the longest, 252 bytes of FIFO
/// at 250 kHz, takes ~9 ms).
const I2C_TIMEOUT: Duration = Duration::from_millis(100);
/// Feature-engine configuration: Bosch's BMA423 config file, which the
/// BMA421 also runs. See `bma423_config.LICENSE` (BSD-3-Clause).
static CONFIG: &[u8; 6144] = include_bytes!("bma423_config.bin");
/// The feature-configuration window behind `REG_FEATURES`.
const FEATURES_LEN: usize = 70;
/// Within it: the step counter's settings (offset 0x3A) high byte, whose
/// bit 4 enables counting.
const STEP_COUNTER_ENABLE: (usize, u8) = (0x3A + 1, 0x10);
/// Within it too: the step counter's 25 parameters (little-endian words),
/// which the config's defaults set to Bosch's wrist set: the
/// `BMA423_WRIST_SC_PARAM_*` values that older versions of Bosch's BMA423
/// driver wrote from `bma423_select_platform(BMA423_WRIST_CONFIG)`, which
/// called it the default (the other set being for a phone). They're the
/// bytes at 0x208 into `bma423_config.bin`, and the window reads them back
/// on the dev kit.
///
/// Bosch documents none of them for this chip, and warns that they depend
/// on each other. But that driver's phone set is, value for value, the
/// defaults of the BMI323's step counter, whose header (`bmi3_defs.h`, in
/// Bosch's BMI323_SensorAPI) names its fields. Matched by those values,
/// the first seven here are:
///
/// | # | at   | wrist  | phone  | BMI323's field                           |
/// |---|------|--------|--------|------------------------------------------|
/// | 1 | 0x08 | 301    | 306    | `env_min_dist_up`: upper peak threshold  |
/// | 2 | 0x0A | 0x7BD4 | 0x78E6 | `env_coef_up`: its decay                 |
/// | 3 | 0x0C | 315    | 132    | `env_min_dist_down`: lower peak threshold|
/// | 4 | 0x0E | 0x7ADB | 0x6C9C | `env_coef_down`: its decay               |
/// | 5 | 0x10 | 4      | 7      | `step_buffer_size` ([`STEP_BUFFER_SIZE`])|
/// | 6 | 0x12 | 0x7B3F | 0x7564 | `mean_val_decay`                         |
/// | 7 | 0x14 | 0x6CCD | 0x7EAA | `mean_step_dur`                          |
///
/// (The BMI323's struct has `step_buffer_size` after `mean_step_dur`; the
/// values say this window has it before.) The rest are unidentified, and
/// we change only the fifth. Setup logs them all as the chip reads them
/// back (at debug), so a field log shows what a watch ran.
const STEP_COUNTER_PARAMS: core::ops::Range<usize> = 0x08..0x3A;
/// Within those, the fifth: "minimum number of consecutive steps to be
/// detected for updating step count" (`bmi3_defs.h`), and what we set it
/// to. With the wrist set, what it counts are arm swings, two steps each:
/// under Bosch's 4, none of the 1,258 runs of counting in a week of field
/// logs opened with fewer than 8 steps, and 97% opened with 8, 10 or 12
/// (the 8 held back, and what the same 2 s poll added).
///
/// 3 makes that 6 steps, for the stop-and-go walking that 4 loses.
/// Against a Garmin on the other wrist, five days mostly about the house
/// counted 73% of its steps, while continuous walks matched it; and the
/// runs' lengths are cut off at their peak (284 of 8 steps, 176 of 10, 140
/// of 12), which puts roughly 300 more at 6. On an afternoon of counted
/// walks with the samples logged, where the count was right it stays so:
/// 200 counted steps came to 196–214 with the arm swinging, in a pocket or
/// carrying, on either wrist, bouts of 16–80 steps came to 97–99%, and a
/// walk that passes four swings passes three. What 3 adds is the bouts of
/// 6 or 7 steps: 6 to 11 of those that afternoon, 1% of its steps, but
/// 6–11% of those in its stop-and-go stretches.
///
/// The price is in what isn't walking: three swings' worth of arm
/// movement now counts 6 steps, so the ~160 steps a day counted where the
/// Garmin saw none should about double.
///
/// To judge it from a field log alone, `scripts/step_runs.py` gives the
/// runs' opening bursts and lengths: runs opening with 6 say the chip
/// counts by this, and the runs of 6 steps and no more are what it added.
///
/// TODO: 2 (a 4-step filter) would add about as much again, with three to
/// four times the false steps; try it only once this is measured.
const STEP_BUFFER_SIZE: (usize, u16) = (0x10, 3);
/// The any-motion and no-motion settings (offsets 0x00 and 0x04), two
/// little-endian words each: the threshold (11 bits, in 1/2048 g), then the
/// duration (13 bits, in 20 ms steps) with the axes to watch in its top
/// three bits. See [`ANY_MOTION`] and [`NO_MOTION`].
const ANY_MOTION_OFFSET: usize = 0x00;
const NO_MOTION_OFFSET: usize = 0x04;
const MOTION_ALL_AXES: u16 = 0xE000;
/// The settings end there, with the step counter's, at 0x3C: this config
/// has no taps and no wrist-wear gesture (unlike the BMA456's wearable one,
/// whose layout Bosch's BMA423 driver and InfiniTime's copy of it
/// describe, with those at 0x3C..0x42 and an axis remap at 0x44). Read on
/// the dev kit, 0x3C.. holds 4e 14 88 00 57 00 71 00 7a 00, and writes
/// there don't stick. An earlier attempt at the wrist-wear gesture wrote
/// its "enable" to 0x40 and its axis remap to 0x44, which is why it never
/// fired.
const SETTINGS_LEN: usize = 0x3C;

/// Feature interrupt bits, in `INT_STATUS_0` and the `INT*_MAP`s.
const INT_ANY_MOTION: u8 = 0x20;
const INT_NO_MOTION: u8 = 0x40;
/// `INTERNAL_STATUS`'s error flags, as Bosch's BMA456 documents them: the
/// sample rate too low for the features (0x40), or a bad axis remap (0x20).
/// Not 0x80 ("too low for taps"): it's set with this config whatever the
/// features, and there are no taps.
const INTERNAL_ERRORS: u8 = 0x60;

/// A motion feature's settings: a threshold in 1/2048 g, and how many
/// samples at 50 Hz (20 ms) it must hold for.
struct Motion {
    threshold: u16,
    samples: u16,
}

/// Any-motion: the acceleration differs from a reference by more than
/// this, for this long, on any axis. The reference is the acceleration
/// when it last fired (per Bosch's BMA456 notes), so a slow turn of the
/// wrist adds up and fires too. Bosch's default threshold (83 mg, the pull
/// of gravity tilted ~5°), held for 100 ms: turning the wrist to look takes
/// a few hundred.
const ANY_MOTION: Motion = Motion {
    threshold: 0xAA,
    samples: 5,
};

/// No-motion: every axis changes by less than this, for this long. While
/// that holds, the chip raises its status again every sample (20 ms), so
/// it's only read as a status, never raises INT1.
///
/// The dev kit lying on a desk holds 10 mg throughout, and 5 mg with
/// gaps of seconds; this is twice the former, and well under
/// [`ANY_MOTION`]'s threshold, so the two don't take turns.
///
/// TODO: check on a wrist, with the field log's `still` time: a wrist at
/// rest may rarely be this still, which only costs polling, as before.
const NO_MOTION: Motion = Motion {
    threshold: 0x28,
    samples: 150,
};

// The feature window's layout, checked when compiling (so it costs the
// watch nothing): the settings lie within the window, the motion features
// before the step counter's parameters and clear of each other, and each
// motion setting fits its bits (11 of threshold, 13 of duration), as a
// duration any longer would switch axes on or off.
const _: () = assert!(SETTINGS_LEN <= FEATURES_LEN);
const _: () = assert!(STEP_COUNTER_PARAMS.end <= STEP_COUNTER_ENABLE.0);
const _: () = assert!(STEP_COUNTER_ENABLE.0 < SETTINGS_LEN);
const _: () = assert!(ANY_MOTION_OFFSET + 4 <= NO_MOTION_OFFSET);
const _: () = assert!(NO_MOTION_OFFSET + 4 <= STEP_COUNTER_PARAMS.start);
const _: () = assert!(
    STEP_COUNTER_PARAMS.start <= STEP_BUFFER_SIZE.0
        && STEP_BUFFER_SIZE.0 + 2 <= STEP_COUNTER_PARAMS.end
);
const _: () = assert!(ANY_MOTION.threshold < 1 << 11 && ANY_MOTION.samples < 1 << 13);
const _: () = assert!(NO_MOTION.threshold < 1 << 11 && NO_MOTION.samples < 1 << 13);
// No-motion's threshold is under any-motion's, so the two don't take turns.
const _: () = assert!(NO_MOTION.threshold < ANY_MOTION.threshold);

/// Writes `motion` into the feature window at `offset`, all axes enabled.
fn set_motion(window: &mut [u8], offset: usize, motion: &Motion) {
    window[offset..offset + 2].copy_from_slice(&motion.threshold.to_le_bytes());
    window[offset + 2..offset + 4]
        .copy_from_slice(&(motion.samples | MOTION_ALL_AXES).to_le_bytes());
}

/// Upload granularity: the config goes over I2C in pieces this size, each
/// preceded by its position (in 16-bit words). Even, and small enough to
/// stage in RAM (the TWIM can't send from flash).
const CHUNK: usize = 32;
// Even, and the config a whole number of them, each at a word address
// that fits the two address registers (4 bits and 8).
const _: () = assert!(CHUNK.is_multiple_of(2) && CONFIG.len().is_multiple_of(CHUNK));
const _: () = assert!(CONFIG.len() / 2 <= 1 << 12);

/// One FIFO read's samples (see [`Bma421::drain_fifo`]).
type Frames = heapless::Vec<[i16; 3], 36>;

struct Bma421 {
    i2c: Device,
    /// Whether to keep transfer failures out of the log: while setup is
    /// retried slowly, only some attempts log (see [`crate::backoff`]).
    quiet: bool,
}

impl Bma421 {
    async fn write(&mut self, register: u8, value: u8) -> Result<(), ()> {
        self.write_bytes(&[register, value]).await
    }

    /// Writes `[register, data…]`. Every transfer has a timeout: a wedged
    /// bus must not wedge this task (or, via the shared bus, touch).
    async fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), ()> {
        match with_timeout(I2C_TIMEOUT, self.i2c.write(ADDRESS, bytes)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                if !self.quiet {
                    warn!("activity: write to {=u8:#04x} failed: {}", bytes[0], e);
                }
                crate::i2c::reset_controller().await;
                Err(())
            }
            Err(_) => {
                if !self.quiet {
                    warn!("activity: write to {=u8:#04x} timed out", bytes[0]);
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
                    warn!("activity: read of {=u8:#04x} failed: {}", register, e);
                }
                crate::i2c::reset_controller().await;
                Err(())
            }
            Err(_) => {
                if !self.quiet {
                    warn!("activity: read of {=u8:#04x} timed out", register);
                }
                crate::i2c::reset_controller().await;
                Err(())
            }
        }
    }

    /// Brings the chip from any state (including whatever InfiniTime left
    /// behind) to counting steps and filling the FIFO.
    async fn init(&mut self) -> Result<(), ()> {
        let mut id = [0];
        self.read(REG_CHIP_ID, &mut id).await?;
        if id[0] != CHIP_ID {
            warn!(
                "activity: unexpected accelerometer chip ID {=u8:#04x}",
                id[0]
            );
            return Err(());
        }

        // Soft reset, so every boot starts from the same state. (It also
        // zeroes the step counter; today's total survives in RAM instead.)
        self.write(REG_CMD, 0xB6).await?;
        // Neither Bosch's driver (which waits nothing) nor InfiniTime
        // (1 ms) waits longer than this: 5 ms is margin from chasing setup
        // hangs, never shown to be needed.
        Timer::after_millis(5).await;
        // The reset can leave the I2C controller wedged: the next transfer, the
        // write to `REG_POWER_CONF` below, then hangs. That was nearly every
        // setup failure in the field and dev kit logs that the bus itself
        // didn't cause (always that write, ~5 ms after the reset, and a fresh
        // attempt usually worked). InfiniTime resets its TWI master after this
        // chip's soft reset for the same reason ("the motion sensor chip most
        // probably crashed it"). So reset ours now, as after a failed transfer,
        // which also frees SDA should the chip have come out of its reset
        // holding it (and says so).
        crate::i2c::reset_controller().await;

        // Upload the feature engine's program: advanced power save off
        // (the chip can't take writes this fast otherwise), loading off,
        // stream the blob, loading on.
        self.write(REG_POWER_CONF, 0x00).await?;
        // The datasheet asks for 450 µs after a write in power-save mode; right
        // after a soft reset, the next write still stalled at that gap.
        Timer::after_millis(2).await;
        self.write(REG_INIT_CTRL, 0x00).await?;
        let mut staging = [0u8; CHUNK + 1];
        for (i, chunk) in CONFIG.chunks(CHUNK).enumerate() {
            let word = i * CHUNK / 2;
            self.write(REG_ASIC_LSB, (word & 0x0F) as u8).await?;
            self.write(REG_ASIC_MSB, (word >> 4) as u8).await?;
            staging[0] = REG_FEATURES;
            staging[1..=chunk.len()].copy_from_slice(chunk);
            self.write_bytes(&staging[..=chunk.len()]).await?;
        }
        self.write(REG_INIT_CTRL, 0x01).await?;
        // As Bosch's driver waits (`bma4_write_config_file`) for the
        // feature engine to initialise, before its status means anything.
        Timer::after_millis(150).await;
        let mut status = [0];
        self.read(REG_INTERNAL_STATUS, &mut status).await?;
        // The low five bits are the engine's message, below the error flags
        // (see `INTERNAL_ERRORS`): 0x01 is "initialised", and anything else
        // (not yet, or a failed load) leaves it without features.
        if status[0] & 0x1F != 0x01 {
            warn!(
                "activity: feature engine didn't start (status {=u8:#04x})",
                status[0]
            );
            return Err(());
        }

        // Enable the step counter and the motion features. The feature
        // window is read and written whole; right after initialisation, the
        // address registers point at its start (as Bosch's driver assumes
        // for full-length accesses).
        let mut features = [0u8; FEATURES_LEN + 1];
        self.read(REG_FEATURES, &mut features[1..]).await?;
        features[0] = REG_FEATURES;
        let window = &mut features[1..];
        // What the config's defaults had for the step buffer, for the log
        // below: Bosch's 4, unless the blob ever changes.
        let default_buffer = window[STEP_BUFFER_SIZE.0];
        window[STEP_BUFFER_SIZE.0..STEP_BUFFER_SIZE.0 + 2]
            .copy_from_slice(&STEP_BUFFER_SIZE.1.to_le_bytes());
        window[STEP_COUNTER_ENABLE.0] |= STEP_COUNTER_ENABLE.1;
        set_motion(window, ANY_MOTION_OFFSET, &ANY_MOTION);
        set_motion(window, NO_MOTION_OFFSET, &NO_MOTION);
        self.write_bytes(&features).await?;
        // Read the settings back: the engine ignores writes it doesn't take
        // without an error, and nothing else would tell.
        let mut written = [0u8; SETTINGS_LEN];
        self.read(REG_FEATURES, &mut written).await?;
        if let Some(at) = (0..SETTINGS_LEN).find(|&i| written[i] != features[1 + i]) {
            warn!(
                "activity: feature setting {=usize:#04x} reads {=u8:#04x}, not {=u8:#04x}",
                at,
                written[at],
                features[1 + at]
            );
            return Err(());
        }
        // The step counter's parameters as the chip now has them, the
        // step buffer among them (the fifth word), so that a field log
        // shows a sealed watch took it. Once a setup: a few a day at most.
        debug!(
            "activity: step counter parameters read back {=[u8]:02x} (step buffer was {=u8})",
            written[STEP_COUNTER_PARAMS], default_buffer
        );

        // Any-motion to INT1 (P0.08; see `motion_task`). Latched: the pin
        // stays high until `INT_STATUS_0` is read, so even a short pulse is
        // seen. Output enabled, push-pull, active high, so the pin needs no
        // pull resistor (which would draw current while it's high).
        // No-motion to INT2, which isn't connected (and whose output stays
        // disabled), only so that its status is set.
        self.write(REG_INT1_IO_CTRL, 0x0A).await?;
        self.write(REG_INT_LATCH, 0x01).await?;
        self.write(REG_INT1_MAP, INT_ANY_MOTION).await?;
        self.write(REG_INT2_MAP, INT_NO_MOTION).await?;

        // Accelerometer: 100 Hz (what the step counter wants), ±2 g,
        // averaging 4 samples, low-power averaging mode, as InfiniTime does.
        self.write(REG_ACC_CONF, 0x28).await?;
        self.write(REG_ACC_RANGE, 0x00).await?;
        // FIFO: filtered accelerometer data only, downsampled 8× to 12.5 Hz
        // (87.5 bytes/s with headers; the 1 KB FIFO holds 11 s).
        self.write(REG_FIFO_DOWNSAMPLING, 0x80 | (3 << 4)).await?;
        // Accelerometer frames, with headers (see `drain_fifo`).
        self.write(REG_FIFO_CONFIG_1, 0x50).await?;
        self.write(REG_POWER_CTRL, 0x04).await?;
        // Advanced power save back on (with FIFO self-wake-up), now that
        // configuration is done.
        self.write(REG_POWER_CONF, 0x03).await?;
        // The feature engine flags a sample rate too low for its features
        // only once it's running with them (which matters if the rate is
        // ever lowered to save power: they need 50 Hz).
        Timer::after_millis(50).await;
        self.read(REG_INTERNAL_STATUS, &mut status).await?;
        if status[0] & INTERNAL_ERRORS != 0 {
            warn!(
                "activity: feature engine unhappy (status {=u8:#04x})",
                status[0]
            );
        }
        Ok(())
    }

    /// Which feature interrupts fired since the last call (`INT_*` bits).
    /// Reading clears them, and lowers the latched INT1.
    async fn interrupts(&mut self) -> Result<u8, ()> {
        let mut status = [0];
        self.read(REG_INT_STATUS_0, &mut status).await?;
        Ok(status[0])
    }

    /// Steps since the last reset (i.e. since boot).
    async fn steps(&mut self) -> Result<u32, ()> {
        let mut bytes = [0u8; 4];
        self.read(REG_STEP_COUNT, &mut bytes).await?;
        Ok(u32::from_le_bytes(bytes))
    }

    /// Reads queued samples, each (x, y, z) in 12-bit counts (1024 per g),
    /// oldest first.
    ///
    /// One I2C transfer only, so at most 36 frames (252 bytes: the nRF52832's
    /// TWIM moves at most 255 bytes per transfer); anything beyond stays
    /// queued for next time. A second back-to-back burst doesn't work: with
    /// advanced power save on, the chip returns filler (`e0 e0 …`) without
    /// removing frames from the FIFO, which looked like data arriving at
    /// twice the configured rate.
    ///
    /// The chip's advanced power save corrupts FIFO reads: bytes go missing
    /// (no I2C error is reported). Headerless, every later frame then came
    /// out shifted, as nonsense (axes rotating, impossible values), and the
    /// movement measure (which sleep tracking and the heart rate motion
    /// veto rely on) was wrong from the first glitch until the next reset.
    /// So power save is suspended around the read (see below), and frames
    /// carry headers anyway: a bad header means a broken batch, which is
    /// dropped and the FIFO flushed, losing a few seconds of samples
    /// instead of everything after.
    async fn drain_fifo(&mut self) -> Result<Frames, ()> {
        let mut length = [0u8; 2];
        self.read(REG_FIFO_LENGTH, &mut length).await?;
        let queued = usize::from(u16::from_le_bytes(length) & 0x3FFF);
        // In a block of its own, so that neither the bytes read nor the
        // frames parsed from them are kept (in this future, and so in the
        // activity task's static RAM) across the flush below: 472 bytes
        // that were, for a wait that needs none of them.
        let broken = {
            let mut buffer = [0u8; 252];
            let take = queued.min(buffer.len());
            if take == 0 {
                return Ok(Frames::new());
            }
            // Advanced power save off for the read, and the 450 µs the chip
            // then needs before the next access. With it on, the FIFO stream
            // came out corrupted (missing frame headers) in most reads of a
            // still watch on the desk; with it off, none were, over minutes.
            self.write(REG_POWER_CONF, 0x00).await?;
            Timer::after_millis(1).await;
            let read = self.read(REG_FIFO_DATA, &mut buffer[..take]).await;
            Timer::after_micros(500).await;
            self.write(REG_POWER_CONF, 0x03).await?;
            read?;

            // Parse first, deliver after, so a broken batch delivers nothing.
            let mut frames = Frames::new();
            let mut i = 0;
            let broken = loop {
                let Some(&header) = buffer[..take].get(i) else {
                    break false;
                };
                match header & 0xFC {
                    // Accelerometer data.
                    0x84 => {
                        let Some(data) = buffer[..take].get(i + 1..i + 7) else {
                            // A frame cut off by the end of our read: it stays
                            // in the FIFO only if we haven't read it, and we
                            // have, so this batch's tail is lost; resync.
                            break true;
                        };
                        let axis = |a: usize| i16::from_le_bytes([data[a], data[a + 1]]) >> 4;
                        let _ = frames.push([axis(0), axis(2), axis(4)]);
                        i += 7;
                    }
                    // Skip (frames dropped by overflow), configuration change,
                    // or "dropped" frames: one parameter byte.
                    0x40 | 0x48 | 0x50 => i += 2,
                    // Sensor time: three bytes.
                    0x44 => i += 4,
                    // Empty FIFO.
                    0x80 => break false,
                    _ => break true,
                }
            };
            if !broken {
                return Ok(frames);
            }
            (buffer[i.min(take - 1)], i)
        };
        let (header, at) = broken;
        warn!(
            "activity: FIFO out of step (header {=u8:#04x} at {}); flushing",
            header, at
        );
        self.write(REG_CMD, CMD_FIFO_FLUSH).await?;
        Ok(Frames::new())
    }
}

/// Stops the accelerometer for System OFF (see [`crate::system_off`]),
/// through the bus `system_off` holds: no sampling, no step counting, in
/// its lowest-power state. (Running, it draws ~50 µA, the most of anything
/// left powered.) Setup at the next boot starts it afresh. Each write is
/// tried a few times, with the bus recovered between tries (see
/// [`crate::i2c::write_held`]).
pub async fn suspend(twim: &mut embassy_nrf::twim::Twim<'static>) {
    // Accelerometer off, then advanced power save on.
    for write in [[REG_POWER_CTRL, 0x00], [REG_POWER_CONF, 0x03]] {
        if !crate::i2c::write_held(twim, ADDRESS, &write, I2C_TIMEOUT).await {
            warn!("activity: suspending the accelerometer failed");
        }
    }
}

// ==============================================================================
// Shared state
// ==============================================================================

/// Per-minute samples for the last 14½ hours: see
/// [`minute_ring::MINUTES`] for why that long.
pub const HISTORY_MINUTES: usize = minute_ring::MINUTES;

/// The hours between the history's oldest minute and a day ago, whose
/// sums [`aged`] keeps, and one more, as neither end falls on the hour.
const AGED_HOURS: usize = (24 * 60 - HISTORY_MINUTES).div_ceil(60) + 1;

/// How far on from the history's newest minute a minute can be filed
/// without more ado. Further on, the history would let go of minutes
/// that the activity log may not have yet (see [`jumps`]).
///
/// Running, the history moves on a minute at a time, and the log is
/// brought up to date every couple of minutes; the clock only jumps when
/// the phone sets it: after the watch was off, above all.
const JUMP: i64 = 15;
// What's not in the log yet is no older than what a fetch may hold back,
// a bucket (the log takes whole ones), and the minutes since the log was
// last brought up to date; a minute filed `JUMP` on must not push that
// out.
const _: () = assert!(
    sleep_hold::CAP
        + JUMP
        + 2 * crate::gb_buckets::MINUTES
        + 2 * crate::snapshot::CHECKPOINT_MINUTES
        <= HISTORY_MINUTES as i64
);

#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub steps: u16,
    /// Average change of the acceleration vector between samples, in
    /// Bangle.js units (8192 per g), since that's what Gadgetbridge expects.
    pub movement: u16,
    /// The heart rate measured during the minute, or 0 if none was (as in
    /// Gadgetbridge, which treats 0 as "not measured").
    pub heart_rate: u8,
    /// Packed into the byte the struct would otherwise pad with (the
    /// history's samples cost the same 5.2 KB either way): the minute's
    /// [`sleep::Kind`] (bits 0–3), whether the watch was worn (bits 4–5: 0
    /// unknown, 1 yes, 2 no, 3 in a workout, see [`Sample::exercise`]),
    /// whether the minute was recorded at all
    /// (bit 6), and whether the wrist fidgeted in it (bit 7: see
    /// [`sleep::FIDGET`]).
    flags: u8,
}

impl Sample {
    const SEEN: u8 = 0x40;
    const FIDGETED: u8 = 0x80;

    pub fn seen(&self) -> bool {
        self.flags & Self::SEEN != 0
    }

    pub fn sleep(&self) -> sleep::Kind {
        sleep::Kind::from_bits(self.flags & 0x0F)
    }

    fn worn(&self) -> Option<bool> {
        match (self.flags >> 4) & 0x3 {
            // A workout's minute too: nothing but a wrist starts one
            // (and what reads this only asks whether it was *off* one).
            1 | 3 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }

    /// Whether the minute was part of a workout (see [`workout`]), for
    /// Gadgetbridge (see [`Sample::gadgetbridge_kind`]).
    ///
    /// Kept as the fourth value of the two bits that say whether the watch
    /// was worn, which had three: the byte has no bit to spare (a byte
    /// more a sample would be 870 of static RAM, of which the `compare`
    /// build had ~100 left when this went in), and its low four are the
    /// classifier's, which rewrites the last hours' every minute. A
    /// workout's minute is never off the wrist (see
    /// [`Sample::set_exercise`]), so nothing is lost by it.
    pub fn exercise(&self) -> bool {
        self.seen() && (self.flags >> 4) & 0x3 == 3
    }

    /// Marks a recorded minute as part of a workout, unless the watch was
    /// off the wrist in it (steps counted in a bag aren't exercise, and
    /// "not worn" is what sleep tracking needs to keep).
    fn set_exercise(&mut self) {
        if self.seen() && self.worn() != Some(false) {
            self.flags |= 0x30;
        }
    }

    /// What Gadgetbridge is told the minute was: its `ActivityKind`'s
    /// name, or `None` for plain activity (its default). A sleep stage, or
    /// not worn, as the classifier has it (see
    /// [`sleep::Kind::gadgetbridge_name`]); else `EXERCISE` for a
    /// workout's minute. Sleep wins where both are said, which only a
    /// sleep dated back over a workout's still last minutes could do.
    ///
    /// What Gadgetbridge (2026.11.1, and master at 93d4d79a17) does with
    /// `EXERCISE` on a sample is very little, and nothing harmful. Its
    /// sleep sessions, and the Health Connect export's, only ask whether
    /// a sample's kind is a sleep one (`SleepSessionSegmenter`), so an
    /// exercise sample ends a session, or doesn't, exactly as a plain one
    /// does; steps and heart rate are summed and exported whatever the
    /// kind (`ActivityAnalysis`, `DailyTotals`, the Health Connect
    /// syncers). Its charts draw every kind that isn't sleep or not-worn
    /// in the activity colour (`AbstractActivityChartFragment`), and its
    /// dashboard's exercise segments come from recorded workouts, not
    /// samples: so the kind is stored, and shows nowhere yet. The one
    /// thing that changes is its "Stats" chart of step-rate zones
    /// (`ActivityAnalysis`), which only counts plain activity samples and
    /// so leaves a workout's steps out.
    ///
    /// TODO: a workout Gadgetbridge shows as one, and Health Connect gets
    /// as an exercise session, takes a recorded workout: the Bangle.js
    /// Recorder's tracks (`actTrksList`, `actTrk`), which we'd have to
    /// emulate. Deliberately not done (user, 2026-11-21).
    pub fn gadgetbridge_kind(&self) -> Option<&'static str> {
        self.sleep()
            .gadgetbridge_name()
            .or(self.exercise().then_some("EXERCISE"))
    }

    fn set_seen(&mut self, worn: Option<bool>) {
        let worn = match worn {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        };
        self.flags = (self.flags & (0x0F | Self::FIDGETED)) | Self::SEEN | (worn << 4);
    }

    /// Marks the minute as one the wrist fidgeted in.
    fn set_fidgeted(&mut self) {
        self.flags |= Self::FIDGETED;
    }

    /// Marks a recorded minute off the wrist, as found on a desk after
    /// the fact (see [`desk`]).
    fn set_off_wrist(&mut self) {
        if self.seen() {
            self.set_seen(Some(false));
        }
    }

    fn set_sleep(&mut self, kind: sleep::Kind) {
        self.flags = (self.flags & 0xF0) | kind as u8;
    }

    /// As kept in a [`crate::snapshot`] record: the fields in order, little
    /// endian.
    pub fn to_bytes(self) -> [u8; 6] {
        let [s0, s1] = self.steps.to_le_bytes();
        let [m0, m1] = self.movement.to_le_bytes();
        [s0, s1, m0, m1, self.heart_rate, self.flags]
    }

    pub fn from_bytes([s0, s1, m0, m1, heart_rate, flags]: [u8; 6]) -> Self {
        Self {
            steps: u16::from_le_bytes([s0, s1]),
            movement: u16::from_le_bytes([m0, m1]),
            heart_rate,
            flags,
        }
    }

    /// The sample as a Gadgetbridge bucket takes it in.
    pub fn bucket_minute(&self) -> crate::gb_buckets::Minute<Option<&'static str>> {
        crate::gb_buckets::Minute {
            steps: self.steps,
            movement: self.movement,
            heart_rate: self.heart_rate,
            seen: self.seen(),
            kind: self.gadgetbridge_kind(),
        }
    }

    fn evidence(&self) -> sleep::Minute {
        sleep::Minute {
            seen: self.seen(),
            movement: self.movement,
            steps: self.steps,
            worn: self.worn(),
            fidgeted: self.flags & Self::FIDGETED != 0,
            pulse: self.heart_rate > 0,
        }
    }
}

const _: () = assert!(size_of::<Sample>() == 6, "flags must fit in the padding");
// The classifier's span is part of the history (with the minute before
// it, for its context), as is what a Gadgetbridge fetch may hold back:
// otherwise either would read the newest minutes' slots as old ones.
const _: () = assert!(sleep::WINDOW + sleep::CONTEXT < HISTORY_MINUTES);
const _: () = assert!(sleep_hold::CAP < HISTORY_MINUTES as i64);

/// A ring of per-minute samples, indexed by UTC minute. Minutes the watch
/// didn't see (off, or before the phone set the clock) stay zero.
///
/// It holds the last [`HISTORY_MINUTES`]: what the sleep tracking works
/// on. Older minutes are in the activity log in the SPI flash, for the
/// phone (see [`crate::spill`]), and summed up in [`History::aged`], for
/// the screens that show a day.
pub struct History {
    samples: [Sample; HISTORY_MINUTES],
    /// The newest minute with a sample (UTC minutes since 1970).
    newest: Option<i64>,
    /// The minutes whose kinds are final, as the wearer was clearly up
    /// after them (see [`sleep_hold::settles`]): neither the classifier
    /// nor the staging touches them again.
    settled: sleep_hold::Settled,
    /// What's kept of the minutes the ring has let go of (see [`aged`]):
    /// the sums of their hours, for the Today screen, and the older part
    /// of the last sleep, for the Last night screen.
    aged: aged::Aged<AGED_HOURS>,
}

impl History {
    const fn new() -> Self {
        Self {
            samples: [Sample {
                steps: 0,
                movement: 0,
                heart_rate: 0,
                flags: 0,
            }; HISTORY_MINUTES],
            newest: None,
            settled: sleep_hold::Settled::NONE,
            aged: aged::Aged::new(),
        }
    }

    fn slot(minute: i64) -> usize {
        // Any real minute fits 32 bits (until the year 10136), which the
        // CPU divides itself; 64 bits it leaves to a routine, and the
        // minute's revision of a night's sleep asks for some thousands of
        // slots, with everything else waiting (see `activity_task`).
        match u32::try_from(minute) {
            Ok(minute) => (minute % HISTORY_MINUTES as u32) as usize,
            Err(_) => minute.rem_euclid(HISTORY_MINUTES as i64) as usize,
        }
    }

    /// The sample for `minute`, clearing any skipped minutes (and the
    /// samples of the minutes the ring no longer reaches back to, which
    /// [`aged`] takes in) on the way; or, if the clock went back, the
    /// minutes after it (see [`crate::minute_ring`]), but for what a
    /// clock trimmed by seconds carries over.
    fn at(&mut self, minute: i64) -> &mut Sample {
        let (stale, newest) = minute_ring::file(self.newest, minute, HISTORY_MINUTES as i64);
        // The common case by far is the minute under way, filed again at
        // every poll: nothing below is for it.
        if self.newest != Some(minute) {
            // Whose hours the sums are by: the wearer's now. (Unknown,
            // after the watch was off: UTC's, which are everyone's but
            // for a few zones; see `aged`.)
            let offset = clock::utc_offset_minutes().unwrap_or(0);
            if self.newest.is_some_and(|newest| minute < newest) {
                info!(
                    "activity: clock went back; dropped {} minutes",
                    stale.end - stale.start
                );
                // What was settled may be among the minutes dropped, or
                // reach past the newest now: start over, holding back as
                // usual.
                self.settled = sleep_hold::Settled::NONE;
                self.aged.back_to(minute, offset);
                // The activity log lets go of the minutes from here on,
                // and takes them in again as they turn out. It would see
                // for itself that the history's newest minute is before
                // its end, but only if it looks before the clock has
                // caught up: after a trim of seconds back over a bucket's
                // end, it looks (every couple of minutes) at a history
                // that's past that again, and kept the bucket as it was
                // before these seconds' steps.
                crate::system_off::relog_from(minute);
            }
            // The minutes the ring lets go of, before their slots are
            // emptied for the new ones.
            for old in minute_ring::let_go(self.newest, minute, HISTORY_MINUTES as i64) {
                self.let_go(old, offset, self.samples[Self::slot(old)]);
            }
        }
        // A clock trimmed by seconds, back over the turn of a minute: what
        // the new minute's first seconds recorded isn't dropped with it
        // (see `minute_ring::carried`). Both minutes are the ring's own,
        // its newest two, so `aged` has neither and isn't touched (its
        // `back_to` above does nothing this side of what it took in).
        let carried =
            minute_ring::carried(self.newest, minute).map(|m| self.samples[Self::slot(m)]);
        for m in stale {
            self.samples[Self::slot(m)] = Sample::default();
        }
        self.newest = Some(newest);
        let sample = &mut self.samples[Self::slot(minute)];
        if let Some(carried) = carried {
            // The steps, and a heart rate if this minute has none. Not
            // the movement, a mean over the minute that the next poll
            // works out afresh, nor the flags, which this minute has its
            // own of.
            sample.steps = sample.steps.saturating_add(carried.steps);
            if sample.heart_rate == 0 {
                sample.heart_rate = carried.heart_rate;
            }
        }
        sample
    }

    /// Hands `minute`'s sample, which the ring is letting go of, to
    /// [`aged`], if anything was recorded in it.
    fn let_go(&mut self, minute: i64, offset: i16, sample: Sample) {
        if sample.seen() || sample.heart_rate > 0 {
            self.aged.take(
                minute,
                offset,
                sample.steps,
                sample.heart_rate,
                sample.sleep(),
            );
        }
    }

    /// Whether filing a sample for `minute` would be a jump (see
    /// [`jumps`]).
    fn jumps(&self, minute: i64) -> bool {
        self.newest.is_some_and(|newest| minute - newest > JUMP)
    }

    /// Samples from `since` (UTC minute) on, oldest first, skipping minutes
    /// without data.
    pub fn since(&self, since: i64) -> impl Iterator<Item = (i64, Sample)> + '_ {
        let newest = self.newest.unwrap_or(i64::MIN);
        let oldest = newest.saturating_sub(HISTORY_MINUTES as i64 - 1).max(since);
        (oldest..=newest)
            .map(|m| (m, self.samples[Self::slot(m)]))
            .filter(|(_, s)| s.seen() || s.heart_rate > 0)
    }

    /// Puts back a sample saved before System OFF, an update or a crash
    /// (see [`crate::system_off`]), without overwriting anything recorded
    /// since boot: normally nothing has been, as the clock is unknown until
    /// the phone syncs, but after a reset that kept the clock, the activity
    /// task may already have recorded the minutes since.
    ///
    /// The flash holds days of minutes, and they're put back oldest first:
    /// each one filed lets go of an older one, into [`aged`], as it would
    /// have running. So the day's sums and the last sleep come back with
    /// the history.
    fn restore(&mut self, minute: i64, sample: Sample) {
        match self.newest {
            Some(newest) if minute <= newest => {
                if newest - minute < HISTORY_MINUTES as i64 {
                    let slot = &mut self.samples[Self::slot(minute)];
                    if !slot.seen() && slot.heart_rate == 0 {
                        *slot = sample;
                    }
                } else {
                    // Older than the ring reaches, as of a newest minute
                    // recorded since boot: straight to where the ring's
                    // oldest go.
                    let offset = clock::utc_offset_minutes().unwrap_or(0);
                    self.let_go(minute, offset, sample);
                }
            }
            _ => *self.at(minute) = sample,
        }
    }

    /// The day so far, hour by hour, as of `utc_minute` for a wearer
    /// `offset` minutes ahead of UTC: the ring's minutes since local
    /// midnight, and the sums of those it has let go of since (see
    /// [`aged`]). Not after `utc_minute`: when the clock went back, the
    /// minutes after it stay in the ring until the next sample is filed
    /// (see [`History::at`]).
    fn day(&self, utc_minute: i64, offset: i16) -> crate::day::Day {
        let midnight = crate::day::midnight(utc_minute, offset);
        let mut day = crate::day::Day::default();
        self.aged.add_to(&mut day, midnight, offset);
        let samples = self.since(midnight).take_while(|&(m, _)| m <= utc_minute);
        for (minute, sample) in samples {
            day.add(
                (minute - midnight) as usize,
                sample.steps,
                sample.heart_rate,
            );
        }
        day
    }

    /// Marks the recorded minutes from `first` to the newest as part of a
    /// workout (see [`Sample::set_exercise`]), as far as the history holds
    /// them.
    fn exercised_since(&mut self, first: i64) {
        let Some(newest) = self.newest else {
            return;
        };
        let oldest = newest - HISTORY_MINUTES as i64 + 1;
        for minute in first.max(oldest)..=newest {
            self.samples[Self::slot(minute)].set_exercise();
        }
    }

    /// Marks the minutes from `first` to the newest off the wrist (see
    /// [`Sample::set_off_wrist`]), as far as the history holds them.
    fn off_wrist_since(&mut self, first: i64) {
        let Some(newest) = self.newest else {
            return;
        };
        let oldest = newest - HISTORY_MINUTES as i64 + 1;
        for minute in first.max(oldest)..=newest {
            self.samples[Self::slot(minute)].set_off_wrist();
        }
    }

    /// Re-classifies the recent past's sleep (see [`sleep`]): the last
    /// [`sleep::WINDOW`] minutes, with [`sleep::CONTEXT`] more before them
    /// for context; but never a settled minute (see
    /// [`sleep_hold::Settled::rewritten`]).
    fn classify_sleep(&mut self) {
        const SPAN: usize = sleep::WINDOW + sleep::CONTEXT;
        let Some(newest) = self.newest else {
            return;
        };
        let start = newest - SPAN as i64 + 1;
        // The minutes before the span, which no pass rewrites any more.
        let before = |ago: usize| self.sample(newest, start - ago as i64);
        let asleep_ago = sleep::asleep_ago(|ago| before(ago).sleep());
        let before_evidence = before(1).evidence();
        let mut evidence = [sleep::Minute::default(); SPAN];
        for (i, minute) in evidence.iter_mut().enumerate() {
            *minute = self.samples[Self::slot(start + i as i64)].evidence();
        }
        let mut kinds = [sleep::Kind::Unknown; SPAN];
        sleep::classify(asleep_ago, before_evidence, &evidence, &mut kinds);
        let rewritten = (self.settled.rewritten(newest) - start) as usize;
        for (i, kind) in kinds.iter().enumerate().skip(rewritten) {
            self.samples[Self::slot(start + i as i64)].set_sleep(*kind);
        }
    }

    /// Settles the night if the wearer is clearly up (see
    /// [`sleep_hold::settles`]): everything before the newest minute.
    /// Returns that minute and the last minute asleep, if it did.
    fn settle(&mut self) -> Option<(i64, i64)> {
        let newest = self.newest?;
        let sample = |minute: i64| self.sample(newest, minute);
        let last = sleep_hold::settles(
            newest,
            self.settled,
            |minute| sample(minute).sleep(),
            |minute| sample(minute).evidence(),
        )?;
        self.settled = sleep_hold::Settled::before(newest);
        Some((newest, last))
    }

    /// Minute `minute`'s sample, if the history (up to `newest`) holds it.
    fn sample(&self, newest: i64, minute: i64) -> Sample {
        if minute > newest || newest - minute >= HISTORY_MINUTES as i64 {
            Sample::default()
        } else {
            self.samples[Self::slot(minute)]
        }
    }

    /// Whether the newest minute is classified as sleep, restless included:
    /// at the classifier's leading edge, any moving minute is `Restless`
    /// until it's clear whether it becomes a wake (after
    /// [`sleep::WAKE_RUN`] minutes), so counting only light and deep sleep
    /// flipped this to awake at every stir (51 times in one night, letting
    /// glances through).
    fn asleep_now(&self) -> bool {
        self.newest
            .is_some_and(|minute| self.samples[Self::slot(minute)].sleep().is_sleep())
    }

    /// The most recent sleep in the history (see [`sleep::last_sleep`]),
    /// staged (see [`stages`]) while it's recent: while it reaches into the
    /// window that [`History::classify_sleep`] just rewrote as light. Once
    /// it's older, its sleep and its stages' inputs are final, and so are
    /// its stages; as they are once it's settled.
    ///
    /// With the part of it that the ring has let go of, by the afternoon
    /// after a night; and with no sleep in the ring, the last one that it
    /// let go of, for a day after its end (see [`aged::Aged::last_sleep`]).
    ///
    /// TODO: a sleep longer than the ring (14½ hours: none in a year of
    /// one wearer's nights came within three) is restaged from where the
    /// ring begins, as if it began there, while it's still under way; the
    /// part before keeps the stages it left with.
    fn last_sleep(&mut self) -> Option<Sleep> {
        let newest = self.newest?;
        let oldest = newest - HISTORY_MINUTES as i64 + 1;
        let recent = sleep::last_sleep(HISTORY_MINUTES, |i| {
            self.samples[Self::slot(oldest + i as i64)].sleep()
        })
        .map(|(start, end, _)| {
            let (start, end) = (oldest + start as i64, oldest + end as i64);
            if self.settled.restaged(newest, start, end) {
                stages::stage(self, start, end);
            }
            let stages =
                stages::Minutes::count((start..end).map(|m| self.samples[Self::slot(m)].sleep()));
            (start, end, stages)
        });
        let (start, end, stages) = self.aged.last_sleep(newest, recent)?;
        Some(Sleep {
            start_utc_minute: start,
            end_utc_minute: end,
            asleep_minutes: stages.asleep(),
            deep_minutes: stages.deep,
            rem_minutes: stages.rem,
        })
    }
}

/// Staging works on the history in place.
impl stages::Record for History {
    fn minute(&self, minute: i64) -> (sleep::Kind, Option<u16>, u8) {
        let newest = self.newest.unwrap_or(i64::MIN);
        if minute > newest || newest - minute >= HISTORY_MINUTES as i64 {
            return (sleep::Kind::Unknown, None, 0);
        }
        let sample = &self.samples[Self::slot(minute)];
        (
            sample.sleep(),
            sample.seen().then_some(sample.movement),
            sample.heart_rate,
        )
    }

    fn newest(&self) -> i64 {
        self.newest.unwrap_or(i64::MIN)
    }

    fn set_stage(&mut self, minute: i64, kind: sleep::Kind) {
        self.samples[Self::slot(minute)].set_sleep(kind);
    }
}

/// A night's (or nap's) sleep.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct Sleep {
    pub start_utc_minute: i64,
    /// The minute after the last asleep one.
    pub end_utc_minute: i64,
    /// Minutes actually asleep (light, deep, or REM; not restless) in
    /// between.
    pub asleep_minutes: u16,
    /// Of those, deep sleep (see [`stages`]).
    pub deep_minutes: u16,
    /// And REM sleep; the rest is light.
    pub rem_minutes: u16,
}

static LAST_SLEEP: Mutex<CriticalSectionRawMutex, Cell<Option<Sleep>>> =
    Mutex::new(Cell::new(None));

/// Whether the wearer is asleep, as of the last minute's classification.
/// Falling asleep is recognised once [`sleep::ONSET`] minutes of a
/// sleeper's stillness have passed (see [`sleep`]), waking after [`sleep::WAKE_RUN`] minutes of movement (or
/// at once on getting up and walking), so this lags both by that much.
/// (Or a dev kit was told the wearer is: see `debug::asleep_forced`.)
pub fn asleep() -> bool {
    ASLEEP.load(Ordering::Relaxed) || crate::debug::asleep_forced()
}

static ASLEEP: AtomicBool = AtomicBool::new(false);

/// Whether the minutes before this one were light sleep, or awake (see
/// [`stages::light`]), as of the last classification: what a smart alarm
/// waits for to ring early.
pub fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

static LIGHT: AtomicBool = AtomicBool::new(false);

/// The most recent sleep, until a day after its end, for the UI; updated
/// every minute.
pub fn last_sleep() -> Option<Sleep> {
    LAST_SLEEP.lock(Cell::get)
}

/// What a Gadgetbridge fetch holds back now, if anything: the minutes
/// whose kind may still change (see [`sleep_hold`]). As of the last
/// classification: the newest minute and its classification change
/// together, with no `await` in between.
pub fn gadgetbridge_hold() -> Option<sleep_hold::Hold> {
    HISTORY.lock(|history| {
        let history = history.borrow();
        let newest = history.newest?;
        let sample = |minute: i64| history.sample(newest, minute);
        sleep_hold::hold(
            newest,
            history.settled,
            |minute| sample(minute).sleep(),
            |minute| sample(minute).evidence(),
        )
    })
}

struct Today {
    /// Local date, as days since 1970 ([`UNKNOWN_DAY`] before the clock is
    /// known).
    day: i64,
    steps: u32,
}

impl Today {
    /// Today's total as of `now` (UTC minute and offset): unchanged on
    /// the same date, and on a new one, by midnight or a new time zone,
    /// the history's since local midnight (see
    /// [`crate::day::steps_today`]).
    fn on(&self, (utc_minute, offset): (i64, i16)) -> Self {
        let day = crate::day::date(utc_minute, offset);
        let counted_on = (self.day != UNKNOWN_DAY).then_some(self.day);
        let steps = crate::day::steps_today(counted_on, self.steps, day, || {
            steps_since_midnight(utc_minute, offset)
        });
        Self { day, steps }
    }

    /// Brings the total up to `now` (see [`Today::on`]), keeping the new
    /// date's. The activity task does at every poll, so the date it's
    /// kept under is never more than a poll old: otherwise, on a date that
    /// came back (flying west across midnight), the old total would pass
    /// for the new day's.
    fn settle(&mut self, now: (i64, i16)) {
        let on = self.on(now);
        if on.day != self.day {
            if self.day != UNKNOWN_DAY {
                info!("activity: new day, {} steps so far", on.steps);
            }
            *self = on;
            save_today(self);
        }
    }
}

pub static HISTORY: Mutex<CriticalSectionRawMutex, RefCell<History>> =
    Mutex::new(RefCell::new(History::new()));
static TODAY: Mutex<CriticalSectionRawMutex, RefCell<Option<Today>>> =
    Mutex::new(RefCell::new(None));

/// Records a heart-rate reading in the current minute's sample (if the
/// time is known). A later reading in the same minute replaces it.
///
/// TODO: average the readings within a minute, for the continuous
/// measurement while the clock face is showing.
///
/// Not while the clock has just jumped far ahead (see [`jumps`]): the
/// activity task files the first minute after that, within a poll, once
/// the log has what the jump pushes out of the history. A reading in
/// those two seconds is dropped.
pub fn record_heart_rate(bpm: u8) {
    let Some(utc_minute) = clock::now_utc_seconds().map(|s| s.div_euclid(60)) else {
        return;
    };
    HISTORY.lock(|history| {
        let mut history = history.borrow_mut();
        if history.jumps(utc_minute) {
            debug!("activity: a heart rate right after the clock jumped; dropped");
        } else {
            history.at(utc_minute).heart_rate = bpm;
        }
    });
}

/// Whether filing a sample for `utc_minute` would be a jump: so far on
/// from the history's newest minute that the history would let go of
/// minutes the activity log may not have yet (the last bucket, and what a
/// fetch was holding back: a night under way when the battery ran out,
/// say, with the charger found the evening after). Then
/// [`crate::system_off::serve`] brings the log up to date first, and
/// files the minute itself ([`skip_to`]).
pub fn jumps(utc_minute: i64) -> bool {
    HISTORY.lock(|history| history.borrow().jumps(utc_minute))
}

/// Moves the history on to `utc_minute`, with nothing recorded in it yet:
/// after a jump (see [`jumps`]), once the log has what that lets go of.
pub fn skip_to(utc_minute: i64) {
    HISTORY.lock(|history| {
        history.borrow_mut().at(utc_minute);
    });
}

/// How much the wrist moved over the last few seconds (one poll of the
/// FIFO), in the same units as [`Sample::movement`].
pub fn movement() -> u16 {
    MOVEMENT.load(Ordering::Relaxed)
}

static MOVEMENT: AtomicU16 = AtomicU16::new(0);

/// How many polls ([`POLL_SECONDS`] each) since [`take_active_polls`] last
/// ran saw steps, or movement too vigorous to measure the heart rate
/// through ([`crate::heart::MOTION_LIMIT`]): for the heart rate's schedule
/// (see [`crate::heart_schedule::MOVED_POLLS`]), as a sign that activity may
/// be starting. Saturates; it only has to tell a few from none.
pub fn active_polls() -> u8 {
    ACTIVE_POLLS.load(Ordering::Relaxed)
}

/// Like [`active_polls`], and starts counting afresh.
pub fn take_active_polls() -> u8 {
    ACTIVE_POLLS.swap(0, Ordering::Relaxed)
}

static ACTIVE_POLLS: AtomicU8 = AtomicU8::new(0);

/// How many polls in a row, up to the latest, were calm enough to measure
/// the heart rate through ([`crate::heart::CALM`]); saturating. Whether
/// the wrist has *stayed* calm, for starting a session (see
/// [`crate::heart_schedule::START_CALM_POLLS`]).
pub fn calm_polls() -> u8 {
    CALM_POLLS.load(Ordering::Relaxed)
}

static CALM_POLLS: AtomicU8 = AtomicU8::new(0);

/// How many polls in a row, up to the latest, counted no steps;
/// saturating. Whether a heart rate reading is a resting one (see
/// [`crate::heart_schedule::RESTING_STEPLESS_POLLS`]).
pub fn stepless_polls() -> u8 {
    STEPLESS_POLLS.load(Ordering::Relaxed)
}

static STEPLESS_POLLS: AtomicU8 = AtomicU8::new(0);

/// The workout under way, if the last minutes look like one (see
/// [`workout::detect`], which this task asks as each minute ends):
/// [`crate::heart`] measures continuously through it.
pub fn workout() -> Option<workout::Kind> {
    workout::Kind::from_bits(WORKOUT.load(Ordering::Relaxed))
}

/// [`workout()`], as [`workout::Kind::bits`]. One byte, and all the
/// state detection has: the minutes it judges by are the history's own.
static WORKOUT: AtomicU8 = AtomicU8::new(0);

/// Set once the accelerometer is configured; nothing else may talk to it
/// before (setup soft-resets it and uploads its program).
static READY: AtomicBool = AtomicBool::new(false);

/// Whether the wrist is still: the accelerometer saw no motion for a few
/// seconds ([`NO_MOTION`]), and hasn't seen any since ([`ANY_MOTION`]).
/// [`crate::heart`] is woken whenever this changes.
///
/// Coarser than [`movement`], and on different terms (every sample within
/// a threshold, rather than an average), so it isn't what the heart rate's
/// calm threshold or sleep tracking use.
pub fn still() -> bool {
    STILL.load(Ordering::Relaxed)
}

static STILL: AtomicBool = AtomicBool::new(false);

/// Whether the watch lies on a desk, off the wrist (see [`desk`]), as of
/// the last poll.
pub fn on_desk() -> bool {
    DESK.load(Ordering::Relaxed)
}

static DESK: AtomicBool = AtomicBool::new(false);

/// Whether the wrist stirred lately (see [`desk::stirring`]): more than a
/// twitch in the last minute, for the touch controller.
pub fn stirring() -> bool {
    desk::stirring(
        embassy_time::Instant::now().as_secs() as u32,
        STIRRED.load(Ordering::Relaxed),
    )
}

/// When the wrist last stirred (see [`desk::STIR`]), in seconds of
/// uptime. Boot counts as one.
static STIRRED: AtomicU32 = AtomicU32::new(0);

// ==============================================================================
// Motion and glances
// ==============================================================================

/// How often to look at the wrist's pose while the screen is off and the
/// wrist moves: fast enough that the time is up within ~0.5 s of turning
/// the wrist.
const GLANCE_POLL: Duration = Duration::from_millis(200);

/// How often to look for the end of movement while no glance is wanted
/// (the screen is on, or the wearer asleep): one byte of interrupt status.
const STATUS_POLL: Duration = Duration::from_secs(1);

/// Polls that must pass after the last any-motion before polling stops,
/// however still the accelerometer says the wrist is: at least
/// [`crate::glance::RECENT`] + [`crate::glance::SETTLE`] samples (2 s), so
/// a glance in progress can finish, and so that
/// [`crate::glance::Detector::pause`]'s premise holds.
const MIN_MOVING_POLLS: u8 = crate::glance::RECENT + crate::glance::SETTLE;

/// While still, how often to check that a motion interrupt wasn't missed
/// (a latched any-motion with the pin low). Cheap insurance: if INT1 ever
/// stopped working, glances would stop with it.
const STILL_CHECK: Duration = Duration::from_secs(60);

/// Follows the wrist moving and keeping still, from the accelerometer's
/// motion features (see [`still`]), and while it moves with the screen
/// off, watches for it turning to look at the watch (see
/// [`crate::glance`]) and asks the UI for a glance.
///
/// While the wrist moves, this polls: every [`GLANCE_POLL`] while a glance
/// may be wanted, reading the acceleration and the interrupt status in one
/// transfer (no FIFO, so no power save dance; a misread in power save
/// shows up as an implausible magnitude, which the detector ignores), or
/// every [`STATUS_POLL`] otherwise. Once the chip reports no motion, it
/// stops, and sleeps until INT1 (`interrupt`, P0.08) rises with
/// any-motion: no timer, no I2C, until something moves.
#[embassy_executor::task]
pub async fn motion_task(i2c: Device, mut interrupt: Input<'static>) {
    let mut bma = Bma421 { i2c, quiet: false };
    let mut detector = crate::glance::Detector::default();
    let mut screen_was_on = true;
    while !READY.load(Ordering::Relaxed) {
        Timer::after(GLANCE_POLL).await;
    }
    // Small types throughout: this task's state is a static, and RAM is
    // tight. Polls since the last any-motion (saturating); until the chip
    // says otherwise, the wrist may be moving.
    let mut polls_since_motion = 0u8;
    // Whether the last poll was for a glance (which sets the pace), and
    // whether to skip the wait before the next.
    let mut glancing = true;
    let mut poll_now = false;
    // Cleared if INT1 is ever caught missing a motion: from then on this
    // polls as if the wrist never kept still, as it did before INT1 was
    // used.
    let mut interrupt_works = true;
    loop {
        if STILL.load(Ordering::Relaxed) {
            crate::power::on(crate::power::Meter::Still);
            loop {
                let woken = select(interrupt.wait_for_high(), Timer::after(STILL_CHECK)).await;
                // Before the read below lowers it.
                let raised = matches!(woken, Either::First(())) || interrupt.is_high();
                // A failed read counts as motion: polling is the safe side.
                let status = bma.interrupts().await.unwrap_or(INT_ANY_MOTION);
                if status & INT_ANY_MOTION != 0 {
                    if !raised {
                        warn!("activity: INT1 missed a motion; polling instead");
                        interrupt_works = false;
                    }
                    break;
                }
            }
            debug!("activity: moving");
            STILL.store(false, Ordering::Relaxed);
            crate::heart::wake();
            crate::power::off(crate::power::Meter::Still);
            polls_since_motion = 0;
            // Look at once: this may be the start of a glance.
            poll_now = true;
        }

        if !poll_now {
            Timer::after(if glancing { GLANCE_POLL } else { STATUS_POLL }).await;
        }
        poll_now = false;
        let screen_on = crate::glance::screen_on();
        if screen_on != screen_was_on {
            detector.reset();
            screen_was_on = screen_on;
        }
        // Asleep, turning over in bed shouldn't light the screen (and the
        // accelerometer needn't be read five times a second all night). The
        // button and a tap still wake it. On waking, glances return once the
        // classifier has seen it: after a few minutes of movement, or at
        // once on getting up and walking.
        glancing = !screen_on && !asleep();
        if !glancing {
            detector.reset();
        }

        // The acceleration (0x12..=0x17) and, five bytes on, the interrupt
        // status (0x1C), in one transfer; or only the status.
        let mut data = [0u8; 11];
        let (register, bytes) = if glancing {
            (REG_ACC_DATA, &mut data[..])
        } else {
            (REG_INT_STATUS_0, &mut data[10..])
        };
        if bma.read(register, bytes).await.is_err() {
            continue;
        }
        let status = data[10];
        polls_since_motion = if status & INT_ANY_MOTION != 0 {
            0
        } else {
            polls_since_motion.saturating_add(1)
        };
        if glancing {
            let axis = |i: usize| i16::from_le_bytes([data[i], data[i + 1]]) >> 4;
            let sample = crate::glance::from_sensor([axis(0), axis(2), axis(4)]);
            if detector.push(sample) {
                let _ = crate::INPUT.try_send(crate::InputEvent::Glance);
            }
        }

        // Still: stop polling until INT1 says otherwise. The read above
        // cleared the latch, so any motion from here on raises the pin.
        if interrupt_works && status & INT_NO_MOTION != 0 && polls_since_motion >= MIN_MOVING_POLLS
        {
            debug!("activity: still");
            detector.pause();
            STILL.store(true, Ordering::Relaxed);
            crate::heart::wake();
        }
    }
}

/// Today's steps, for the clock face. Brought up to date first, as a
/// poll would, so a new date (midnight, or a new time zone) shows at once.
pub fn steps_today() -> Option<u32> {
    let now = now_minute()?;
    TODAY.lock(|t| {
        let mut t = t.borrow_mut();
        let t = t.as_mut()?;
        t.settle(now);
        Some(t.steps)
    })
}

/// A running count of the steps counted since boot, wrapping, for
/// [`crate::live`]'s reports of the steps since the last: unlike today's
/// total, it never starts over or jumps (see [`Today::on`]).
pub fn steps_counted() -> u32 {
    STEPS_COUNTED.load(Ordering::Relaxed)
}

static STEPS_COUNTED: AtomicU32 = AtomicU32::new(0);

/// Now, as UTC minutes since 1970 and the UTC offset, once the phone has
/// set the clock.
fn now_minute() -> Option<(i64, i16)> {
    Some((
        clock::now_utc_seconds()?.div_euclid(60),
        clock::utc_offset_minutes()?,
    ))
}

/// The history's steps from local midnight to `utc_minute` at `offset`:
/// what the Today screen adds up (see [`History::day`]), hour by hour.
fn steps_since_midnight(utc_minute: i64, offset: i16) -> u32 {
    let day = HISTORY.lock(|history| history.borrow().day(utc_minute, offset));
    day.hours.iter().map(|hour| u32::from(hour.steps)).sum()
}

/// Today's activity hour by hour since local midnight (see [`crate::day`]),
/// for the Today screen, once the time is known. Computed afresh from the
/// history when asked (see [`History::day`]), rather than kept up to date
/// in RAM, as the screen is seldom on.
///
/// TODO: a day with a change to or from summer time has an hour too many
/// or too few; this files each minute under the offset in force now.
pub fn today() -> Option<crate::day::Day> {
    let (utc_minute, offset) = now_minute()?;
    // Up to now, as today's total counts (see `steps_since_midnight`).
    Some(HISTORY.lock(|history| history.borrow().day(utc_minute, offset)))
}

// ==============================================================================
// The task
// ==============================================================================

/// How often to drain the FIFO and read the step counter. Frames are 7
/// bytes with their headers, so at 12.5 Hz, 2 s fills 175 bytes, well
/// inside one 252-byte read (see `drain_fifo`), with room to catch up after
/// a late poll. (At 3 s it was 263 bytes: every read was a full 36 frames,
/// the FIFO fell further behind until it overflowed, and the skip frame
/// that marks an overflow misaligned the read, dropping the batch, about
/// every half hour.)
const POLL_SECONDS: u64 = 2;

/// The least movement (as in [`Sample::movement`]) over a poll for
/// [`trace_samples`] to log its samples: under the quietest walks the
/// chip missed (their middle polls moved 300–500), over a wrist lying
/// still overnight (under 100), so a night costs little.
const TRACE_MOVEMENT: u64 = 200;

/// Logs a poll's samples at trace level, for tuning a step counter of our
/// own offline against another watch (see CONTRIBUTING's Field logs): the
/// chip's own misses short and stop-and-go walks (see the module docs),
/// and can't be replayed, but ours could be. Only a build that logs this
/// module at trace (`UR_ACTIVITY_LOG=trace scripts/release.sh`) has this
/// line; the release builds compile it out.
///
/// Each poll's samples, x, y, z in turn, at 12.5 Hz, oldest first, the
/// newest a moment before the log line's time, in 1/64 g (so each fits a
/// byte, halving the log: ±2 g, as the chip measures, is ±128; ample for
/// steps). That's 80 bytes a poll, 40 a second while it moves (twice the
/// rest of the log on a busy day), so a capture day's log should be
/// fetched twice.
fn trace_samples(frames: &Frames, movement: u64) {
    if movement < TRACE_MOVEMENT {
        return;
    }
    let mut raw = [0i8; 3 * 36];
    for (out, &axis) in raw.iter_mut().zip(frames.as_flattened()) {
        *out = (axis >> 4).clamp(-128, 127) as i8;
    }
    defmt::trace!("activity: samples {=[?]}", raw[..3 * frames.len()]);
}

/// Logs a poll in a field build: a minute's polls to a line (see
/// [`crate::log_pack::Polls`]), which `scripts/expand_log.py` turns back
/// into a line a poll, as other builds log them. That line a poll was
/// 60% of a worn day's field log, nearly all of it the timestamp and
/// argument tags each line carries. A partly filled minute is lost if the
/// watch resets or switches off.
///
/// A capture build (this module at trace) also logs each poll as before,
/// at trace: `scripts/accel_samples.py` pairs each poll's samples (see
/// [`trace_samples`]) with its steps, by time. (The expander then leaves
/// the minute's line alone, rather than repeat its polls.)
#[cfg(feature = "field-log")]
fn log_poll(
    polls: &mut crate::log_pack::Polls,
    samples: u32,
    movement: u64,
    steps: u32,
    count: u32,
    [x, y, z]: [i16; 3],
) {
    defmt::trace!(
        "activity: {} samples, movement {}, {} new steps (counter {}), last ({}, {}, {})",
        samples,
        movement,
        steps,
        count,
        x,
        y,
        z
    );
    let now = embassy_time::Instant::now().as_millis();
    if polls.push(now, samples, movement, steps) {
        // Samples as counted from 24, so that each is nearly always a 0,
        // which costs a bit. The expander knows the line by its start.
        defmt::debug!(
            "activity: polls over {=u32} ms: movement {=[?]}, steps {=[?]}, samples-24 {=[u8]}, counter {=u32}, last ({=i16}, {=i16}, {=i16})",
            polls.span_ms(now),
            polls.movement(),
            polls.steps(),
            polls.samples(),
            count,
            x,
            y,
            z
        );
        polls.clear();
    }
}

#[embassy_executor::task]
pub async fn activity_task(i2c: Device) {
    restore_today();
    // What the flash holds of the history (the log of its minutes, and
    // what was saved before the watch last reset, or switched itself
    // off): here, before any minute is recorded, and because saving and
    // restoring the history in this task's future costs little static
    // RAM (see `system_off`).
    let mut log = crate::system_off::restore().await;
    // Let the touch controller be configured first: it only listens for a
    // moment after its reset, and our config upload holds the shared bus
    // for about a third of a second.
    Timer::after_millis(200).await;
    let mut bma = Bma421 { i2c, quiet: false };
    // Setup used to fail partway maybe two times in ten, nearly always with
    // the first write after the chip's soft reset hanging (see `init`, which
    // now resets the controller there); a fresh attempt from soft reset
    // almost always worked. But a bus that stays stuck can take longer (or
    // need recovering again; see `i2c`), so it's retried for as long as it
    // takes, ever more slowly: giving up would leave a sealed watch without
    // steps until its next reboot.
    let mut backoff = crate::backoff::Backoff::new();
    loop {
        bma.quiet = !backoff.loud();
        if bma.init().await.is_ok() {
            break;
        }
        let retry = backoff.failed();
        if retry.log {
            warn!(
                "activity: accelerometer setup failed ({=u16}x); retrying in {=u32} ms",
                retry.failures, retry.wait_ms
            );
        }
        // In pieces, still doing what `system_off` needs done meanwhile.
        let mut wait_ms = retry.wait_ms;
        while wait_ms > 0 {
            let piece = wait_ms.min(POLL_SECONDS as u32 * 1000);
            Timer::after_millis(u64::from(piece)).await;
            crate::system_off::serve(&mut log).await;
            wait_ms -= piece;
        }
    }
    bma.quiet = false;
    info!("activity: accelerometer running");
    READY.store(true, Ordering::Relaxed);

    // Which reads of the step counter to believe (see `step_counter`),
    // from 0 now, as setup leaves it. Its clock: seconds since boot.
    let uptime = || embassy_time::Instant::now().as_secs() as u32;
    let mut counter = StepCounter::new(uptime());
    // The previous sample, to measure movement against.
    let mut previous: Option<[i16; 3]> = None;
    // Whether the watch lies on a desk (see `desk`).
    let mut lying = desk::Desk::default();
    // Movement accumulated within the current minute.
    let (mut minute, mut movement_sum, mut movement_samples) = (None::<i64>, 0u64, 0u32);
    // The polls not yet in the field log (see `log_poll`).
    #[cfg(feature = "field-log")]
    let mut polls = crate::log_pack::Polls::new();

    loop {
        Timer::after_secs(POLL_SECONDS).await;
        crate::system_off::serve(&mut log).await;

        // In a block, so the samples are gone by the next `await` rather
        // than kept in this task's (static) future.
        let (moved, samples) = {
            let Ok(frames) = bma.drain_fifo().await else {
                warn!("activity: reading the accelerometer failed");
                continue;
            };
            let mut moved = 0u64;
            let mut samples = 0u32;
            for &now in &frames {
                if let Some(before) = previous {
                    let d = |i: usize| f32::from(now[i]) - f32::from(before[i]);
                    moved += libm::sqrtf(d(0) * d(0) + d(1) * d(1) + d(2) * d(2)) as u64;
                    samples += 1;
                }
                previous = Some(now);
            }
            trace_samples(&frames, moved * 8 / u64::from(samples.max(1)));
            // In the logging build, through each heart rate session.
            #[cfg(feature = "field-log")]
            crate::heart::trace_accel(&frames);
            (moved, samples)
        };
        let Ok(count) = bma.steps().await else {
            warn!("activity: reading the accelerometer failed");
            continue;
        };
        // The counter only counts up, but a read can be garbage: lower
        // (0, after a broken FIFO batch) or far higher (the chip's
        // filler). Either, taken as it is, is billions of steps. So only a
        // count a wrist could have reached is counted (see
        // `step_counter`), and the count can't get stuck on a bad one.
        let last = counter.last();
        let new_steps = match counter.read(count, uptime()) {
            Read::Counted(steps) => steps,
            Read::Ignored => {
                warn!(
                    "activity: step counter read {} after {}; ignored",
                    count, last
                );
                0
            }
            Read::Moved(steps) => {
                warn!("activity: step counter carries on from {}", count);
                steps
            }
        };
        // The last sample too: which way up the watch is (1024 per g, in
        // the sensor's own axes), for orientation questions such as how
        // it's mounted, or sleep position.
        let [x, y, z] = previous.unwrap_or_default();
        #[cfg(feature = "field-log")]
        log_poll(
            &mut polls,
            samples,
            moved * 8 / u64::from(samples.max(1)),
            new_steps,
            count,
            [x, y, z],
        );
        #[cfg(not(feature = "field-log"))]
        defmt::debug!(
            "activity: {} samples, movement {}, {} new steps (counter {}), last ({}, {}, {})",
            samples,
            moved * 8 / u64::from(samples.max(1)),
            new_steps,
            count,
            x,
            y,
            z
        );

        add_steps_today(new_steps);
        // Counts (1024 per g) to Bangle.js units (8192 per g).
        let recent = (moved * 8 / u64::from(samples.max(1))).min(u64::from(u16::MAX)) as u16;
        // A dev kit lies still on a desk: it can be told to take the wrist
        // as moving (for the heart rate's busy sessions), as far as
        // everything that goes by this measure is concerned.
        #[cfg(feature = "debug-input")]
        let recent = crate::debug::movement().unwrap_or(recent);
        MOVEMENT.store(recent, Ordering::Relaxed);
        if new_steps > 0 || recent > crate::heart::MOTION_LIMIT {
            ACTIVE_POLLS.update(Ordering::Relaxed, Ordering::Relaxed, |polls| {
                polls.saturating_add(1)
            });
        }
        let run = |counts: bool| move |polls: u8| if counts { polls.saturating_add(1) } else { 0 };
        CALM_POLLS.update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            run(recent <= crate::heart::CALM),
        );
        // The wrist has just been calm for long enough to start a heart
        // rate session: tell `heart`, which may be waiting for exactly
        // that with a reading late (see `heart_schedule`'s "Seeking
        // calm"). Once per calm stretch, and a look that finds nothing to
        // do is a few comparisons.
        if CALM_POLLS.load(Ordering::Relaxed) == crate::heart_schedule::START_CALM_POLLS {
            crate::heart::wake();
        }
        STEPLESS_POLLS.update(Ordering::Relaxed, Ordering::Relaxed, run(new_steps == 0));

        // A stir, which wakes the touch controller; and whether the watch
        // lies on a desk, from how this poll's last sample lies (see
        // `desk`). Since when, should it have just been found to.
        let now = uptime();
        if recent > desk::STIR {
            STIRRED.store(now, Ordering::Relaxed);
        }
        let pulse = crate::heart_reading::latest().map(|(_, at)| at.as_secs() as u32);
        let landed = match previous.and_then(|sample| lying.poll(now, recent, sample, pulse)) {
            Some(desk::Change::Down { since }) => {
                info!("desk: lying still for {} s", now.saturating_sub(since));
                Some(since)
            }
            Some(desk::Change::Up) => {
                info!("desk: moved");
                None
            }
            None => None,
        };
        DESK.store(lying.on(), Ordering::Relaxed);

        // Per-minute history needs wall-clock time.
        let Some(utc_second) = clock::now_utc_seconds() else {
            continue;
        };
        let utc_minute = utc_second.div_euclid(60);
        // The phone set the clock far ahead since this poll began (after
        // `serve`, above, looked): the next poll's `serve` brings the log
        // up to date before anything is filed there (see `jumps`). This
        // poll's steps are in today's total, but in no minute.
        if jumps(utc_minute) {
            continue;
        }
        let new_minute = minute != Some(utc_minute);
        if new_minute {
            (minute, movement_sum, movement_samples) = (Some(utc_minute), 0, 0);
        }
        movement_sum += moved;
        movement_samples += samples;
        // Counts (1024 per g) to Bangle.js units (8192 per g).
        let movement =
            (movement_sum * 8 / u64::from(movement_samples.max(1))).min(u64::from(u16::MAX)) as u16;
        HISTORY.lock(|history| {
            let mut history = history.borrow_mut();
            let sample = history.at(utc_minute);
            sample.steps = sample
                .steps
                .saturating_add(new_steps.min(u32::from(u16::MAX)) as u16);
            sample.movement = movement;
            // What tells lying awake from sleep (see `sleep`): the
            // minute's mean hides a poll like this one.
            if recent >= sleep::FIDGET {
                sample.set_fidgeted();
            }
            // Off the wrist on a desk, whatever the heart rate sensor
            // last made of it (often nothing: a desk reflects its LED).
            sample.set_seen(if lying.on() {
                Some(false)
            } else {
                crate::heart::worn()
            });
            // A workout's minute, for Gadgetbridge. After the above, which
            // starts each poll's verdict afresh: so the minute a workout
            // ends in (found at its first poll, below) isn't one.
            if workout().is_some() {
                history.at(utc_minute).set_exercise();
            }
            // And so it was since the desk's spell began: never asleep.
            if let Some(since) = landed {
                let seconds = i64::from(now.saturating_sub(since));
                history.off_wrist_since((utc_second - seconds).div_euclid(60));
            }
        });
        // Once a minute: revise the recent past's sleep, its stages, and the
        // summary.
        //
        // All of it at once, with nothing else running: a fetch, or the
        // smart alarm, must not see the window re-classified but not yet
        // re-staged. So it has to be quick, as every other task waits for
        // it: at up to 93 ms late in a night (0.11.3-rc.3, staging then
        // reading the history some 31 times per minute of sleep, now 6),
        // heart rate sessions running across a minute's start lost I2C
        // transfers to their 50 ms timeout. The time goes in the log, to
        // keep an eye on.
        //
        // TODO: stage only what can still change (the classifier's window
        // and an hour before it), from a checkpoint of the staging's state
        // there, so that this stops growing with the night; that takes
        // ~30 bytes of static RAM, which the `compare` build doesn't have.
        if new_minute {
            let started = embassy_time::Instant::now();
            let (summary, changed, asleep, light, settled) = HISTORY.lock(|history| {
                let mut history = history.borrow_mut();
                history.classify_sleep();
                let summary = history.last_sleep();
                // After the staging, so that the night is settled as
                // staged at its last.
                let settled = history.settle();
                (
                    summary,
                    summary != LAST_SLEEP.lock(Cell::get),
                    history.asleep_now(),
                    // After `last_sleep`, which stages what's new.
                    stages::light(&*history),
                    settled,
                )
            });
            debug!(
                "activity: revised sleep in {=u32} us",
                started.elapsed().as_micros() as u32
            );
            if asleep != ASLEEP.swap(asleep, Ordering::Relaxed) {
                info!("sleep: {}", if asleep { "asleep now" } else { "awake now" });
            }
            // So a field log says why a smart alarm did or didn't ring at
            // a stir (a few dozen lines a night).
            if light != LIGHT.swap(light, Ordering::Relaxed) {
                info!("sleep: {}", if light { "light" } else { "not light" });
            }
            if let Some((before, last)) = settled {
                info!(
                    "sleep: up, so settled before minute {} (last asleep {})",
                    before, last
                );
            }
            if changed && let Some(sleep) = summary {
                info!(
                    "sleep: {} min asleep ({} deep, {} REM), minutes {}..{} (UTC)",
                    sleep.asleep_minutes,
                    sleep.deep_minutes,
                    sleep.rem_minutes,
                    sleep.start_utc_minute,
                    sleep.end_utc_minute
                );
            }
            LAST_SLEEP.lock(|s| s.set(summary));

            // And whether the minutes just finished make a workout (see
            // `workout`), which the heart rate is measured through. After
            // the sleep revision, as asleep is never one. The minute under
            // way isn't looked at: it has seen one poll.
            let recent = HISTORY.lock(|history| {
                let history = history.borrow();
                let mut recent = [workout::Minute::default(); workout::WINDOW];
                for (ago, minute) in recent.iter_mut().rev().enumerate() {
                    let sample = history.sample(utc_minute, utc_minute - 1 - ago as i64);
                    *minute = workout::Minute {
                        steps: sample.steps,
                        movement: sample.movement,
                    };
                }
                recent
            });
            let was = workout();
            let now = workout::detect(was, &recent, asleep);
            if now != was {
                WORKOUT.store(workout::Kind::bits(now), Ordering::Relaxed);
                // What the rules went by, so a field log shows why: the
                // last three minutes' steps (a walk's rule), and how many
                // of the ten moved hard (an effort's).
                let [.., a, b, c] = recent;
                let working = recent
                    .iter()
                    .filter(|m| m.movement >= workout::EFFORT_MOVEMENT)
                    .count() as u8;
                // The minutes that made it one were part of it: three of
                // walking, or the ten an effort is judged over. (A fetch
                // may have sent some already, as plain activity; they stay
                // so on the phone.)
                //
                // TODO: a manual start and stop would set `WORKOUT` here
                // too (a kind of its own, that `detect` leaves alone until
                // stopped): the rules can't know a strength session from
                // housework. Not wanted for now (user, 2026-11-21).
                if was.is_none()
                    && let Some(kind) = now
                {
                    let minutes = match kind {
                        workout::Kind::Walk => workout::WALK_MINUTES,
                        workout::Kind::Effort => workout::WINDOW,
                    };
                    HISTORY.lock(|history| {
                        history
                            .borrow_mut()
                            .exercised_since(utc_minute - minutes as i64)
                    });
                    // Those the phone hasn't fetched are mostly in the
                    // activity log already, as they were: it takes them
                    // in again, for the fetch that reads them there.
                    crate::system_off::relog_from(utc_minute - minutes as i64);
                }
                match (was, now) {
                    (None, Some(kind)) => info!(
                        "workout: started, {} (steps {=u16}, {=u16}, {=u16}; {=u8}/10 min moving)",
                        kind, a.steps, b.steps, c.steps, working
                    ),
                    // A walk within an effort, or the effort a walk ends
                    // in: the same workout.
                    (Some(_), Some(kind)) => info!(
                        "workout: now {} (steps {=u16}, {=u16}, {=u16}; {=u8}/10 min moving)",
                        kind, a.steps, b.steps, c.steps, working
                    ),
                    (_, None) => info!(
                        "workout: over (steps {=u16}, {=u16}, {=u16}; {=u8}/10 min moving{=str})",
                        a.steps,
                        b.steps,
                        c.steps,
                        working,
                        if asleep { "; asleep" } else { "" }
                    ),
                }
                // Starts the heart rate's continuous session, or lets the
                // one under way end, without waiting out its timer.
                crate::heart::wake();
            }
            // Every couple of minutes, the SPI flash is brought up to
            // date, for a crash to lose no more than those minutes: the
            // buckets a fetch could now send are appended to the activity
            // log (see `spill`), and the rest, what a fetch holds back,
            // is checkpointed (see `snapshot`). At the next poll, from
            // `serve`.
            if crate::snapshot::checkpoint_due(utc_minute) {
                crate::system_off::checkpoint();
            }
        }
    }
}

/// `Today::day` before the clock is known.
const UNKNOWN_DAY: i64 = i64::MIN;

/// Counts `steps` as walked now, in today's total and this minute's
/// history: for [`crate::debug`], to test what keeps them across resets and
/// updates on a watch that can't walk.
#[cfg(feature = "debug-input")]
pub fn walk(steps: u32) {
    add_steps_today(steps);
    if let Some(utc_minute) = clock::now_utc_seconds().map(|s| s.div_euclid(60)) {
        HISTORY.lock(|history| {
            let mut history = history.borrow_mut();
            let sample = history.at(utc_minute);
            sample.steps = sample
                .steps
                .saturating_add(steps.min(u32::from(u16::MAX)) as u16);
            sample.set_seen(Some(true));
        });
    }
}

/// Adds steps to today's total, which starts a new day when the date
/// changes (see [`Today::on`]). Call before filing the steps in the
/// history, which would otherwise count them twice on a new date.
fn add_steps_today(steps: u32) {
    STEPS_COUNTED.fetch_add(steps, Ordering::Relaxed);
    let now = now_minute();
    TODAY.lock(|today| {
        let mut today = today.borrow_mut();
        // Steps before the clock is known count towards whatever day it
        // turns out to be.
        let t = today.get_or_insert(Today {
            day: UNKNOWN_DAY,
            steps: 0,
        });
        if let Some(now) = now {
            t.settle(now);
        }
        t.steps = t.steps.saturating_add(steps);
        save_today(t);
    });
}

// ==============================================================================
// Surviving System OFF, updates and crashes
// ==============================================================================
//
// What `system_off` saves to the SPI flash before switching the watch off
// or installing an update, and every couple of minutes (checkpoints, for a
// crash), and puts back at the next boot: the history the phone hasn't
// fetched (the sleep tracker's night with it, as each minute's kind is in
// its sample), and today's steps.

/// The minutes worth saving: (first, how many), from the first minute with
/// data at or after `from` to the newest. `None` if there's nothing.
pub fn span_from(from: i64) -> Option<(i64, usize)> {
    HISTORY.lock(|history| {
        let history = history.borrow();
        let newest = history.newest?;
        let (first, _) = history.since(from).next()?;
        Some((first, (newest - first + 1) as usize))
    })
}

/// The newest minute with a sample, if any.
pub fn newest_minute() -> Option<i64> {
    HISTORY.lock(|history| history.borrow().newest)
}

/// Copies the samples from `first` on into `out`, as bytes (see
/// [`Sample::to_bytes`]), as many whole ones as fit. A minute that has
/// since dropped out of the history (the save takes a moment, and a new
/// minute may have begun) is copied as empty, rather than as the newer
/// minute now in its slot.
pub fn copy_samples(first: i64, out: &mut [u8]) {
    HISTORY.lock(|history| {
        let history = history.borrow();
        let newest = history.newest.unwrap_or(i64::MIN);
        for (i, bytes) in out.as_chunks_mut::<6>().0.iter_mut().enumerate() {
            let minute = first + i as i64;
            let sample = if minute <= newest && newest - minute < HISTORY_MINUTES as i64 {
                history.samples[History::slot(minute)]
            } else {
                Sample::default()
            };
            *bytes = sample.to_bytes();
        }
    });
}

/// A minute's sample as bytes (see [`Sample::to_bytes`]), read from the
/// activity log (see [`crate::spill`]), as a Gadgetbridge bucket takes it
/// in.
pub fn bucket_minute(bytes: [u8; 6]) -> crate::gb_buckets::Minute<Option<&'static str>> {
    Sample::from_bytes(bytes).bucket_minute()
}

/// Puts back saved samples from `first` on (see [`History::restore`]).
pub fn restore_samples(first: i64, bytes: &[u8]) {
    HISTORY.lock(|history| {
        let mut history = history.borrow_mut();
        for (i, &sample) in bytes.as_chunks::<6>().0.iter().enumerate() {
            let sample = Sample::from_bytes(sample);
            history.restore(first + i as i64, sample);
        }
    });
}

/// Today's (local days since 1970, or `None` if the date wasn't known when
/// they were counted) steps, for saving.
pub fn steps_for_saving() -> (Option<i64>, u32) {
    TODAY.lock(|today| match &*today.borrow() {
        Some(t) => ((t.day != UNKNOWN_DAY).then_some(t.day), t.steps),
        None => (None, 0),
    })
}

/// Puts back today's saved steps, unless steps were already restored from
/// RAM (after a reset that kept it) or counted since boot.
pub fn restore_steps(day: Option<i64>, steps: u32) {
    TODAY.lock(|today| {
        let mut today = today.borrow_mut();
        if today.is_none() {
            let restored = Today {
                day: day.unwrap_or(UNKNOWN_DAY),
                steps,
            };
            save_today(&restored);
            *today = Some(restored);
        }
    });
}

// ==============================================================================
// Surviving resets
// ==============================================================================
//
// The step counter restarts whenever we reset the chip (every boot), so
// today's total is kept in reset-surviving RAM, like the clock (see
// `clock`): the first 32 bytes of the retained RAM in `memory.x`.

#[repr(C)]
struct Retained {
    magic: u32,
    steps: u32,
    day: i64,
    check: i64,
}

const RETAINED_MAGIC: u32 = 0x7572_5331; // "urS1"
/// Must match the `RETAINED` region in `memory.x`.
const RETAINED_ADDRESS: usize = 0x2000_D400;
const _: () = assert!(size_of::<Retained>() <= 32);

// The fields mixed and inverted, as `clock` does for its own block (its
// `Retained::check`), so that random power-on RAM contents can't pass
// for a saved total.
fn check(today: &Today) -> i64 {
    !(today.day ^ (i64::from(today.steps) << 24))
}

fn save_today(today: &Today) {
    let value = Retained {
        magic: RETAINED_MAGIC,
        steps: today.steps,
        day: today.day,
        check: check(today),
    };
    // SAFETY: Reserved, aligned RAM (see `memory.x`), only touched within
    // `TODAY`'s lock (and by `restore_today` before anything else runs).
    unsafe { (RETAINED_ADDRESS as *mut Retained).write_volatile(value) };
}

fn restore_today() {
    // SAFETY: As in `save_today`; any bit pattern is a valid `Retained`,
    // and the magic and check fields say whether it's real.
    let saved = unsafe { (RETAINED_ADDRESS as *const Retained).read_volatile() };
    let today = Today {
        day: saved.day,
        steps: saved.steps,
    };
    if saved.magic == RETAINED_MAGIC && saved.check == check(&today) {
        info!("activity: resumed with {} steps today", today.steps);
        TODAY.lock(|t| *t.borrow_mut() = Some(today));
    }
}
