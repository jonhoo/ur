//! The activity history and Gadgetbridge's copy of it against a clock that
//! the phone keeps setting: a model-based test.
//!
//! `tests::put_right` has one wrong clock put right, minute by minute.
//! This runs any number of the phone's settings of the clock, second by
//! second, between the watch's polls (every 2 s, as `activity` polls the
//! accelerometer) and Gadgetbridge's fetches, some of them set while a
//! fetch is under way:
//!
//! - the phone trimming the clock by the seconds its time arrives late (1
//!   to 12 s at about one connection in six in the field logs), which now
//!   and then crosses back over the turn of a minute;
//! - the phone's own clock going wrong, by seconds to days either way,
//!   and being put right.
//!
//! The watch files each poll's steps under its clock's minute as
//! `activity::History::at` does (`minute_ring::file` and `carried`), and
//! answers fetches as `ble` does (`start`, `Fetch`, `synced_after_set` at
//! each setting, `Fetch::clock_set` for one under way). Gadgetbridge
//! stores the buckets as `tests::upsert` does. The reference is the true
//! time, what the wearer walked when, and what the watch filed under
//! which minute (all of it, where the history lets go of the oldest).
//! Not modelled: the activity log, which a fetch reads what the history
//! no longer holds from, and restores after a reset.
//!
//! What must hold, at the end, after a quarter of an hour more and a
//! last fetch:
//!
//! - With only trims, no step is lost or counted twice: the history holds
//!   every step walked, and Gadgetbridge every bucket from where its
//!   first fetch started as the watch has it.
//! - Whatever the phone did, Gadgetbridge has every bucket as the watch
//!   has it from the one its clock last went back over a minute in (a
//!   wrong clock put right: what was filed after that is gone from the
//!   history, as it was filed under a time that wasn't), or from where
//!   its first fetch started if that's later; as far back as the history
//!   reaches.
//! - Nothing a fetch sends is dated after the watch's clock.

use std::collections::BTreeMap;

use quickcheck::{Arbitrary, Gen};

use super::gb_buckets::{Fetch, MINUTES, Minute, start, start_of, synced_after_set};
use super::minute_ring;
use super::tests::{History, Stored, quickcheck, reference, send};

/// The history's length (`activity::HISTORY_MINUTES`).
const HISTORY: i64 = minute_ring::MINUTES as i64;

/// How often the watch files what the accelerometer counted
/// (`activity::POLL_SECONDS`).
const POLL: i64 = 2;

/// 2026-11-09 12:00 UTC, in seconds.
const NOON: i64 = super::tests::NOON * 60;

#[derive(Clone, Copy, Debug)]
enum Step {
    /// Polls go by, the wearer walking.
    Walk(u16),
    /// The phone sets the clock to its own, `late` seconds late (modulo
    /// 13), as it does on connecting.
    Set { late: u8 },
    /// The phone's clock goes wrong by this many seconds, or right (0), for
    /// its next setting.
    PhoneClock { lead: i64 },
    /// Gadgetbridge fetches (after setting the clock, if it isn't the
    /// phone's: it does as it connects); with `set`, the phone sets the
    /// clock once that many buckets have gone out, `late` seconds late.
    Fetch { set: Option<(u8, u8)> },
}

impl Arbitrary for Step {
    fn arbitrary(g: &mut Gen) -> Self {
        match u8::arbitrary(g) % 10 {
            0..=3 => Step::Walk(u16::arbitrary(g) % 900),
            4 | 5 => Step::Set {
                late: u8::arbitrary(g),
            },
            6 => Step::PhoneClock {
                lead: match u8::arbitrary(g) % 4 {
                    0 => 0,
                    // Seconds.
                    1 => i64::from(i8::arbitrary(g)),
                    // Minutes to two days, either way.
                    _ => i64::from(i32::arbitrary(g) % (2 * 24 * 60 * 60)),
                },
            },
            _ => Step::Fetch {
                set: bool::arbitrary(g).then(|| (u8::arbitrary(g) % 8, u8::arbitrary(g))),
            },
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        match *self {
            Step::Walk(polls) if polls > 0 => Box::new([Step::Walk(polls / 2)].into_iter()),
            Step::PhoneClock { lead } if lead != 0 => {
                Box::new([Step::PhoneClock { lead: lead / 2 }].into_iter())
            }
            Step::Fetch { set: Some(_) } => Box::new([Step::Fetch { set: None }].into_iter()),
            _ => Box::new(std::iter::empty()),
        }
    }
}

struct World {
    /// The true time (UTC seconds), the phone's clock's lead on it, and
    /// the watch's.
    real: i64,
    lead: i64,
    error: i64,
    /// The watch's history, as `activity::History` files it.
    history: History,
    newest: Option<i64>,
    /// Every step walked (filed in a poll).
    walked: u64,
    /// The steps filed under each minute, as the history keeps them, but
    /// for its length: what it lets go of is still here.
    filed: BTreeMap<i64, u32>,
    /// The watch's last minute before the last setting that took its clock
    /// back over a minute, and the minute it took it to.
    put_right: Option<(i64, i64)>,
    // Gadgetbridge's samples, where the phone's copy ends as far as the
    // watch knows, and the phone's clock at its last fetch.
    store: BTreeMap<i64, Stored>,
    synced: Option<i64>,
    last_sync_ms: Option<i64>,
    /// Where the first fetch started, and the phone's copy ends after the
    /// last.
    first: Option<i64>,
    end: i64,
}

impl World {
    fn new() -> Self {
        Self {
            real: NOON,
            lead: 0,
            error: 0,
            history: History::new(),
            newest: None,
            walked: 0,
            filed: BTreeMap::new(),
            put_right: None,
            store: BTreeMap::new(),
            synced: None,
            last_sync_ms: None,
            first: None,
            end: i64::MIN,
        }
    }

