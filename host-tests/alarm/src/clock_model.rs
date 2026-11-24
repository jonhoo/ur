//! The alarms against a clock that the phone keeps moving: a model-based
//! test, with Bolero generating what happens.
//!
//! Clock bugs are the kind that reach a wearer most: of the fixes up to
//! 0.12, about one in ten was the clock moving under something (timers
//! counted on it, summer time ringing alarms twice or not at all, the
//! smart alarm skipped), nearly all found in field logs weeks later. The
//! example tests in `main.rs` pin each of those down; this drives the
//! alarms through any mix of the clock's moves, second by second as
//! `App::check_alarms` checks, against a reference that holds the true
//! time and says, from the module's docs alone, what must ring:
//!
//! - Time passes; the phone sets the clock at each connection, a few
//!   seconds late at times (the field logs have 1 to 12 s at about one
//!   connection in six); summer time starts and ends, sent while
//!   connected or at the next connection; the phone's own clock is wrong
//!   by minutes to days and put right; the watch's crystal drifts; the
//!   watch resets, its clock resumed a second on (`clock::restore`) or
//!   lost; timers start; rings are dismissed, snoozed or left to ring
//!   out.
//! - Alarms at a time of day go by the local clock's high-water mark,
//!   the latest second checked. Moving on from it by up to
//!   [`CONTINUOUS`] (an hour of summer time, plus a phone's late
//!   seconds), every occurrence the clock reaches rings once, in time
//!   order, at most an hour late (`CATCH_UP`); back by up to as much,
//!   nothing rings again until the clock is past the mark. Moving
//!   [`JUMP`] or more on, what it moved over doesn't ring; back, what
//!   comes round again rings again. In between, either.
//! - Timers and the snooze ring once, at the first check their time
//!   has come in uptime, whatever the clock did meanwhile.
//! - Nothing rings that the clock hasn't reached.
//!
//! `cargo test` runs it for a second of random scenarios (Bolero's
//! default; `BOLERO_RANDOM_TEST_TIME_MS=600000` runs it for ten minutes),
//! and shrinks any failure to a short scenario ([`minimise`]). What it
//! has found is in `main.rs`: `winter_time_sent_late_rings_each_alarm_once`
//! (fixed), and two known gaps it allows for,
//! `queued_behind_an_hour_late_alarm_rings` and
//! `a_reset_in_an_alarms_minute_rings_it_once`.

use std::collections::BTreeMap;

use super::alarm::*;

const DAY: i64 = 24 * 60;

/// How far the local clock may move from the high-water mark, in
/// seconds, and still be one clock running on: summer time's hour, and
/// the seconds late a phone's time can arrive (12 s in the field logs;
/// a little more for the watch's own drift).
const CONTINUOUS: i64 = CATCH_UP * 60 + 15;

/// How far the local clock must move to be a jump: a new time zone, or a
/// phone's clock that was wrong, put right.
const JUMP: i64 = 2 * 60 * 60;

/// How late an alarm at a time of day may ring, in minutes: `CATCH_UP`,
/// and the minute the check before was in.
const LATEST: i64 = CATCH_UP + 1;

/// 2026-11-09 00:00 UTC, a Monday, in seconds.
const MONDAY: i64 = 20_766 * DAY * 60;

// ==============================================================================
// What happens
// ==============================================================================

#[derive(Clone, Copy, Debug, bolero::TypeGenerator)]
enum Move {
    /// Time passes, the UI checking every second.
    Pass { minutes: u8, seconds: u8 },
    /// The phone connects and sets the watch's clock to its own, `late`
    /// seconds late (modulo 13).
    Connect { late: u8 },
    /// Summer time starts (`forward`) or ends on the phone; Gadgetbridge
    /// sends it 5 s later if `connected`, else at the next connection.
    SummerTime {
        forward: bool,
        connected: bool,
        late: u8,
    },
    /// The phone's clock goes wrong by this many minutes (modulo three
    /// days either way), or right (0), for the next connection.
    PhoneClock { lead: i16 },
    /// The watch's crystal gains or loses a second or two.
    Drift { seconds: i8 },
    /// The watch resets, for `down` seconds: the alarms' state is lost
    /// (it's in the UI's RAM), and the clock resumes from the retained
    /// checkpoint a second on (`clock::restore`), or after System OFF or
    /// a drained battery, is unknown until the phone connects.
    Reset { down: u8, clock_kept: bool },
    /// A timer is started (or started again).
    StartTimer { second: bool },
}

