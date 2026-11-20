//! Telling a workout from ordinary activity, by the minute, so that the
//! heart rate can be measured through it (see `heart`, which measures
//! continuously for as long as [`detect`] says one is under way).
//!
//! Why it's needed: the heart rate's schedule waits for a calm wrist and
//! backs off when a session fails, which suits a desk and loses exercise
//! altogether. On the sealed watch (2026-11-07 to 20, 12 days of logs, a
//! Garmin alongside until the 16th), sessions during walks read 4 times
//! in 36, and Health Connect got no reading at 110 BPM or more, though
//! the Garmin was at 100 or more for 165 minutes of it.
//!
//! (The figures here are from replaying those logs through these rules:
//! `host-tests/workout`'s binary does that for any decoded log.)
//!
//! What a workout looks like to this watch, from the same logs:
//!
//! - **Walking or running** is the step counter keeping a pace. Minutes
//!   with 100 steps or more had the Garmin at a median 123 BPM, against 60
//!   for minutes with none (see [`WALK_STEPS`]).
//! - **Effort without steps** (weights, carrying, housework) is the wrist
//!   moving hard for most of ten minutes (see [`EFFORT_MOVEMENT`]). This
//!   one is a weaker sign, and it's untested on what it's meant for: the
//!   one strength session of those two weeks (2026-11-12, the Garmin at
//!   90-118) fell in a stretch the field log doesn't cover. Its false
//!   starts are cheap (some LED time), so it errs towards starting.
//!
//! The heart rate itself isn't part of the rule: a reading is exactly what
//! a moving wrist rarely gives, so a rule that waited for an elevated one
//! would start late or never.
//!
//! What detection can't do is make a moving wrist readable. Walking, the
//! light level swings by 30-700 counts at the arm's rhythm against a
//! pulse of ~5, and the PPG's spectrum peaks at half the step rate, the
//! step rate and three halves of it; the Garmin's heart rate on those
//! walks (111-125) sat within 2-10 BPM of the step rate (121-127), so
//! rejecting the accelerometer's harmonics would reject the pulse too.
//! Readings come in the pauses instead: a calm stretch of ~14 s is enough
//! for one, and measuring continuously catches them all, where a session
//! that first waits for calm needs ~18 s. The same logs held 18 such
//! stretches inside walks and 90 inside efforts, where sessions read 4 and
//! 18 times. (And at the walk's end: the workout runs on for
//! [`WALK_END_MINUTES`], through the first minutes of recovery.)
//!
//! Like [`crate::sleep`], this knows nothing of hardware or the clock, so
//! it runs in host tests (`host-tests/workout`), whose binary replays a
//! field log's minutes through it.

/// What kind of workout is under way; for the log, and for [`swing`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
#[repr(u8)]
pub enum Kind {
    /// The step counter keeps a walking pace or faster (running too: the
    /// rule has no upper limit, and the logs no run to set one by).
    Walk = 1,
    /// The wrist is working hard without many steps.
    Effort = 2,
}

impl Kind {
    /// From [`Kind`] as a byte; anything else is no workout.
    pub fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            1 => Some(Self::Walk),
            2 => Some(Self::Effort),
            _ => None,
        }
    }

    /// As a byte, 0 for no workout: what `activity` keeps.
    pub fn bits(kind: Option<Self>) -> u8 {
        kind.map_or(0, |kind| kind as u8)
    }
}

/// One finished minute, as `activity`'s history has it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Minute {
    pub steps: u16,
    /// The minute's mean movement (see `activity::Sample::movement`).
    pub movement: u16,
}

/// How many finished minutes [`detect`] looks at, the latest last.
pub const WINDOW: usize = 10;

/// Steps in a minute that make it one of walking. A purposeful walk
/// counted 100-117 a minute on the sealed watch (the counter misses some:
/// the wearer's cadence is ~120), pottering about a room 10-40. 80 leaves
/// room for a minute cut short by a crossing.
pub const WALK_STEPS: u16 = 80;

/// Minutes of walking in a row that start a workout.
///
/// Three: in 12 days, 26 starts (2.2 a day, 26 minutes a day), among them
/// all 16 walks of eight minutes or more, each three minutes in; 8 were
/// over within five minutes (a walk to a shop's door). With the Garmin
/// alongside, its median through these was 120 BPM, 75% of its points at
/// 100 or more. Asking for four of five minutes instead makes 18 starts
/// and the same walks, a minute or two later each; a start is cheap, and
/// the first minutes of a walk are when the heart rate climbs.
pub const WALK_MINUTES: usize = 3;

/// Steps in a minute below which the walk has paused.
pub const WALK_END_STEPS: u16 = 40;

/// Minutes of pause in a row that end a walk: long enough for a red
/// light or a shop's queue not to, and for the first minutes of recovery
/// to be measured, when the wrist is at last still and the heart rate
/// still near what it was (it falls ~20-30 BPM in the first minute or
/// two).
pub const WALK_END_MINUTES: usize = 3;

