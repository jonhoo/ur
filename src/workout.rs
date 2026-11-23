//! Telling a workout from ordinary activity, by the minute, so that its
//! minutes can be marked as exercise, and the heart rate measured through
//! the kind that can be (see `heart`, which measures continuously for as
//! long as [`detect`] says an effort is under way, and not through a walk:
//! see "Walks aren't measured through" below).
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
//!   one is a weaker sign: it can't tell a strength session from an
//!   evening of busy hands, and it has seen one strength session
//!   (2026-11-22, 26 minutes by the training app's record). What started
//!   the effort there was the nine hard minutes of arriving and setting
//!   up, a minute before the first set. The lifting alone wouldn't have:
//!   9 of its 27 minutes were hard, the rest at 600-1900, since a wrist
//!   that holds a bar hardly moves and the rests are longer than the
//!   sets. So an effort is started by a busy stretch and kept by the odd
//!   hard minute after it (see [`EFFORT_LAPSE_MINUTES`]).
//!
//! An effort's false starts aren't cheap unless they end. The sensor is
//! on for every minute of one (that day's `power:` lines: 1128 s of
//! LED in an hour that held 17 minutes of one, 210-270 s in the hours
//! without), and the battery's reading fell a point for every ten minutes
//! or so of it, twice that day: from 98% to 94% over an effort of 36
//! minutes, and from 87% to 80% over one of 68, against a point an hour
//! between the two. That second one was the strength session, still
//! going 40 minutes after it when the log was fetched: moving about at
//! home kept the wrist at 1000-1800, never quiet for five minutes in a
//! row, which was all that ended an effort then. So the start errs
//! towards starting, and the end mustn't err towards going on.
//!
//! TODO: that point per ten minutes is about three times what
//! `heart`'s "Through a workout" and docs/power-plan.md reckon from an
//! estimated ~2.8 mA; measure a continuous session's draw on the dev kit.
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
//! 18 times.
//!
//! # Walks aren't measured through
//!
//! An effort has those pauses (the rests between sets: 118 readings in
//! the strength session's 46 minutes). A walk that keeps its pace has
//! none, and up to 0.12.0-rc.2 the sensor stayed on through walks all the
//! same. On the sealed watch, 2026-11-23, two walks of some twenty
//! minutes at 110-150 steps a minute were found three minutes in, and
//! their sessions (1382 s and 1501 s of LED) took no reading while the
//! wearer walked: of ~1180 windows, the ones that weren't aperiodic or
//! unsteady had locked onto the arm's swing at half the cadence (55-65
//! BPM), which [`swing`] rightly leaves out. Some forty minutes of LED,
//! at a point of battery for every ten, for the eight readings of the
//! first walk's recovery.
//!
//! So a walk starts no session of its own (user, 2026-11-23): the heart
//! rate's schedule carries on through it as through any other minutes,
//! with its own wait for calm, which takes a pause at a crossing or the
//! walk's end when a reading is due or overdue (see `heart_schedule`'s
//! "Seeking calm"). What that gives up is the readings of pauses the
//! schedule isn't looking in: the 18 calm stretches above were in 12 days
//! of walks.
//!
//! TODO: heart rate through a walk needs a motion-aware estimator, not
//! more LED time: one that takes the cadence from the accelerometer (the
//! step rate, and the swing at half of it) and looks past those peaks in
//! the PPG's spectrum for the pulse's, or cancels them. The field result
//! above is what says so: 40 minutes of continuous windows, none usable.
//! It would need walks recorded against a reference to be built on (see
//! `heart`'s `WORKOUT_TRACE_CHECKS`), and a way to tell the pulse from
//! the step rate when the two are within 10 BPM, as they were on the
//! walks with the Garmin.
//!
//! A walk is still a workout in every other way: its minutes reach
//! Gadgetbridge as exercise (as an effort's do; see
//! `activity::Sample::gadgetbridge_kind`), a reading during one never
//! counts as a resting one, and [`swing`] guards the schedule's sessions
//! through it.
//!
//! TODO: no manual start or stop (user, 2026-11-21: not for now). It
//! would be the sure way to cover what the effort rule can't tell from
//! housework; `activity` notes where it would go.
//!
//! Like [`crate::sleep`], this knows nothing of hardware or the clock, so
//! it runs in host tests (`host-tests/workout`), whose binary replays a
//! field log's minutes through it.