/// How the wearer answers a ring.
#[derive(Clone, Copy, Debug, bolero::TypeGenerator)]
enum Answer {
    /// After this many seconds (modulo three minutes).
    Dismiss(u8),
    Snooze(u8),
    /// Not at all: it rings out after `RING_FOR`.
    RingOut,
}

#[derive(Clone, Debug, bolero::TypeGenerator)]
struct Scenario {
    /// Seconds after [`MONDAY`] that it starts, modulo a year.
    start: u32,
    /// The phone's UTC offset, in quarter hours (modulo ±14 hours).
    offset: i8,
    /// The smart alarm's weekday and weekend times, if it's on. The
    /// wearer never stirs, so it rings at its time, as an alarm does.
    smart: Option<(u16, u16)>,
    /// The phone's alarms: a time of day, and the weekdays (none is
    /// every day: one-offs turn off, which this doesn't model).
    alarms: Vec<(u16, u8)>,
    /// The two timers' durations, in minutes (modulo an hour).
    timers: (u8, u8),
    answers: Vec<Answer>,
    moves: Vec<Move>,
}

// ==============================================================================
// The reference
// ==============================================================================

/// An alarm's key in the reference: the smart alarm is 0, the phone's
/// `1 + i`.
fn key(kind: Kind) -> u8 {
    match kind {
        Kind::Smart => 0,
        Kind::Phone(i) => 1 + i,
        other => panic!("{other:?} isn't an alarm at a time of day here"),
    }
}

/// One occurrence of an alarm at a time of day: how often it has rung,
/// and how often it must and may have by the time the clock is past it.
#[derive(Clone, Copy, Debug)]
struct Occurrence {
    rings: u32,
    required: u32,
    allowed: u32,
}

#[derive(Default)]
struct Reference {
    /// The latest local second checked (the high-water mark), if any
    /// since the clock was last known; and whether the clock has moved
    /// back from it by more than [`CONTINUOUS`] but less than [`JUMP`]
    /// since, up to when (local seconds), so that what it reaches again
    /// before that may ring again or not.
    mark: Option<i64>,
    unsure_until: Option<i64>,
    /// By local minute and key.
    occurrences: BTreeMap<(i64, u8), Occurrence>,
    /// When each timer and the snooze ring, in uptime.
    timers: [Option<i64>; TIMERS],
    snooze: Option<i64>,
    /// The occurrence (local minute) the last check rang, if it rang one.
    /// `State::due` keeps that as the last minute checked, so a ring that
    /// was late leaves it behind the clock by as much: if the next check
    /// is more than [`LATEST`] on from it, it starts afresh, as after a
    /// jump, and what was queued to ring from before is dropped. Above
    /// all, an alarm that rang most of an hour late (summer time's hour,
    /// caught up) and rang out for `RING_FOR` drops what's queued behind
    /// it, even an alarm whose time had only just come: a known gap, and
    /// a rare one (it takes an alarm early in the hour summer time
    /// skips); see `queued_behind_an_hour_late_alarm_rings`.
    rang_at: Option<i64>,
}

impl Reference {
    /// The local minutes in `from..=to` that the alarms have an
    /// occurrence at, with their keys.
    fn occurrences_in(alarms: &Alarms, from: i64, to: i64) -> Vec<(i64, u8)> {
        let mut found = Vec::new();
        for day in from.div_euclid(DAY)..=to.div_euclid(DAY) {
            // 0 = Monday (1970-01-01 was a Thursday).
            let weekday = (day + 3).rem_euclid(7);
            if let Some((weekday_time, weekend_time)) = alarms.smart {
                let time = if weekday < 5 {
                    weekday_time
                } else {
                    weekend_time
                };
                found.push((day * DAY + i64::from(time), 0));
            }
            for (i, &(time, days)) in alarms.phone.iter().enumerate() {
                if days >> weekday & 1 != 0 {
                    found.push((day * DAY + i64::from(time), 1 + i as u8));
                }
            }
        }
        found.retain(|&(minute, _)| (from..=to).contains(&minute));
        found
    }