/// A minute's mean movement that counts as the wrist working hard. At a
/// desk a minute averages 150-800, and handling things 800-2000; of the
/// minutes at 2000 or more without walking, the Garmin's median was 82-86
/// BPM against 53-69 below 800.
pub const EFFORT_MOVEMENT: u16 = 2000;

/// How many of the last [`WINDOW`] minutes must reach [`EFFORT_MOVEMENT`]
/// to start a workout without steps.
///
/// Eight of ten: 21 starts in 12 days (1.8 a day, 42 minutes a day), the
/// Garmin's median through them 93 BPM where an awake wrist's is ~68, a
/// third of its points at 100 or more. That's "up and doing something",
/// not proof of exercise: the longest (117 minutes, an evening at 5 steps
/// a minute) can't have been a workout, and it's the rule that costs the
/// most LED for what it's sure of. Five of six starts twice as often for
/// a median of 90.
///
/// TODO: tune on a field log that has a strength session in it (the rule
/// has never seen one); reps may well show up as bursts between rests
/// rather than as eight busy minutes.
pub const EFFORT_MINUTES: usize = 8;

/// A minute's mean movement below which an effort has let up.
pub const EFFORT_END_MOVEMENT: u16 = 1000;

/// Minutes below [`EFFORT_END_MOVEMENT`] in a row that end an effort:
/// longer than a rest between sets.
pub const EFFORT_END_MINUTES: usize = 5;

const _: () = assert!(WALK_MINUTES <= WINDOW && WALK_END_MINUTES <= WINDOW);
const _: () = assert!(EFFORT_MINUTES <= WINDOW && EFFORT_END_MINUTES <= WINDOW);
// What ends a workout can't also start it, or it would never end.
const _: () = assert!(WALK_END_STEPS < WALK_STEPS && EFFORT_END_MOVEMENT < EFFORT_MOVEMENT);

/// Whether a workout is under way, given whether one was (`current`) and
/// the last [`WINDOW`] finished minutes, oldest first (a minute the watch
/// didn't record is [`Minute::default`]). Called once a minute.
///
/// Asleep, never: turning over isn't effort, and the night's schedule is
/// its own (walking wakes the sleep classifier at once).
///
/// A walk takes over from an effort, as the better description; and a
/// walk that ends where an effort would start (the wrist still working)
/// carries on as one.
pub fn detect(current: Option<Kind>, recent: &[Minute; WINDOW], asleep: bool) -> Option<Kind> {
    if asleep {
        return None;
    }
    let last = |minutes: usize| &recent[WINDOW - minutes..];
    let walking = last(WALK_MINUTES).iter().all(|m| m.steps >= WALK_STEPS);
    let working = recent
        .iter()
        .filter(|m| m.movement >= EFFORT_MOVEMENT)
        .count()
        >= EFFORT_MINUTES;
    let current = match current {
        Some(Kind::Walk)
            if last(WALK_END_MINUTES)
                .iter()
                .all(|m| m.steps < WALK_END_STEPS) =>
        {
            None
        }
        Some(Kind::Effort)
            if last(EFFORT_END_MINUTES)
                .iter()
                .all(|m| m.movement < EFFORT_END_MOVEMENT) =>
        {
            None
        }
        current => current,
    };
    if walking {
        Some(Kind::Walk)
    } else if current.is_none() && working {
        Some(Kind::Effort)
    } else {
        current
    }
}

/// A heart rate below which a window measured while walking isn't
/// trusted (see [`swing`]).
pub const SWING_BPM: f32 = 70.0;

/// Whether a window's rate is more likely the arm's swing than the pulse:
/// under [`SWING_BPM`], in a [`Kind::Walk`], on a wrist that counted steps
/// within the last `stepless_limit` polls (`stepless_polls` being how many
/// in a row, up to now, counted none).
///
/// A walking arm swings at half the step rate, 40-65 times a minute at 80
/// to 130 steps, which is as periodic as a pulse and passes for one: on
/// one walk, sessions read 58-60 BPM against a Garmin's 97, and two of the
/// four windows accepted in 80 walked sessions were at 51 and 57. A few
/// minutes into a walk the heart rate is nowhere near that (the Garmin's
/// lowest median over a walk was 87), so nothing true is lost. Measuring
/// through a whole walk makes hundreds of windows where a session made
/// eleven, and three of them agreeing on the swing would put a resting
/// rate in the middle of a workout.
///
/// Once the steps stop, anything goes again: recovery is what the pauses
/// are measured for.
///
/// TODO: a run's arms swing at 75-90, which this doesn't cover; the logs
/// have no run to judge by. With the accelerometer's samples (the logging
/// build's `accel:` lines) the swing's own rate could be measured and
/// rejected instead of a fixed band.
pub fn swing(bpm: f32, kind: Option<Kind>, stepless_polls: u8, stepless_limit: u8) -> bool {
    kind == Some(Kind::Walk) && stepless_polls < stepless_limit && bpm < SWING_BPM
}