    /// The watch's clock, in UTC seconds.
    fn watch(&self) -> i64 {
        self.real + self.error
    }

    /// The oldest minute `History::since` reads.
    fn oldest(&self) -> i64 {
        self.newest
            .map_or(i64::MIN / 2, |newest| newest - HISTORY + 1)
    }

    /// One poll: `steps` filed under the watch's minute, as
    /// `activity::History::at` files them.
    fn poll(&mut self, steps: u16) {
        self.real += POLL;
        let minute = self.watch().div_euclid(60);
        let carried =
            minute_ring::carried(self.newest, minute).and_then(|m| self.history.get(&m).copied());
        // What's filed, as the history files it, but for letting go of the
        // oldest: what goes back over the turn of a minute is carried, and
        // what's after a minute the clock went back to is dropped.
        if self.newest.is_some_and(|newest| minute < newest) {
            let carried = carried.map_or(0, |c| u32::from(c.steps));
            self.filed.split_off(&(minute + 1));
            *self.filed.entry(minute).or_default() += carried;
        }
        *self.filed.entry(minute).or_default() += u32::from(steps);
        let (stale, newest) = minute_ring::file(self.newest, minute, HISTORY);
        for m in stale {
            self.history.remove(&m);
        }
        // What `file` empties are slots, each of which may have held any of
        // the minutes `HISTORY` apart: none of those older than the ring now
        // reaches is left, even if it's back in reach after the clock went
        // back.
        self.history = self.history.split_off(&(newest - HISTORY + 1));
        self.newest = Some(newest);
        let sample = self.history.entry(minute).or_insert(Minute {
            steps: 0,
            movement: 1,
            heart_rate: 0,
            seen: true,
            kind: None,
        });
        if let Some(carried) = carried {
            sample.steps += carried.steps;
            if sample.heart_rate == 0 {
                sample.heart_rate = carried.heart_rate;
            }
        }
        sample.steps += steps;
        // A heart rate now and then, so that buckets differ in it too.
        if self.real % 300 == 0 {
            sample.heart_rate = 50 + (self.real / 300 % 100) as u8;
        }
        self.walked += u64::from(steps);
    }

    /// The phone sets the watch's clock to its own, `late` seconds late, and
    /// the watch takes it as `ble` does.
    fn set(&mut self, late: u8) -> i64 {
        let before = self.watch().div_euclid(60);
        self.error = self.lead - i64::from(late % 13);
        let minute = self.watch().div_euclid(60);
        if minute < before - 1 {
            self.put_right = Some((before, minute));
        }
        self.synced = synced_after_set(self.synced, minute);
        minute
    }

    /// Gadgetbridge fetches, as `ble::Fetching` answers it; with `set`, the
    /// phone sets the clock once that many buckets have gone out (or all of
    /// them, if fewer), and the rest go out after.
    fn fetch(&mut self, set: Option<(u8, u8)>) {
        // The phone sets the clock as it connects, and whenever its own
        // changes while connected: a fetch always finds the watch's clock
        // its own, give or take the seconds late that can arrive.
        if (self.real + self.lead - self.watch()).abs() > 12 {
            self.set(0);
        }
        let phone_ms = (self.real + self.lead) * 1000;
        let asked = self
            .last_sync_ms
            .map_or(phone_ms - 86_400_000, |last| last - 60_000);
        let now = self.watch().div_euclid(60);
        let mut fetch = Fetch::new(start(asked, self.synced, now), self.oldest() + MINUTES, now);
        self.first.get_or_insert(fetch.from());
        let mut sent = 0;
        let mut set = set;
        loop {
            if let Some((after, late)) = set
                && sent >= after
            {
                let minute = self.set(late);
                fetch.clock_set(minute);
                set = None;
            }
            let from = fetch.from().max(self.oldest());
            let minutes = self.history.range(from..).map(|(&m, &s)| (m, s));
            let Some(bucket) = fetch.next(minutes) else {
                break;
            };
            assert!(
                bucket.timestamp_ms() / 1000 < self.watch(),
                "a bucket from {} sent at {}, dated after the watch's clock",
                bucket.start,
                self.watch() / 60
            );
            send(&mut self.store, &bucket);
            sent += 1;
        }
        if let Some((_, late)) = set {
            let minute = self.set(late);
            fetch.clock_set(minute);
        }
        self.synced = Some(fetch.synced_through());
        self.last_sync_ms = Some(phone_ms);
        self.end = fetch.synced_through() + 1;
    }