    /// The UI is about to check at local second `now`: the occurrences
    /// the clock reaches with it, and what each must do.
    fn reach(&mut self, alarms: &Alarms, now: i64) {
        let minute = now.div_euclid(60);
        // Checking afresh from this minute: as if the minute before had
        // just been checked.
        let afresh = minute * 60 - 1;
        let (mark, reset) = match self.mark {
            // The first check since the clock became known (or since a
            // reset, whose state said what rang): what's in this very
            // minute rings (the watch may have just been given the time at
            // 07:00:30).
            None => (afresh, true),
            // Back a long way: whatever comes round again rings again.
            Some(mark) if now - mark < -JUMP => {
                self.unsure_until = None;
                (afresh, false)
            }
            // Back further than summer time, not far enough to be a jump:
            // what comes round again before the mark may ring again, or
            // not.
            Some(mark) if now - mark < -CONTINUOUS => {
                self.unsure_until = Some(self.unsure_until.map_or(mark, |until| until.max(mark)));
                (afresh, false)
            }
            // Back within it: the minutes up to the mark were checked.
            Some(mark) if now <= mark => return,
            Some(mark) => (mark, false),
        };
        let moved = now - mark;
        // Back further than summer time, on further, or more than
        // `CATCH_UP` from the last occurrence rung: `State::due` may start
        // afresh, dropping what was queued from before.
        let back = self.mark.is_some_and(|mark| now - mark < -CONTINUOUS);
        let behind = self.rang_at.is_some_and(|at| minute - at > LATEST);
        if back || moved > CONTINUOUS || behind {
            for occurrence in self.occurrences.values_mut() {
                occurrence.required = occurrence.rings;
            }
        }
        let unsure_until = self.unsure_until.map(|until| until.div_euclid(60));
        for at in Self::occurrences_in(alarms, mark.div_euclid(60) + 1, minute) {
            let occurrence = self.occurrences.entry(at).or_insert(Occurrence {
                rings: 0,
                required: 0,
                allowed: 0,
            });
            let rings = occurrence.rings;
            let once = (rings + 1, rings + 1);
            let maybe = (rings, rings + 1);
            let never = (rings, rings);
            (occurrence.required, occurrence.allowed) = if reset && rings > 0 {
                // It rang before the reset, in this same minute, and may
                // again: a known gap (see
                // `a_reset_in_an_alarms_minute_rings_it_once`).
                maybe
            } else if at.1 == 0 && rings > 0 {
                // The smart alarm remembers the morning it last rang
                // across any move of the clock (`smart_rang`), but only
                // that one.
                maybe
            } else if unsure_until.is_some_and(|until| at.0 <= until) {
                maybe
            } else if at.0 < minute && moved > JUMP {
                never
            } else if at.0 < minute && (moved > CONTINUOUS || behind) {
                maybe
            } else {
                once
            };
        }
        if self.unsure_until.is_some_and(|until| now > until) {
            self.unsure_until = None;
        }
        self.mark = Some(now);
    }

    /// `State::due` rang `kind` at local second `now`.
    fn rang(&mut self, kind: Kind, now: i64) {
        let minute = now.div_euclid(60);
        let wanted = key(kind);
        // The earliest of it that may still ring, and isn't more than an
        // hour late. (One that may have rung after a move of the clock,
        // and didn't, stays allowed to for good, but too late to.)
        let Some((&(at, _), _)) = self
            .occurrences
            .range((minute - LATEST, 0)..)
            .find(|&(&(at, k), o)| k == wanted && at <= minute && o.rings < o.allowed)
        else {
            panic!(
                "{kind:?} rang at local minute {minute}, with nothing of it due in the hour before"
            );
        };
        // In time order: nothing before it was still to ring.
        if let Some((&(before, k), o)) = self
            .occurrences
            .range((minute - DAY, 0)..(at, 0))
            .find(|(_, o)| o.rings < o.required)
        {
            panic!("{kind:?} for minute {at} rang before alarm {k} for minute {before}: {o:?}");
        }
        // Every alarm at that minute rings as one.
        for ((_, _), occurrence) in self.occurrences.range_mut((at, 0)..=(at, u8::MAX)) {
            if occurrence.rings < occurrence.allowed {
                occurrence.rings += 1;
            }
        }
        self.rang_at = Some(at);
    }

