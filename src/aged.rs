//! What the watch keeps of the minutes its history has let go of.
//!
//! [`crate::activity`]'s history holds the last 14½ hours by the minute
//! (see [`crate::minute_ring::MINUTES`] for why that long, and no longer
//! a day): what the sleep tracking still works on. Older minutes are in
//! the activity log in the SPI flash, for the phone (see `crate::spill`).
//! But two things on the watch itself reach further back than the
//! history, up to a day, and neither can read the flash (they're asked
//! for by the UI, whose task has no RAM to spare for a flash read's
//! future):
//!
//! - **The Today screen**, which charts the day hour by hour since local
//!   midnight, and today's step total, which is taken from the same sums
//!   when the date changes other than by midnight passing (see
//!   [`crate::day`]).
//! - **The Last night screen**, which shows the last sleep until the
//!   next: by the afternoon, a night's first hours are older than the
//!   history.
//!
//! Neither needs the minutes themselves. So as the history lets go of a
//! minute, it's added here ([`Aged::take`]): to the sum of its hour, and,
//! if it was asleep, to the totals of the sleep it was part of. What the
//! screens show is then these and the history's minutes together
//! ([`Aged::add_to`], [`Aged::last_sleep`]), exactly what a history a day
//! long would have shown; the tests check it against one
//! (`host-tests/aged`).
//!
//! A boot rebuilds it without any code of its own: the history is put
//! back by filing the log's minutes oldest first (see
//! `system_off::restore`), each of which pushes an older one out, and in
//! here.
//!
//! ## Hours
//!
//! The sums are by the hour, of the clock on the wrist: an hour starts
//! when the local time's minutes are 00, so in a zone half an hour off
//! UTC (India), on UTC's half hour. Each is kept by when it started, in
//! UTC, so a change of zone by whole hours (nearly every flight, summer
//! time) files nothing wrong: the chart reads each hour at its new local
//! time.
//!
//! TODO: after a move between zones that aren't whole hours apart, the
//! sums from before it straddle the new zone's hours until they're a day
//! old: each is charted in the hour it started in, and the one that
//! straddles local midnight is left out of today. Quarter-hour sums would
//! fix that at four times the RAM.
//!
//! Hardware-free, so it runs in host tests.

use crate::day::{Day, Hour};
use crate::sleep::{Kind, SAME_NIGHT_GAP};
use crate::stages::Minutes;

const DAY: i64 = 24 * 60;

/// A minute as kept here: UTC minutes since 1970 in 32 bits (as the
/// activity log has them), which the CPU divides itself, where 64 bits
/// are a routine's work, and a division or two is done for every minute
/// let go of. 0, which no real minute is (the phone sets the clock), is
/// "none": so a fresh [`Aged`] is all zeros, and the static it's part of
/// (`activity::HISTORY`, 5 KB) takes no flash for its initial values.
type Minute = u32;

fn minute32(minute: i64) -> Minute {
    minute.clamp(0, i64::from(u32::MAX)) as Minute
}

/// The part of a sleep that the history has let go of.
#[derive(Clone, Copy)]
struct Slept {
    /// Its first minute.
    start: Minute,
    /// The last minute of sleep taken in (0 if there's none).
    last: Minute,
    /// The stages of those from `start` to `last`.
    stages: Minutes,
}

/// What's kept of the minutes the history has let go of: `N` hours' sums
/// (as many as lie between the history's oldest minute and a day ago,
/// and one more, as neither end falls on the hour), and the totals of the
/// newest sleep among them.
pub struct Aged<const N: usize> {
    /// The newest minute taken in (0 if none): minutes come in order, and
    /// none twice.
    through: Minute,
    /// Each hour's sum, hour `h` (see [`hour_of`]) in slot `h % N`.
    hours: [Hour; N],
    /// The newest hour with a slot (0 if none has): the slots are those
    /// of the `N` hours up to it.
    newest_hour: u32,
    slept: Slept,
}

/// How far into UTC's hours the local ones start, at `offset` minutes
/// ahead of UTC: 0, but for the few zones that aren't whole hours off.
fn phase(offset: i16) -> u32 {
    offset.rem_euclid(60) as u32
}

/// The hour `minute` is in, counted from 1970, for a wearer `offset`
/// minutes ahead of UTC: the same number for every minute between one
/// local hour's start and the next.
fn hour_of(minute: Minute, offset: i16) -> u32 {
    minute.saturating_add(phase(offset)) / 60
}

impl<const N: usize> Default for Aged<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Aged<N> {
    pub const fn new() -> Self {
        Self {
            through: 0,
            hours: [Hour::NONE; N],
            newest_hour: 0,
            slept: Slept {
                start: 0,
                last: 0,
                stages: Minutes {
                    light: 0,
                    deep: 0,
                    rem: 0,
                },
            },
        }
    }

    fn slot(hour: u32) -> usize {
        hour as usize % N
    }