    fn apply(&mut self, step: Step) {
        match step {
            Step::Walk(polls) => {
                for _ in 0..polls {
                    // Never none, so that every bucket sent overwrites what
                    // Gadgetbridge had at its time.
                    self.poll(1 + (self.real / POLL % 3) as u16);
                }
            }
            Step::Set { late } => {
                self.set(late);
            }
            Step::PhoneClock { lead } => self.lead = lead,
            Step::Fetch { set } => self.fetch(set),
        }
    }

    /// Runs `steps`, then a quarter of an hour of walking, the phone's clock
    /// as it was last, and a last fetch.
    fn run(steps: &[Step]) -> Self {
        let mut world = Self::new();
        world.set(0);
        for &step in steps {
            world.apply(step);
        }
        world.apply(Step::Walk(15 * 30));
        world.fetch(None);
        world
    }

    /// The buckets from `from` to where the phone's copy ends that
    /// Gadgetbridge doesn't have as the watch does: their starts, with the
    /// steps it has (if any) and the watch's. As far back as a fetch reads
    /// the history alone: `ble` leaves out its oldest bucket, which may be
    /// going as the fetch reads it, and takes what's older from the
    /// activity log, which this doesn't model.
    ///
    /// Nor a bucket the history has let go of some of the minutes of, and
    /// has back in reach after the clock went back (a slot it emptied, so
    /// an empty minute), which isn't the watch's to send either:
    /// Gadgetbridge may have it whole (`Fetch::new`).
    fn lost(&self, from: i64) -> Vec<(i64, Option<u32>, u32)> {
        let from = start_of(from).max(start_of(self.oldest() + 2 * MINUTES - 1));
        (from..self.end)
            .step_by(MINUTES as usize)
            .filter_map(|start| {
                let bucket = reference(&self.history, start)?;
                let filed: u32 = self
                    .filed
                    .range(start..start + MINUTES)
                    .map(|(_, &s)| s)
                    .sum();
                if filed != bucket.steps {
                    return None;
                }
                let stored = self.store.get(&(bucket.timestamp_ms() / 1000));
                let stored = stored.map(|s| s.steps);
                (stored != Some(bucket.steps)).then_some((start, stored, bucket.steps))
            })
            .collect()
    }
}

/// Any trims of the clock, between any polls and fetches, some under way:
/// every step walked is in the history once, and Gadgetbridge has every
/// bucket from where it first fetched as the watch has it.
#[test]
fn trims_lose_and_double_no_step() {
    fn property(steps: Vec<Step>) -> bool {
        let steps: Vec<Step> = steps
            .into_iter()
            .filter(|step| !matches!(step, Step::PhoneClock { .. }))
            .collect();
        let world = World::run(&steps);
        // Unless the history has let go of the oldest (more than 14½
        // hours of it).
        let total: u64 = world.history.values().map(|m| u64::from(m.steps)).sum();
        let whole = world.real - NOON < (HISTORY - 1) * 60;
        if whole && total != world.walked {
            eprintln!(
                "{steps:?}: walked {}, the history has {total}",
                world.walked
            );
            return false;
        }
        let lost = world.lost(world.first.expect("fetched at the end"));
        if !lost.is_empty() {
            eprintln!("{steps:?}: lost {lost:?}");
        }
        lost.is_empty()
    }
    quickcheck().quickcheck(property as fn(Vec<Step>) -> bool);
}

/// Anything the phone does with the clock: Gadgetbridge has every bucket
/// as the watch has it from where the clock last went back over a minute
/// (or from where it first fetched).
#[test]
fn whatever_the_phone_does_the_watch_and_gadgetbridge_agree() {
    fn property(steps: Vec<Step>) -> bool {
        let world = World::run(&steps);
        let first = world.first.expect("fetched at the end");
        let from = world.put_right.map_or(first, |(_, to)| to.max(first));
        let lost = world.lost(from);
        if !lost.is_empty() {
            eprintln!("{steps:?}: from {from}, lost {lost:?}");
        }
        lost.is_empty()
    }
    quickcheck().quickcheck(property as fn(Vec<Step>) -> bool);
}
