//! Packing for the field log's busiest lines, so that its 2 MB ring holds
//! days rather than a day (see [`crate::field_log`]).
//!
//! Two kinds of line were 86% of a worn day's log (0.11.1's): the
//! accelerometer's poll every 2 s (`activity: N samples, movement M, …`,
//! 60% on its own, mostly the timestamp and argument tags each line
//! carries), and the heart rate sensor's samples with the level check
//! before them, every second of a session. A field build logs these
//! instead:
//!
//! - **A minute's polls in one line** ([`Polls`]): their movement, steps,
//!   and sample counts as arrays, the step counter and the watch's
//!   orientation as at the last of them, and how long they spanned.
//! - **Samples as differences** ([`pack_samples`]): a second's samples are
//!   ~3,800 give or take a few dozen, so each is a byte of difference from
//!   the one before rather than two of value, and the level check's
//!   numbers ride on the same line (its mean and peak are the samples'
//!   own, so they're left out).
//!
//! defmt can't format these back, so `scripts/expand_log.py` rewrites the
//! decoded log's packed lines into the ones they replace, which is what
//! every script and replay reads. Nothing here touches hardware, so it's
//! tested on the host (host-tests/log_pack), against the same byte
//! sequences the expander's own test decodes.

/// The most bytes [`pack_samples`] writes for each sample.
pub const PACKED_PER_SAMPLE: usize = 3;

/// In [`pack_samples`]' output: the next two bytes are a sample whole
/// (little-endian), not a difference. -128 is a difference no sample is
/// written as, so this can't be mistaken for one.
pub const ESCAPE: u8 = 0x80;

/// Writes `samples` into `out` as differences, and returns how many bytes
/// that took: the first sample, and any that differs from the one before
/// by more than 127 either way, as [`ESCAPE`] and the sample's two bytes;
/// each other as one byte, the difference as an `i8`. Stops at a sample
/// that doesn't fit, so `out` should have [`PACKED_PER_SAMPLE`] bytes for
/// each sample.
///
/// On 0.11.1's sessions, 97% of differences fit a byte: the line takes
/// ~30 bytes a second where the samples as `u16`s took ~70.
pub fn pack_samples(samples: &[u16], out: &mut [u8]) -> usize {
    let mut len = 0;
    let mut previous: Option<u16> = None;
    for &sample in samples {
        let delta = previous.map(|before| i32::from(sample) - i32::from(before));
        let bytes = match delta {
            Some(delta @ -127..=127) => &[delta as i8 as u8][..],
            _ => {
                let [low, high] = sample.to_le_bytes();
                &[ESCAPE, low, high][..]
            }
        };
        let Some(room) = out.get_mut(len..len + bytes.len()) else {
            break;
        };
        room.copy_from_slice(bytes);
        len += bytes.len();
        previous = Some(sample);
    }
    len
}

/// Polls to a line: a minute's, at one every 2 s.
pub const POLLS: usize = 30;

/// What [`Polls::samples`] counts from: nearly every poll has 24 samples
/// (12.5 Hz for 2 s), so nearly every count is logged as a zero, which the
/// log's encoding (rzCOBS) keeps in a bit.
pub const SAMPLES_FROM: u8 = 24;

/// The accelerometer's polls since the last line, up to [`POLLS`] of them.
/// What it keeps is lossless for every analysis of the polls (movement
/// and steps per poll, samples per poll for weighting): only each poll's
/// exact time (they're 2 s apart, give or take the timer) and its step
/// counter and orientation (the line has the last poll's) are left out.
pub struct Polls {
    movement: [u16; POLLS],
    steps: [u16; POLLS],
    samples: [u8; POLLS],
    len: u8,
    /// When the first poll was, in ms of uptime.
    first_ms: u64,
}

impl Polls {
    pub const fn new() -> Self {
        Self {
            movement: [0; POLLS],
            steps: [0; POLLS],
            samples: [0; POLLS],
            len: 0,
            first_ms: 0,
        }
    }

    /// Adds a poll at `now_ms` of uptime, with its sample count, movement
    /// and new steps (each saturating, as the minute history does), and
    /// says whether that made [`POLLS`] of them: time to log the line and
    /// [`clear`](Self::clear).
    pub fn push(&mut self, now_ms: u64, samples: u32, movement: u64, steps: u32) -> bool {
        let at = usize::from(self.len);
        if at >= POLLS {
            return true;
        }
        if at == 0 {
            self.first_ms = now_ms;
        }
        self.movement[at] = movement.min(u64::from(u16::MAX)) as u16;
        self.steps[at] = steps.min(u32::from(u16::MAX)) as u16;
        self.samples[at] = (samples.min(u32::from(u8::MAX)) as u8).wrapping_sub(SAMPLES_FROM);
        self.len += 1;
        usize::from(self.len) == POLLS
    }

    /// Each poll's movement, oldest first.
    pub fn movement(&self) -> &[u16] {
        &self.movement[..usize::from(self.len)]
    }

    /// Each poll's new steps, oldest first.
    pub fn steps(&self) -> &[u16] {
        &self.steps[..usize::from(self.len)]
    }

    /// Each poll's sample count less [`SAMPLES_FROM`], wrapping (so 23 is
    /// 255), oldest first.
    pub fn samples(&self) -> &[u8] {
        &self.samples[..usize::from(self.len)]
    }

    /// How long ago the first poll was, at `now_ms` of uptime.
    pub fn span_ms(&self, now_ms: u64) -> u32 {
        now_ms
            .saturating_sub(self.first_ms)
            .min(u64::from(u32::MAX)) as u32
    }

    /// Starts afresh, once the polls are logged.
    pub fn clear(&mut self) {
        self.len = 0;
    }
}