    /// `State::due` rang nothing at local second `now`: nothing may be
    /// waiting to.
    fn quiet(&mut self, now: i64) {
        let minute = now.div_euclid(60);
        self.rang_at = None;
        // Older ones were looked at by the checks since (this runs every
        // second); the end of `World::run` looks at all of them.
        let recent = self
            .occurrences
            .range_mut((minute - DAY, 0)..=(minute, u8::MAX));
        for (&(at, k), occurrence) in recent {
            if occurrence.rings < occurrence.required {
                panic!("alarm {k} for minute {at} didn't ring by minute {minute}: {occurrence:?}");
            }
        }
    }

    /// A duration rang at `uptime`.
    fn elapsed(&mut self, kind: Kind, uptime: i64) {
        let slot = match kind {
            Kind::Timer(i) => &mut self.timers[usize::from(i)],
            Kind::Snooze => &mut self.snooze,
            other => panic!("{other:?} came from `elapsed`"),
        };
        match slot.take() {
            Some(at) if at <= uptime => {}
            Some(at) => panic!("{kind:?} for uptime {at} rang early, at {uptime}"),
            None => panic!("{kind:?} rang at uptime {uptime}, not running"),
        }
    }

    /// `State::elapsed` rang nothing at `uptime`: no duration may be over.
    fn none_elapsed(&self, uptime: i64) {
        for at in self.timers.iter().chain([&self.snooze]).flatten() {
            assert!(
                *at > uptime,
                "a duration for uptime {at} didn't ring by {uptime}"
            );
        }
    }
}

// ==============================================================================
// The watch
// ==============================================================================

/// The alarms of a [`Scenario`], as the reference reads them.
struct Alarms {
    smart: Option<(u16, u16)>,
    /// Each one's time, and weekdays (bit 0 Monday).
    phone: Vec<(u16, u8)>,
}

struct World {
    // The truth.
    utc: i64,
    uptime: i64,
    phone_lead: i64,
    phone_offset: i64,
    // The watch's clock: how far its UTC is off the true one, and its
    // offset (minutes), if it knows the time.
    clock: Option<(i64, i64)>,
    // The firmware.
    settings: Settings,
    phone: Phone,
    state: State,
    /// What rings: since when (uptime), and how it's answered.
    ringing: Option<(i64, Answer)>,
    answers: Vec<Answer>,
    answered: usize,
    alarms: Alarms,
    reference: Reference,
}

impl World {
    fn new(scenario: &Scenario) -> Self {
        let mut phone = Phone::default();
        let mut alarms = Alarms {
            smart: scenario
                .smart
                .map(|(weekday, weekend)| (weekday % DAY as u16, weekend % DAY as u16)),
            phone: Vec::new(),
        };
        for &(time, days) in scenario.alarms.iter().take(PHONE_ALARMS) {
            let time = time % DAY as u16;
            let days = match days & 0x7F {
                0 => 0x7F,
                days => days,
            };
            assert!(phone.push(time / 60, time % 60, days, true));
            alarms.phone.push((time, days));
        }
        let (weekday, weekend) = alarms.smart.unwrap_or((7 * 60, 9 * 60));
        let settings = Settings {
            smart: alarms.smart.is_some(),
            smart_weekday: weekday,
            smart_weekend: weekend,
            timers: [
                1 + u16::from(scenario.timers.0 % 60),
                1 + u16::from(scenario.timers.1 % 60),
            ],
            ..Settings::default()
        };
        let mut answers = scenario.answers.clone();
        if answers.is_empty() {
            answers.push(Answer::RingOut);
        }
        Self {
            utc: MONDAY + i64::from(scenario.start % (365 * 86_400)),
            uptime: 0,
            phone_lead: 0,
            phone_offset: i64::from(scenario.offset % 57) * 15,
            clock: None,
            settings,
            phone,
            state: State::default(),
            ringing: None,
            answers,
            answered: 0,
            alarms,
            reference: Reference::default(),
        }
    }

