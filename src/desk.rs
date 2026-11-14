//! The watch lying on a desk, off the wrist.
//!
//! **A desk** (or a table, a nightstand, the charger's cradle) holds the
//! watch perfectly still, which sleep tracking otherwise takes for a
//! sleeping wearer: [`crate::sleep`] calls 20 still minutes sleep, and the
//! heart rate sensor, which alone sees skin, often can't tell a desk from
//! a wrist (a desk reflects its LED much like skin, so a session ends
//! without a verdict). In the field (2026-11-14), the sealed watch lay on
//! a desk from 17:07, fell "asleep" at 17:27, started a nap's quiet, and
//! put the touch controller to sleep: taps put on at 17:58 did nothing,
//! and Gadgetbridge got a 50-minute nap that never happened.
//!
//! So [`Desk`] watches the accelerometer's 2-second polls for what only a
//! desk does, all of it for [`SETTLE`] (10 minutes):
//!
//! - **Level and face up**: the last sample of every poll within
//!   [`LEVEL`] of level on both horizontal axes (about 3.6°), gravity
//!   through the back. That's how the watch lies on a desk or the charger,
//!   on its back. A sleeping wrist rarely lies that level: in the field
//!   logs' six nights and two naps (42 hours asleep, 71 000 polls), 337
//!   polls did, in one spell of 11 minutes, broken twice by a stir.
//! - **Steady**: every poll within [`STEADY`] of the spell's first on each
//!   axis, and none moving more than [`STIR`]. A desk jitters by 5 or so;
//!   that sleeping wrist drifted 30 in 10 minutes.
//! - **Quiet**: the spell's mean movement at most [`QUIET`]. The desk and
//!   the charger, 26 to 31 (the sensor's noise; the charger the noisier);
//!   the level sleeping wrist, 45 (a pulse and breathing move it). In
//!   other poses, a sleeping wrist can be as quiet as 29 and as steady as
//!   a desk for half an hour, which is why level matters.
//! - **No pulse**: no heart rate reading since the spell began. A reading
//!   means skin, so a still spell with one in it is a wrist asleep, never a
//!   desk (as for `temperature::Wear`). Asleep, the heart rate is measured
//!   every 5 minutes, and in the field logs 99% of the minutes asleep had
//!   a reading in the 10 minutes before.
//!
//! Each holds a margin on its own: the level sleeping wrist's longest
//! steady spell was 6.3 minutes (7.4 with twice the steadiness), with a
//! reading in it, at a mean movement of 45. Every desk and charger spell
//! in the logs was found, 10 minutes after it began.
//!
//! A desk ends at any stir (picked up: a poll moves more than [`STIR`]),
//! or tilted off level, or at a reading. Nudged across the desk, it stays
//! one: the spell starts over from where it lies, its time on the desk
//! kept.
//!
//! What a desk does (see `crate::activity`): its minutes, back to the
//! start of the spell, are off the wrist in the history, as when the heart
//! rate sensor finds no skin, so they're never sleep, and the minutes
//! before can't borrow their stillness to fall asleep in (see
//! [`crate::sleep::still_among`]); Gadgetbridge gets them as not worn.
//!
//! Only face up: a watch lying on its side or face down isn't recognised
//! (a sleeping wrist lies on its side, level, now and then), and nor is
//! one on a desk that shakes (a desk being typed at). Either is the
//! still, unworn watch it was before.
//! TODO: the heart rate sensor's "no reading" on a calm wrist, session
//! after session, may tell those too.
//!
//! This module knows nothing about the hardware or the clock, so it can
//! be tested on a computer (see `host-tests/desk`).

/// How far from level (in the accelerometer's 1024-per-g counts, on each
/// horizontal axis) a desk lies: about 3.6°. The desks and charger in the
/// field logs lay within 54.
pub const LEVEL: u16 = 64;

/// Face up: gravity through the watch's back, as the sensor sees it
/// (1024 per g). Level, it's about 1030 deep; this only tells face up from
/// face down.
pub const FACE_UP: i16 = -900;

/// How far (1024 per g, on any axis) a desk's polls stray from its
/// spell's first: three times a desk's jitter, but under a sleeping
/// wrist's drift in 10 minutes.
pub const STEADY: u16 = 16;

/// A poll's movement (Bangle.js units, as `activity::movement`) that's
/// more than a twitch: a stir.
pub const STIR: u16 = 300;

/// The most mean movement (as [`STIR`]) of a desk's spell.
pub const QUIET: u16 = 34;

/// How long, in seconds, a spell must last to be a desk.
pub const SETTLE: u32 = 10 * 60;

/// A change in whether the watch lies on a desk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// Found to be on a desk since `since` (seconds, as passed to
    /// [`Desk::poll`]).
    Down { since: u32 },
    /// No longer on one.
    Up,
}

/// Whether the watch lies on a desk, from the accelerometer's polls (see
/// the module docs). 20 bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Desk {
    /// When the spell began (seconds), if the last poll was in one.
    since: Option<u32>,
    /// The spell's first sample, or where it was nudged to.
    anchor: [i16; 3],
    /// The spell's polls, and their movement.
    polls: u16,
    movement: u32,
    /// Whether it's a desk.
    on: bool,
}

impl Desk {
    /// Whether the watch lies on a desk.
    pub fn on(&self) -> bool {
        self.on
    }

    /// Takes in a poll at `now` (seconds; any clock that only moves on):
    /// its `movement` (Bangle.js units), its last `sample` (x, y, z, 1024
    /// per g), and when the latest heart rate reading was taken (seconds,
    /// on the same clock), if there's been one. Returns what changed.
    pub fn poll(
        &mut self,
        now: u32,
        movement: u16,
        sample: [i16; 3],
        pulse: Option<u32>,
    ) -> Option<Change> {
        let was = self.on;
        let [x, y, z] = sample;
        let level = x.unsigned_abs() <= LEVEL && y.unsigned_abs() <= LEVEL && z <= FACE_UP;
        if !level || movement > STIR {
            // Picked up, tilted, or a stir in place: the spell's over.
            *self = Self::default();
        } else {
            let strays = |anchor: [i16; 3]| {
                (0..3).any(|i| {
                    (i32::from(sample[i]) - i32::from(anchor[i])).unsigned_abs() > u32::from(STEADY)
                })
            };
            match self.since {
                // Nudged across a desk: from where it lies now, its time
                // there kept. (A spell not yet a desk starts over, as it
                // may be a wrist settling.)
                Some(_) if self.on && strays(self.anchor) => self.anchor = sample,
                Some(_) if !strays(self.anchor) => {}
                _ => {
                    *self = Self {
                        since: Some(now),
                        anchor: sample,
                        ..Self::default()
                    }
                }
            }
            self.polls = self.polls.saturating_add(1);
            self.movement = self.movement.saturating_add(u32::from(movement));
            let since = self.since.expect("set above");
            // A reading since the spell began: skin, not a desk.
            let skin = pulse.is_some_and(|at| at >= since);
            if skin {
                self.on = false;
            } else if !self.on
                && now.saturating_sub(since) >= SETTLE
                && self.movement <= u32::from(QUIET) * u32::from(self.polls)
            {
                self.on = true;
            }
        }
        match (was, self.on) {
            (false, true) => Some(Change::Down {
                since: self.since.expect("a desk is a spell"),
            }),
            (true, false) => Some(Change::Up),
            _ => None,
        }
    }
}