/// What kind of workout is under way; for the log, for `heart` (which
/// measures through an effort only), and for [`swing`].
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
/// light or a shop's queue not to.
///
/// (While walks were measured through, this also kept the sensor on for
/// the first minutes of recovery. Now the schedule's wait for calm takes
/// those, if a reading is due.)
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
/// TODO: a strength session that starts from a calm wrist wouldn't be
/// found: the one seen (2026-11-22, see the module's docs) had 9 hard
/// minutes in 27, in bursts between rests, and it was the busy stretch
/// before it that made eight of ten, only just (with the minutes cut 15
/// or 20 s later than the watch cut them, no effort starts until the
/// wearer has packed up and left). A rule for bursts would need more
/// sessions to set it by, and would start on more housework; a manual
/// start is the sure way.
pub const EFFORT_MINUTES: usize = 8;

/// A minute's mean movement below which the wrist has gone quiet.
pub const EFFORT_END_MOVEMENT: u16 = 1000;

/// Quiet minutes (below [`EFFORT_END_MOVEMENT`]) in a row that end an
/// effort: the wearer has sat down. Longer than a rest between sets,
/// which had one or two such minutes at a time on 2026-11-22.
pub const EFFORT_END_MINUTES: usize = 5;

/// Minutes in a row without one at [`EFFORT_MOVEMENT`] that end an effort,
/// however much the wrist still moves: the hard minutes have stopped
/// coming.
///
/// Why not quiet alone: after a workout the wearer is up and about, at
/// 1000-1800 a minute, and that is what the rests between sets look like
/// too (on 2026-11-22: 600-1900 in the session, 540-1640 in the 19
/// minutes after the wearer had left it). Only the hard minutes differ:
/// as the watch cut the minutes, one came at least every fifth minute
/// through the session, and one in the 34 minutes after the leaving.
///
/// Ten, the whole [`WINDOW`], because where the minutes are cut matters:
/// as the watch cut them, the session's longest stretch without a hard
/// minute was 4, but cut 4 s earlier a minute at 2028 becomes one at
/// 1914, and the stretch 9. Replayed at 30 cuts 2 s apart, with an effort
/// under way as the session began, it lasted the session at 10 of them
/// with five minutes, 23 with seven or eight, 26 with nine, and all 30
/// with ten. An effort that ends in a session doesn't start again (see
/// [`EFFORT_MINUTES`]), so the session's readings are lost, which costs
/// more than the minutes of LED: with ten the effort ends ten minutes
/// after its last hard minute (the 8 minutes of packing up and leaving,
/// at 2700-6300, are as hard as any and count as part of it), 19 after
/// the training app's end, where it had run on for 40 and more.
///
/// On the earlier logs (2026-11-13 to 22, 8.8 days, 18 efforts) no effort
/// starts that didn't, and none splits in two; four end sooner (152
/// minutes to 47; 70 and more to 46; 66 to 21; 57 to 55), and the
/// efforts' minutes go from 668 to 492. Seven minutes would make that
/// 381, and lose the session at 7 cuts of the 30. Counting a minute as
/// hard from 1800 lasted it at all 30 cuts with seven, for 414 minutes,
/// but that is a second threshold, set by one session and with a minute
/// to spare (as ten has here).
///
/// The heart rate is no help here either, though a workout has plenty of
/// readings: it was mostly 76-109 through the session and still 75-98
/// for the 19 minutes of moving about that followed, under 80 only once
/// the wearer sat down, which the quiet minutes say too.
pub const EFFORT_LAPSE_MINUTES: usize = 10;

const _: () = assert!(WALK_MINUTES <= WINDOW && WALK_END_MINUTES <= WINDOW);
const _: () = assert!(EFFORT_MINUTES <= WINDOW && EFFORT_END_MINUTES <= WINDOW);
const _: () = assert!(EFFORT_LAPSE_MINUTES <= WINDOW);
// What ends a workout can't also start it, or it would never end.
const _: () = assert!(WALK_END_STEPS < WALK_STEPS && EFFORT_END_MOVEMENT < EFFORT_MOVEMENT);
// Nor may an effort lapse in the minute it starts: the minutes that lapse
// it leave too few others in the window to start one.
const _: () = assert!(WINDOW - EFFORT_LAPSE_MINUTES < EFFORT_MINUTES);