    fn local(&self) -> Option<i64> {
        self.clock
            .map(|(error, offset)| self.utc + error + offset * 60)
    }

    fn connect(&mut self, late: u8) {
        let late = i64::from(late % 13);
        self.clock = Some((self.phone_lead - late, self.phone_offset));
    }

    /// One second, and the UI's check at its end, as `App::check_alarms`
    /// makes it.
    fn second(&mut self) {
        self.utc += 1;
        self.uptime += 1;
        if let Some((since, answer)) = self.ringing {
            let rung = self.uptime - since;
            match answer {
                Answer::Dismiss(after) if rung > i64::from(after % 180) => self.ringing = None,
                Answer::Snooze(after) if rung > i64::from(after % 180) => {
                    self.state.snooze(self.uptime);
                    self.reference.snooze = Some(self.uptime + SNOOZE);
                    self.ringing = None;
                }
                _ if rung >= RING_FOR * 60 => self.ringing = None,
                _ => return,
            }
        }
        let rang = if let Some(kind) = self.state.elapsed(self.uptime) {
            self.reference.elapsed(kind, self.uptime);
            true
        } else {
            self.reference.none_elapsed(self.uptime);
            match self.local() {
                Some(now) => {
                    self.reference.reach(&self.alarms, now);
                    let due =
                        self.state
                            .due(&mut self.settings, &mut self.phone, now, Wearer::default());
                    match due {
                        Some(due) => {
                            assert!(!due.early, "the wearer never stirs");
                            self.reference.rang(due.kind, now);
                        }
                        None => self.reference.quiet(now),
                    }
                    // The clock face asks this every frame; it mustn't
                    // panic, whatever the clock did. (Once a minute here:
                    // it's most of the time a check takes.)
                    if now.rem_euclid(60) == 0 {
                        let _ = self
                            .state
                            .next(&self.settings, &self.phone, now, self.uptime);
                    }
                    due.is_some()
                }
                None => false,
            }
        };
        if rang {
            let answer = self.answers[self.answered % self.answers.len()];
            self.answered += 1;
            self.ringing = Some((self.uptime, answer));
        }
        for timer in 0..TIMERS {
            let expected = self.reference.timers[timer].map(|at| (at - self.uptime).max(0));
            assert_eq!(self.state.remaining(timer, self.uptime), expected);
        }
    }

    fn pass(&mut self, seconds: i64) {
        for _ in 0..seconds {
            self.second();
        }
    }

    fn apply(&mut self, step: Move) {
        match step {
            Move::Pass { minutes, seconds } => {
                self.pass(i64::from(minutes) * 60 + i64::from(seconds));
            }
            Move::Connect { late } => self.connect(late),
            Move::SummerTime {
                forward,
                connected,
                late,
            } => {
                let shift = if forward { 60 } else { -60 };
                // Within the offsets there are (±14 hours).
                if (self.phone_offset + shift).abs() <= 14 * 60 {
                    self.phone_offset += shift;
                }
                if connected {
                    self.pass(5);
                    self.connect(late);
                }
            }
            Move::PhoneClock { lead } => {
                self.phone_lead = i64::from(lead) % (3 * DAY) * 60;
            }
            Move::Drift { seconds } => {
                if let Some((error, _)) = &mut self.clock {
                    *error += i64::from(seconds % 3);
                }
            }
            Move::Reset { down, clock_kept } => {
                // `clock::checkpoint` saved the time at the last second;
                // `restore` resumes a second on from it, however long the
                // reset took.
                let resumed = self.local().map(|local| local + 1);
                let down = i64::from(down);
                self.utc += down;
                self.uptime = 0;
                self.state = State::default();
                self.ringing = None;
                self.reference.mark = None;
                self.reference.rang_at = None;
                // What was queued to ring is forgotten with the state.
                for occurrence in self.reference.occurrences.values_mut() {
                    occurrence.required = occurrence.rings;
                }
                self.reference.unsure_until = None;
                self.reference.timers = [None; TIMERS];
                self.reference.snooze = None;
                self.clock = match (self.clock, resumed) {
                    (Some((_, offset)), Some(resumed)) if clock_kept => {
                        Some((resumed - offset * 60 - self.utc, offset))
                    }
                    _ => None,
                };
            }
            Move::StartTimer { second } => {
                let timer = usize::from(second);
                self.state.start_timer(timer, &self.settings, self.uptime);
                self.reference.timers[timer] =
                    Some(self.uptime + i64::from(self.settings.timers[timer]) * 60);
            }
        }
    }

