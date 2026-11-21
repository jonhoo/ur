//! Host tests for the firmware's `aged` module: what the watch keeps of
//! the minutes its history has let go of, for the screens that show a
//! day. `cargo test` runs them.
//!
//! What they check: a history of 14½ hours with `aged` shows what a
//! history with no end would. So there are two here, fed the same
//! minutes: [`tests::Watch`], which does what `activity::History` does
//! with the firmware's own `minute_ring` and `aged`, and
//! [`tests::Everything`], which forgets nothing and adds up the obvious
//! way.

#[path = "../../../src/aged.rs"]
#[allow(dead_code)]
mod aged;
#[path = "../../../src/day.rs"]
#[allow(dead_code)]
mod day;
#[path = "../../../src/minute_ring.rs"]
#[allow(dead_code)]
mod minute_ring;
#[path = "../../../src/sleep.rs"]
#[allow(dead_code)]
mod sleep;
#[path = "../../../src/stages.rs"]
#[allow(dead_code)]
mod stages;

fn main() {
    println!("run `cargo test`");
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::aged::Aged;
    use super::day::{self, Day};
    use super::minute_ring;
    use super::sleep::{self, Kind};
    use super::stages::Minutes;

    const DAY: i64 = 24 * 60;
    /// 2026-11-09 12:00 UTC, in minutes since 1970.
    const NOON: i64 = 29_903_760;

    /// What a minute recorded.
    #[derive(Clone, Copy, Default, PartialEq, Debug)]
    struct Sample {
        steps: u16,
        heart_rate: u8,
        kind: Option<Kind>,
    }

    impl Sample {
        fn kind(&self) -> Kind {
            self.kind.unwrap_or(Kind::Unknown)
        }
    }

    // ==========================================================================
    // The watch, and a watch that forgets nothing
    // ==========================================================================

    /// `activity::History`, as far as this goes: a ring of `len` minutes,
    /// and `aged` with `N` hours for what the ring lets go of.
    struct Watch<const N: usize> {
        len: i64,
        samples: Vec<Option<Sample>>,
        newest: Option<i64>,
        aged: Aged<N>,
    }

    impl<const N: usize> Watch<N> {
        /// A ring of `len` minutes; `N` must be the hours between its
        /// oldest minute and a day ago, and one more (as
        /// `activity::AGED_HOURS`).
        fn new(len: usize) -> Self {
            assert_eq!(N, (DAY as usize - len).div_ceil(60) + 1);
            Self {
                len: len as i64,
                samples: vec![None; len],
                newest: None,
                aged: Aged::new(),
            }
        }

        fn slot(&self, minute: i64) -> usize {
            minute.rem_euclid(self.len) as usize
        }

        fn let_go(&mut self, minute: i64, offset: i16, sample: Option<Sample>) {
            if let Some(sample) = sample {
                self.aged.take(
                    minute,
                    offset,
                    sample.steps,
                    sample.heart_rate,
                    sample.kind(),
                );
            }
        }

        /// As `History::at`.
        fn at(&mut self, minute: i64, offset: i16) -> &mut Option<Sample> {
            let (stale, newest) = minute_ring::file(self.newest, minute, self.len);
            if self.newest.is_some_and(|newest| minute < newest) {
                self.aged.back_to(minute, offset);
            }
            for old in minute_ring::let_go(self.newest, minute, self.len) {
                let sample = self.samples[self.slot(old)];
                self.let_go(old, offset, sample);
            }
            for m in stale {
                let slot = self.slot(m);
                self.samples[slot] = None;
            }
            self.newest = Some(newest);
            let slot = self.slot(minute);
            &mut self.samples[slot]
        }

        /// As `History::restore`.
        fn restore(&mut self, minute: i64, offset: i16, sample: Sample) {
            match self.newest {
                Some(newest) if minute <= newest => {
                    if newest - minute < self.len {
                        let slot = self.slot(minute);
                        self.samples[slot].get_or_insert(sample);
                    } else {
                        self.let_go(minute, offset, Some(sample));
                    }
                }
                _ => *self.at(minute, offset) = Some(sample),
            }
        }

        /// As `History::day`.
        fn day(&self, now: i64, offset: i16) -> Day {
            let midnight = day::midnight(now, offset);
            let mut day = Day::default();
            self.aged.add_to(&mut day, midnight, offset);
            let Some(newest) = self.newest else {
                return day;
            };
            let oldest = (newest - self.len + 1).max(midnight);
            for minute in oldest..=newest.min(now) {
                if let Some(sample) = self.samples[self.slot(minute)] {
                    day.add(
                        (minute - midnight) as usize,
                        sample.steps,
                        sample.heart_rate,
                    );
                }
            }
            day
        }

        /// As `History::last_sleep`, less the staging.
        fn last_sleep(&self) -> Option<(i64, i64, Minutes)> {
            let newest = self.newest?;
            let oldest = newest - self.len + 1;
            let kind = |minute: i64| {
                self.samples[self.slot(minute)].map_or(Kind::Unknown, |sample| sample.kind())
            };
            let recent = sleep::last_sleep(self.len as usize, |i| kind(oldest + i as i64)).map(
                |(start, end, _)| {
                    let (start, end) = (oldest + start as i64, oldest + end as i64);
                    (start, end, Minutes::count((start..end).map(kind)))
                },
            );
            self.aged.last_sleep(newest, recent)
        }
    }

    /// A watch that keeps every minute, and adds up the obvious way.
    #[derive(Default)]
    struct Everything {
        samples: BTreeMap<i64, Sample>,
        newest: Option<i64>,
    }

    impl Everything {
        fn at(&mut self, minute: i64) -> &mut Sample {
            // A clock put back drops what was filed after the new time.
            self.samples.split_off(&(minute + 1));
            self.newest = Some(minute);
            self.samples.entry(minute).or_default()
        }

        fn day(&self, now: i64, offset: i16) -> Day {
            let midnight = day::midnight(now, offset);
            let mut day = Day::default();
            for (&minute, sample) in self.samples.range(midnight..=now) {
                day.add(
                    (minute - midnight) as usize,
                    sample.steps,
                    sample.heart_rate,
                );
            }
            day
        }

        fn steps(&self, minutes: std::ops::RangeInclusive<i64>) -> u32 {
            self.samples
                .range(minutes)
                .map(|(_, sample)| u32::from(sample.steps))
                .sum()
        }

        /// The most recent sleep, for a day after its end.
        fn last_sleep(&self) -> Option<(i64, i64, Minutes)> {
            let newest = self.newest?;
            let &oldest = self.samples.keys().next()?;
            let kind = |minute: i64| {
                self.samples
                    .get(&minute)
                    .map_or(Kind::Unknown, Sample::kind)
            };
            let minutes = (newest - oldest + 1) as usize;
            let (start, end, _) = sleep::last_sleep(minutes, |i| kind(oldest + i as i64))?;
            let (start, end) = (oldest + start as i64, oldest + end as i64);
            (newest - (end - 1) < DAY).then(|| (start, end, Minutes::count((start..end).map(kind))))
        }
    }

    /// Both, fed the same.
    struct Pair<const N: usize> {
        watch: Watch<N>,
        everything: Everything,
    }

    impl<const N: usize> Pair<N> {
        fn new(len: usize) -> Self {
            Self {
                watch: Watch::new(len),
                everything: Everything::default(),
            }
        }

        fn record(&mut self, minute: i64, offset: i16, sample: Sample) {
            *self.watch.at(minute, offset) = Some(sample);
            *self.everything.at(minute) = sample;
        }
    }

    // ==========================================================================
    // Days to feed them
    // ==========================================================================

    /// xorshift64*, so runs are reproducible without dependencies.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// A wearer's days, a minute at a time: stretches awake (walking some
    /// of the time) and asleep (in stages, with wakes short and long),
    /// with a heart rate now and then.
    struct Wearer {
        rng: Rng,
        asleep: bool,
        /// Minutes left of the stretch under way.
        left: u64,
        /// The longest sleep, in minutes.
        longest: u64,
    }

    impl Wearer {
        fn new(seed: u64, longest: u64) -> Self {
            Self {
                rng: Rng(seed | 1),
                asleep: false,
                left: 0,
                longest,
            }
        }

        fn next(&mut self) -> Sample {
            if self.left == 0 {
                self.asleep = !self.asleep;
                self.left = if self.asleep {
                    1 + self.rng.below(self.longest)
                } else {
                    // Wakes either side of what joins two sleeps into one
                    // night, and days.
                    match self.rng.below(4) {
                        0 => 1 + self.rng.below(sleep::SAME_NIGHT_GAP as u64),
                        1 => sleep::SAME_NIGHT_GAP as u64 + self.rng.below(3),
                        _ => 60 + self.rng.below(16 * 60),
                    }
                };
            }
            self.left -= 1;
            let heart_rate = match self.rng.below(5) {
                0 => 45 + self.rng.below(120) as u8,
                _ => 0,
            };
            if self.asleep {
                let kind = match self.rng.below(8) {
                    0 => Kind::Restless,
                    1 | 2 => Kind::Deep,
                    3 => Kind::Rem,
                    _ => Kind::Light,
                };
                Sample {
                    steps: 0,
                    heart_rate,
                    kind: Some(kind),
                }
            } else {
                Sample {
                    steps: match self.rng.below(3) {
                        0 => self.rng.below(130) as u16,
                        _ => 0,
                    },
                    heart_rate,
                    kind: Some(Kind::Awake),
                }
            }
        }
    }

    /// UTC offsets to live at: whole hours either side, and the zones
    /// that aren't (India, Nepal, the Chatham Islands, Newfoundland).
    const OFFSETS: [i16; 9] = [0, 60, -480, 540, 14 * 60, 330, 345, 765, -210];

    /// Runs a wearer's `minutes` on both watches from noon, at `offset`,
    /// and checks every minute that the watch shows what the one that
    /// forgets nothing does. `gap` in a thousand minutes aren't recorded
    /// (the watch was off, or lost the time).
    fn agree<const N: usize>(len: usize, seed: u64, offset: i16, minutes: i64, gap: u64) {
        let mut pair = Pair::<N>::new(len);
        let mut wearer = Wearer::new(seed, 16 * 60);
        let mut skips = Rng(seed ^ 0x5DEE_CE66);
        let mut minute = NOON;
        let end = NOON + minutes;
        while minute < end {
            if skips.below(1000) < gap {
                minute += 1 + skips.below(3 * len as u64) as i64;
            }
            pair.record(minute, offset, wearer.next());
            assert_eq!(
                pair.watch.day(minute, offset),
                pair.everything.day(minute, offset),
                "the day at minute {minute} (seed {seed}, offset {offset}, {len} minutes)"
            );
            assert_eq!(
                pair.watch.last_sleep(),
                pair.everything.last_sleep(),
                "the last sleep at minute {minute} (seed {seed}, offset {offset}, {len} minutes)"
            );
            minute += 1;
        }
    }

    // ==========================================================================
    // The tests
    // ==========================================================================

    #[test]
    fn the_watch_shows_what_a_history_without_end_would() {
        // The firmware's history, in every kind of zone.
        const N: usize = (DAY as usize - minute_ring::MINUTES).div_ceil(60) + 1;
        for (seed, &offset) in OFFSETS.iter().enumerate() {
            agree::<N>(minute_ring::MINUTES, seed as u64 + 1, offset, 5 * DAY, 0);
        }
    }

    #[test]
    fn whatever_the_history_s_length() {
        // Short ones let go of nights still under way, and of most of the
        // day; an hour short of a day, of next to nothing.
        for seed in 1..=4 {
            let offset = OFFSETS[seed as usize % OFFSETS.len()];
            agree::<24>(60, seed, offset, 3 * DAY, 0);
            agree::<24>(100, seed, offset, 3 * DAY, 0);
            agree::<14>(11 * 60, seed, offset, 3 * DAY, 0);
            agree::<2>(23 * 60, seed, offset, 3 * DAY, 0);
            agree::<2>(DAY as usize - 1, seed, offset, 3 * DAY, 0);
        }
    }

    #[test]
    fn with_minutes_the_watch_did_not_see() {
        const N: usize = (DAY as usize - minute_ring::MINUTES).div_ceil(60) + 1;
        for seed in 1..=6 {
            let offset = OFFSETS[seed as usize % OFFSETS.len()];
            agree::<N>(minute_ring::MINUTES, seed, offset, 12 * DAY, 3);
            agree::<24>(100, seed, offset, 4 * DAY, 10);
        }
    }

    #[test]
    fn a_night_is_whole_in_the_afternoon_and_gone_a_day_after() {
        const N: usize = (DAY as usize - minute_ring::MINUTES).div_ceil(60) + 1;
        let mut watch = Watch::<N>::new(minute_ring::MINUTES);
        let asleep = |kind| Sample {
            kind: Some(kind),
            ..Sample::default()
        };
        let awake = Sample {
            steps: 3,
            kind: Some(Kind::Awake),
            ..Sample::default()
        };
        // Asleep from 23:00 until 07:00 (UTC, as NOON is), an hour of it
        // deep; then up.
        let (bed, up) = (NOON + 11 * 60, NOON + 19 * 60);
        for minute in NOON..NOON + 3 * DAY {
            let sample = match minute {
                m if (bed..bed + 60).contains(&m) => asleep(Kind::Deep),
                m if (bed..up).contains(&m) => asleep(Kind::Light),
                _ => awake,
            };
            *watch.at(minute, 0) = Some(sample);
            let night = Some((
                bed,
                up,
                Minutes {
                    light: 420,
                    deep: 60,
                    rem: 0,
                },
            ));
            let expected = match minute {
                // Under way: as far as it's got.
                m if m < up => continue,
                // The history lets go of its first minute at 13:30; it's
                // shown whole until a day after its last.
                m if m < up - 1 + DAY => night,
                _ => None,
            };
            assert_eq!(watch.last_sleep(), expected, "at minute {minute}");
        }
    }

    #[test]
    fn the_day_s_first_hours_stay_on_the_chart() {
        const N: usize = (DAY as usize - minute_ring::MINUTES).div_ceil(60) + 1;
        let mut watch = Watch::<N>::new(minute_ring::MINUTES);
        // A step a minute from midnight on, in Kathmandu (UTC+5:45).
        let offset = 345;
        let midnight = day::midnight(NOON, offset);
        for minute in midnight..midnight + DAY {
            *watch.at(minute, offset) = Some(Sample {
                steps: 1,
                heart_rate: 60 + ((minute - midnight) / 60) as u8,
                kind: Some(Kind::Awake),
            });
            let day = watch.day(minute, offset);
            let into = (minute - midnight) as usize;
            for (hour, sums) in day.hours.iter().enumerate() {
                let expected = (into + 1).saturating_sub(hour * 60).min(60) as u16;
                assert_eq!(sums.steps, expected, "hour {hour} at minute {into}");
                if expected > 0 {
                    assert_eq!(
                        (sums.heart_min, sums.heart_max, sums.heart_count),
                        (60 + hour as u8, 60 + hour as u8, expected as u8)
                    );
                }
            }
        }
    }

    #[test]
    fn a_flight_by_whole_hours_files_nothing_wrong() {
        // The zone changes every few hours, by whole hours (and between
        // two zones on the same half hour): every hour's sum is still an
        // hour of the new zone's.
        const N: usize = (DAY as usize - minute_ring::MINUTES).div_ceil(60) + 1;
        for (zones, seed) in [
            ([0i16, 60, -480, 540, 14 * 60], 1),
            ([330, 570, -210, 210, 630], 2),
        ] {
            let mut pair = Pair::<N>::new(minute_ring::MINUTES);
            let mut wearer = Wearer::new(seed, 10 * 60);
            let mut rng = Rng(seed);
            let mut offset = zones[0];
            for minute in NOON..NOON + 6 * DAY {
                if rng.below(400) == 0 {
                    offset = zones[rng.below(zones.len() as u64) as usize];
                }
                pair.record(minute, offset, wearer.next());
                assert_eq!(
                    pair.watch.day(minute, offset),
                    pair.everything.day(minute, offset),
                    "the day at minute {minute}, offset {offset}"
                );
            }
        }
    }

    #[test]
    fn a_flight_by_part_of_an_hour_is_out_by_an_hour_at_midnight_at_most() {
        // The sums from before straddle the new zone's hours: today's
        // total may miss what was in the hour after midnight, or count
        // what was in the hour before, and no more.
        const N: usize = (DAY as usize - minute_ring::MINUTES).div_ceil(60) + 1;
        for seed in 1..=6 {
            let mut pair = Pair::<N>::new(minute_ring::MINUTES);
            let mut wearer = Wearer::new(seed, 10 * 60);
            let mut rng = Rng(seed);
            let mut offset = 0;
            for minute in NOON..NOON + 6 * DAY {
                if rng.below(700) == 0 {
                    offset = OFFSETS[rng.below(OFFSETS.len() as u64) as usize];
                }
                pair.record(minute, offset, wearer.next());
                let midnight = day::midnight(minute, offset);
                let shown: u32 = pair
                    .watch
                    .day(minute, offset)
                    .hours
                    .iter()
                    .map(|hour| u32::from(hour.steps))
                    .sum();
                let least = pair
                    .everything
                    .steps(midnight + 60..=minute.max(midnight + 60));
                let most = pair.everything.steps(midnight - 60..=minute);
                assert!(
                    (least..=most).contains(&shown),
                    "{shown} steps at minute {minute}, offset {offset}: not in {least}..={most}"
                );
            }
        }
    }

    #[test]
    fn a_clock_put_back_a_little_changes_nothing_older() {
        // Back to a minute the history still held at its furthest (once
        // or in several goes): what it had let go of by then stays as it
        // was, and nothing is counted twice as the minutes come round
        // again.
        for seed in 1..=8 {
            let mut pair = Pair::<24>::new(100);
            let mut wearer = Wearer::new(seed, 3 * 60);
            let mut rng = Rng(seed);
            let (mut minute, mut furthest) = (NOON, NOON);
            for _ in 0..4 * DAY {
                minute = match rng.below(300) {
                    0 => (minute - rng.below(99) as i64).max(furthest - 99),
                    _ => minute + 1,
                };
                furthest = furthest.max(minute);
                pair.record(minute, 60, wearer.next());
                assert_eq!(
                    pair.watch.day(minute, 60),
                    pair.everything.day(minute, 60),
                    "the day at minute {minute} (seed {seed})"
                );
            }
        }
    }

    #[test]
    fn a_clock_put_back_a_long_way_shows_nothing_from_after_it() {
        // Back by more than the history holds: the sums of the hours from
        // the new time on are gone (the minutes were filed by a clock
        // that was ahead), with the whole of the hour it's in, and the
        // sleep if it reached that far. So the watch never shows more
        // than was recorded before the new time.
        for seed in 1..=8 {
            let mut pair = Pair::<24>::new(100);
            let mut wearer = Wearer::new(seed, 3 * 60);
            let mut rng = Rng(seed);
            let mut minute = NOON;
            for _ in 0..4 * DAY {
                minute += match rng.below(500) {
                    0 => -(100 + rng.below(2 * DAY as u64) as i64),
                    _ => 1,
                };
                pair.record(minute, 60, wearer.next());
                let (shown, recorded) =
                    (pair.watch.day(minute, 60), pair.everything.day(minute, 60));
                for (hour, (shown, recorded)) in shown.hours.iter().zip(recorded.hours).enumerate()
                {
                    assert!(
                        shown.steps <= recorded.steps && shown.heart_count <= recorded.heart_count,
                        "hour {hour} at minute {minute} (seed {seed}): {shown:?}, of {recorded:?}"
                    );
                }
                if let Some((start, end, _)) = pair.watch.last_sleep() {
                    assert!(start < end && end <= minute + 1, "a sleep after now");
                }
            }
        }
    }

    #[test]
    fn a_boot_puts_it_all_back() {
        // The flash's minutes filed oldest first, as `system_off::restore`
        // does (the log, then a record that repeats the log's last
        // minutes): the day and the last sleep are what they were.
        const N: usize = (DAY as usize - minute_ring::MINUTES).div_ceil(60) + 1;
        for seed in 1..=6 {
            let offset = OFFSETS[seed as usize % OFFSETS.len()];
            let mut pair = Pair::<N>::new(minute_ring::MINUTES);
            let mut wearer = Wearer::new(seed, 12 * 60);
            let last = NOON + 2 * DAY + 137 * seed as i64;
            for minute in NOON..=last {
                pair.record(minute, offset, wearer.next());
            }
            let saved: Vec<(i64, Sample)> = pair
                .everything
                .samples
                .iter()
                .map(|(&minute, &sample)| (minute, sample))
                .collect();
            let (log, record) = saved.split_at(saved.len() - 300);

            // After a planned reset: nothing filed yet.
            let mut booted = Watch::<N>::new(minute_ring::MINUTES);
            // After a crash: a heart rate reading got in first, in the
            // minute after the last one saved.
            let mut crashed = Watch::<N>::new(minute_ring::MINUTES);
            let reading = Sample {
                heart_rate: 70,
                ..Sample::default()
            };
            *crashed.at(last + 1, offset) = Some(reading);
            for watch in [&mut booted, &mut crashed] {
                for &(minute, sample) in log.iter().chain(&log[log.len() - 5..]).chain(record) {
                    // Twice over: a minute in two records counts once.
                    watch.restore(minute, offset, sample);
                    watch.restore(minute, offset, sample);
                }
            }
            assert_eq!(booted.day(last, offset), pair.watch.day(last, offset));
            assert_eq!(booted.last_sleep(), pair.watch.last_sleep());
            // And had the watch run on into that minute instead.
            *pair.watch.at(last + 1, offset) = Some(reading);
            assert_eq!(
                crashed.day(last + 1, offset),
                pair.watch.day(last + 1, offset)
            );
            assert_eq!(crashed.last_sleep(), pair.watch.last_sleep());
        }
    }

    #[test]
    fn it_starts_as_all_zeros() {
        // So the static it's in stays out of `.data`, whose initial
        // values take flash (5 KB of it, for the history).
        let aged = Aged::<11>::new();
        let bytes = unsafe {
            // SAFETY: plain integers, read as bytes for their size.
            std::slice::from_raw_parts((&raw const aged).cast::<u8>(), size_of::<Aged<11>>())
        };
        assert!(bytes.iter().all(|&byte| byte == 0));
        // And small: 8 bytes an hour, and 24 more.
        assert_eq!(size_of::<Aged<11>>(), 112);
    }
}