    /// Takes in a minute (UTC minutes since 1970) the history is letting
    /// go of: its steps, heart rate (0 if none) and sleep kind, as they
    /// were at the last, for a wearer `offset` minutes ahead of UTC now.
    ///
    /// Minutes come oldest first. One that isn't after every minute taken
    /// in so far is ignored: it's been counted (a history put back after a
    /// reset may hold a minute in two of the records it's read from).
    pub fn take(&mut self, minute: i64, offset: i16, steps: u16, heart_rate: u8, kind: Kind) {
        let minute = minute32(minute);
        if minute <= self.through {
            return;
        }
        self.through = minute;

        // Its hour's sum, in a slot emptied of the hour it held `N`
        // hours before (as are those of any hours skipped).
        let hour = hour_of(minute, offset);
        if hour > self.newest_hour {
            let first = (self.newest_hour + 1).max(hour.saturating_sub(N as u32 - 1));
            for emptied in first..=hour {
                self.hours[Self::slot(emptied)] = Hour::NONE;
            }
            self.newest_hour = hour;
        }
        // (An hour before the newest: the zone has changed, by less than
        // an hour. It still has its slot.)
        if self.newest_hour - hour < N as u32 {
            self.hours[Self::slot(hour)].add(steps, heart_rate);
        }

        // And its sleep's totals: the same sleep as the last minute
        // asleep, unless the wearer was up for longer than a night's
        // wakes since (as `sleep::last_sleep` tells one sleep from the
        // next).
        if kind.is_sleep() {
            let same =
                self.slept.last != 0 && minute - self.slept.last - 1 <= SAME_NIGHT_GAP as u32;
            if !same {
                self.slept.start = minute;
                self.slept.stages = Minutes::default();
            }
            self.slept.last = minute;
            // (Saturating: a watch left lying still for weeks, taken for
            // asleep all the while.)
            let stages = &mut self.slept.stages;
            match kind {
                Kind::Light => stages.light = stages.light.saturating_add(1),
                Kind::Deep => stages.deep = stages.deep.saturating_add(1),
                Kind::Rem => stages.rem = stages.rem.saturating_add(1),
                _ => {}
            }
        }
    }

    /// Forgets what was taken in of `minute` and after, as the clock has
    /// gone back to it: those minutes were filed by a clock that was
    /// ahead, and the history has dropped the rest of them (see
    /// [`crate::minute_ring`]). That's a clock put back by more than the
    /// history holds: a phone's that was wrong by most of a day.
    ///
    /// The whole of `minute`'s hour goes, the minutes before it too: a
    /// sum can't give back a part. So does the sleep, if any of it was
    /// after.
    pub fn back_to(&mut self, minute: i64, offset: i16) {
        let minute = minute32(minute);
        if minute > self.through {
            return;
        }
        self.through = minute.saturating_sub(1);
        let hour = hour_of(minute, offset);
        let first = hour.max(self.newest_hour.saturating_sub(N as u32 - 1));
        for emptied in first..=self.newest_hour {
            self.hours[Self::slot(emptied)] = Hour::NONE;
        }
        self.newest_hour = self.newest_hour.min(hour.saturating_sub(1));
        if self.slept.last >= minute {
            self.slept.last = 0;
        }
    }

    /// Adds the hours' sums to `day`, which began at `midnight` (UTC
    /// minutes since 1970) for a wearer `offset` minutes ahead of UTC:
    /// those of the hours since. With the history's own minutes since
    /// midnight added by the minute, that's the day so far.
    pub fn add_to(&self, day: &mut Day, midnight: i64, offset: i16) {
        let oldest = self.newest_hour.saturating_sub(N as u32 - 1);
        for hour in oldest..=self.newest_hour {
            // When the hour started, and how far into the day that is: a
            // whole number of hours, unless the zone's minutes have
            // changed since (see the module docs).
            let into = i64::from(hour) * 60 - i64::from(phase(offset)) - midnight;
            if (0..DAY).contains(&into) {
                day.hours[into as usize / 60].merge(&self.hours[Self::slot(hour)]);
            }
        }
    }

    /// The most recent sleep, given the most recent in the history
    /// (`recent`: its first minute, the minute after its last, and its
    /// stages), whose newest minute is `newest`: the same, with the part
    /// of it that the history has let go of; or, with none in the
    /// history, the last one that it let go of, for a day after its end.
    ///
    /// A sleep is one with its older part when the wearer wasn't up for
    /// longer than a night's wakes in between
    /// ([`crate::sleep::SAME_NIGHT_GAP`]), as within the history.
    pub fn last_sleep(
        &self,
        newest: i64,
        recent: Option<(i64, i64, Minutes)>,
    ) -> Option<(i64, i64, Minutes)> {
        let older = (self.slept.last != 0).then_some(self.slept);
        let last = |older: Slept| i64::from(older.last);
        match (older, recent) {
            (Some(older), Some((start, end, stages)))
                if start - last(older) - 1 <= SAME_NIGHT_GAP as i64 =>
            {
                let stages = Minutes {
                    light: stages.light.saturating_add(older.stages.light),
                    deep: stages.deep.saturating_add(older.stages.deep),
                    rem: stages.rem.saturating_add(older.stages.rem),
                };
                Some((i64::from(older.start), end, stages))
            }
            (_, Some(recent)) => Some(recent),
            (Some(older), None) if newest - last(older) < DAY => {
                Some((i64::from(older.start), last(older) + 1, older.stages))
            }
            _ => None,
        }
    }
}