    /// Runs the scenario, then four hours with the clock left alone, for
    /// whatever is queued to ring; and checks that everything the clock
    /// reached rang as it must.
    fn run(scenario: &Scenario) {
        let mut world = Self::new(scenario);
        world.connect(0);
        for &step in &scenario.moves {
            world.apply(step);
        }
        world.connect(0);
        world.pass(4 * 60 * 60);
        for (&(at, k), occurrence) in &world.reference.occurrences {
            assert!(
                occurrence.rings >= occurrence.required,
                "alarm {k} for minute {at} never rang: {occurrence:?}"
            );
        }
    }
}

// ==============================================================================
// Tests
// ==============================================================================

/// Whether `scenario` fails, quietly.
fn fails(scenario: &Scenario) -> Option<String> {
    std::panic::catch_unwind(|| World::run(scenario))
        .err()
        .map(|panic| {
            panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
        })
}

/// A smaller scenario that still fails, by dropping and halving what it
/// can, one thing at a time, for as long as that keeps it failing.
/// Bolero's own shrinking makes little headway on these: a scenario runs
/// for some milliseconds, and its time to shrink runs out.
fn minimise(scenario: &Scenario) -> Scenario {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut best = scenario.clone();
    loop {
        let mut candidates = Vec::new();
        for i in 0..best.moves.len() {
            let mut smaller = best.clone();
            smaller.moves.remove(i);
            candidates.push(smaller);
            let mut smaller = best.clone();
            let halved = match smaller.moves[i] {
                Move::Pass { minutes, seconds } if minutes > 0 || seconds > 0 => Move::Pass {
                    minutes: minutes / 2,
                    seconds: if minutes > 0 { seconds } else { seconds / 2 },
                },
                Move::PhoneClock { lead } if lead != 0 => Move::PhoneClock { lead: lead / 2 },
                Move::Reset { down, clock_kept } if down > 0 => Move::Reset {
                    down: down / 2,
                    clock_kept,
                },
                Move::Connect { late } if late % 13 > 0 => Move::Connect {
                    late: late % 13 - 1,
                },
                _ => continue,
            };
            smaller.moves[i] = halved;
            candidates.push(smaller);
        }
        for i in 0..best.alarms.len() {
            let mut smaller = best.clone();
            smaller.alarms.remove(i);
            candidates.push(smaller);
        }
        for i in 0..best.answers.len() {
            let mut smaller = best.clone();
            smaller.answers.remove(i);
            candidates.push(smaller);
        }
        if best.smart.is_some() {
            candidates.push(Scenario {
                smart: None,
                ..best.clone()
            });
        }
        match candidates
            .into_iter()
            .find(|smaller| fails(smaller).is_some())
        {
            Some(smaller) => best = smaller,
            None => break,
        }
    }
    std::panic::set_hook(hook);
    best
}

/// Runs `scenario`, and if it fails, fails with the smallest version of
/// it that does.
fn check(scenario: &Scenario) {
    if fails(scenario).is_some() {
        let smallest = minimise(scenario);
        let why = fails(&smallest).unwrap_or_default();
        panic!("{why}\nin {smallest:#?}");
    }
}

/// Any scenario (see the module docs).
#[test]
fn alarms_ring_as_the_clock_says_whatever_it_does() {
    bolero::check!().with_type::<Scenario>().for_each(check);
}