/// Whether a workout is under way, given whether one was (`current`) and
/// the last [`WINDOW`] finished minutes, oldest first (a minute the watch
/// didn't record is [`Minute::default`]). Called once a minute.
///
/// Asleep, never: turning over isn't effort, and the night's schedule is
/// its own (walking wakes the sleep classifier at once).
///
/// A walk takes over from an effort, as the better description. When it
/// ends, an effort only follows if the minutes since the walking would
/// start one by themselves (see [`hard_since_walking`]): the walk's own
/// minutes move as hard as any effort's, and aren't one.
pub fn detect(current: Option<Kind>, recent: &[Minute; WINDOW], asleep: bool) -> Option<Kind> {
    if asleep {
        return None;
    }
    let last = |minutes: usize| &recent[WINDOW - minutes..];
    let walking = last(WALK_MINUTES).iter().all(|m| m.steps >= WALK_STEPS);
    let working = hard_since_walking(recent) >= EFFORT_MINUTES;
    let current = match current {
        Some(Kind::Walk)
            if last(WALK_END_MINUTES)
                .iter()
                .all(|m| m.steps < WALK_END_STEPS) =>
        {
            None
        }
        // Quiet, or no longer working hard (see `EFFORT_LAPSE_MINUTES` for
        // why quiet alone doesn't do).
        Some(Kind::Effort)
            if last(EFFORT_END_MINUTES)
                .iter()
                .all(|m| m.movement < EFFORT_END_MOVEMENT)
                || last(EFFORT_LAPSE_MINUTES)
                    .iter()
                    .all(|m| m.movement < EFFORT_MOVEMENT) =>
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

/// How many of `recent`'s minutes count towards starting an effort: those
/// at [`EFFORT_MOVEMENT`] or more since the wearer last walked, which is
/// the last [`WALK_MINUTES`] in a row at [`WALK_STEPS`] or more (what
/// [`detect`] calls walking). With no such run among them, all ten count.
///
/// Why: a walk's minutes are at 3000-6000, well over [`EFFORT_MOVEMENT`],
/// so counting them let a walk end as an effort: as its steps stopped,
/// the last ten minutes still held seven of walking, and one hard minute
/// of slowing down made eight. On 2026-11-23 both of the day's walks did
/// (`workout: now Effort (steps 20, 20, 20; 9/10 min moving)`), and ran
/// on for 4 and 5 minutes with the sensor on, until five quiet ones ended
/// them; on the earlier logs such tails lasted 17 and 30 minutes, kept
/// going by the odd hard minute.
///
/// Why this rule and not a simpler one:
///
/// - Leaving out every minute at [`WALK_STEPS`] or more, in a run or not,
///   ends those walks the same, but changes efforts that no walk was
///   near: hard work has the odd minute of crossing a room at pace.
///   Replayed over the logs of 2026-11-13 to 23, two efforts no longer
///   start (of 21 and 27 minutes), and both of 2026-11-22 start a minute
///   later, the strength session among them, whose start had no minute to
///   spare (see [`EFFORT_MINUTES`]).
/// - Counting only since the last single minute at walking pace loses
///   that strength session altogether: one minute of 128 steps sits in
///   the middle of the nine hard ones that started it.
///
/// This one changes nothing unless three walking minutes in a row are
/// among the last ten, which is to say unless a walk was under way in
/// them. On those logs no effort that wasn't a walk's tail starts or ends
/// differently. Of the six walks that ended as efforts, four now simply
/// end; one is followed 21 minutes later by an effort of its own (its
/// hard minutes came after the walk); and one still hands over: its last
/// eleven minutes were stop-and-go (22-114 steps, never three at pace)
/// and then stepless, all but one of them hard, which is an effort by
/// any reading of the rule.
///
/// The price: hard work straight after a walk (walking to the gym, then
/// lifting) takes its own eight minutes of ten to be found, where the
/// walk's minutes used to vouch for it at once; and an effort that a walk
/// interrupts doesn't resume as the walk ends, but is found afresh. Up to
/// eight minutes of an effort's readings, against a tail on every walk.
pub fn hard_since_walking(recent: &[Minute; WINDOW]) -> usize {
    // Where the last run of walking ends, as an index into `recent`: the
    // minutes from there on are the ones since.
    let walked = recent
        .windows(WALK_MINUTES)
        .rposition(|run| run.iter().all(|m| m.steps >= WALK_STEPS))
        .map_or(0, |run| run + WALK_MINUTES);
    recent[walked..]
        .iter()
        .filter(|m| m.movement >= EFFORT_MOVEMENT)
        .count()
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
/// lowest median over a walk was 87), so nothing true is lost. The two
/// walks measured through on 2026-11-23 bear it out: the windows that
/// were periodic at all were at 55-65, half the cadence, and three of
/// them agreeing would have put a resting rate in the middle of a walk.
/// Walks aren't measured through since (see the module's docs), but the
/// schedule's sessions and looks at the clock face still fall in them.
///
/// Once the steps stop, anything goes again: a pause is when a walk's
/// heart rate can be read at all.
///
/// TODO: a run's arms swing at 75-90, which this doesn't cover; the logs
/// have no run to judge by. With the accelerometer's samples (the logging
/// build's `accel:` lines) the swing's own rate could be measured and
/// rejected instead of a fixed band.
pub fn swing(bpm: f32, kind: Option<Kind>, stepless_polls: u8, stepless_limit: u8) -> bool {
    kind == Some(Kind::Walk) && stepless_polls < stepless_limit && bpm < SWING_BPM
}
