//! The activity history as Gadgetbridge gets it: in 5-minute buckets.
//!
//! The watch keeps a sample a minute (see [`crate::activity`]), but
//! Gadgetbridge's Bangle.js support can't store samples that close
//! together. It files each `act` sample of a fetch through
//! `BangleJSSampleProvider.upsertSample`, which looks for samples it already
//! has within ±2 minutes (inclusive) of the new one and, if it finds any,
//! overwrites the nearest one's steps, heart rate and intensity (each only
//! if non-zero) instead of adding the new one. So of a run of samples a
//! minute apart, one in three survived, carrying the steps of the last
//! minute merged into it: a field day's 10,346 steps reached Health Connect
//! as 4,206. (The Bangle.js it was written for sends 10-minute records.)
//!
//! So a fetch sends one sample per UTC-aligned bucket of [`MINUTES`]
//! minutes, more than 2 apart, so that each lands on its own and sums lose
//! nothing. 3 is the shortest that does, and loses least: on a field day's
//! log, 3-minute buckets kept all 123 background heart rate readings and
//! 5-minute ones lost 5 of them (the schedule measures every 2–4 minutes
//! while active, see [`crate::heart_schedule`]); and on a night's
//! minute-level sleep stages, 3 changed 2–6% of minutes and 5 changed
//! 6–7% (stage totals within a couple of minutes either way). The user
//! chose 5 all the same (2026-11-10): rounder times, 288 samples a day
//! (a fifth of the minutes) rather than 480, and a reading every
//! 5 minutes is what the schedule takes asleep
//! ([`crate::heart_schedule::ASLEEP_INTERVAL`]), so nearly all of a
//! night's readings fit. Back to 3 is this constant alone.
//!
//! Per bucket:
//!
//! - **Steps** are summed, so totals are exact.
//! - **Heart rate**: the bucket's latest reading. A real reading, never an
//!   average of several; the latest is the one nearest the bucket's
//!   timestamp (its end, below). Earlier readings in the same bucket are
//!   dropped: readings less than 5 minutes apart, which the background
//!   schedule takes while active (5 of a field day's 123 were lost), and
//!   measuring continuously does (while the clock face is looked at, or
//!   Gadgetbridge's live activity is open). Both Gadgetbridge's charts and
//!   Health Connect then get at most a reading every 5 minutes. Asleep,
//!   the schedule times its sessions so that the bucket's one reading
//!   comes in its last minute, within a minute of the timestamp below
//!   (see [`crate::heart_schedule::in_phase`]).
//! - **Kind** (activity, a sleep stage, or exercise for a workout's
//!   minutes; see `activity::Sample::gadgetbridge_kind`): the one most of
//!   its minutes have, ties going to the later minute's. So a bucket a
//!   workout starts or ends in is exercise if three or more of its five
//!   minutes were (a walk is found three minutes in, and those three
//!   count; it ends three minutes after the steps stop, and those count
//!   too), and plain activity otherwise: a workout's edges are within
//!   two minutes of the truth, like a sleep's. Sleep sessions and
//!   stages in Gadgetbridge (and so in Health Connect) are then at 5-minute
//!   resolution, a little finer than the stages themselves (deep and REM
//!   bouts last at least 8 minutes, see [`crate::stages::MIN_BOUT`]); a
//!   restless minute or two among asleep ones is lost.
//! - **Movement** (`mov`, Gadgetbridge's intensity, which only its charts
//!   use): the mean over the minutes the accelerometer recorded, rounded up
//!   so that any movement stays non-zero.
//!
//! **The timestamp** is the bucket's last second (`hh:04:59` for a bucket
//! from `hh:00`). Gadgetbridge takes a sample as the end of the time it
//! describes: its time-in-each-activity tallies count from the previous
//! sample to it (`ActivityAnalysis`), its sleep stages run from the
//! previous sample to it (`SleepSessionSegmenter`), and its Health Connect
//! export writes a step count stamped `t` as the minute ending at `t`
//! rounded up to a whole minute (`AbstractActivitySampleSyncer`). But its
//! own daily totals take a day as midnight to 23:59:59 (`DailyTotals`), so
//! a sample stamped at midnight would count to the next day. The last
//! second is in the bucket for both: its steps land in its last minute in
//! Health Connect, and on its own day in either. (The minute samples this
//! replaced were stamped at their start, so their steps landed a minute
//! early in Health Connect.)
//!
//! **In Health Connect, steps are counted twice** where the phone counts
//! them too. Gadgetbridge's export writes every sample as a record one
//! minute long ([Gadgetbridge#7391]), so the bucket's last minute holds
//! all 5 minutes' steps. Health Connect settles overlapping records by
//! the sources' priority, interval by interval, so wherever the phone has
//! nothing in that minute, the watch's 5 minutes are added to what the
//! phone counted in the other four: a day the watch counted 17,110 steps
//! and the phone 13,699 came to 19,470. Nothing the watch sends can fix
//! that, as samples a minute apart are merged (above).
//!
//! Three more bugs in the same export touch what the watch sends:
//! - heart rates on a sync's boundary are written twice
//!   ([Gadgetbridge#7392]);
//! - two sleep sessions starting in the same UTC hour share an id, so
//!   Health Connect keeps only one ([Gadgetbridge#7393]); Gadgetbridge
//!   splits a night at any short wake with steps;
//! - all-day steps are marked as actively recorded ([Gadgetbridge#7394]).
//!
//! TODO: once #7391 is fixed (records spanning the sample's period),
//! check Health Connect's daily total against the watch's own.
//!
//! [Gadgetbridge#7391]: https://codeberg.org/Freeyourgadget/Gadgetbridge/issues/7391
//! [Gadgetbridge#7392]: https://codeberg.org/Freeyourgadget/Gadgetbridge/issues/7392
//! [Gadgetbridge#7393]: https://codeberg.org/Freeyourgadget/Gadgetbridge/issues/7393
//! [Gadgetbridge#7394]: https://codeberg.org/Freeyourgadget/Gadgetbridge/issues/7394
//!
//! **Where a fetch starts** ([`start`]): where the watch's last fetch
//! stopped, as the watch knows it ([`Fetch::synced_through`], kept across
//! a switch-off by `system_off`), so that what a hold kept back goes out;
//! and without that, after what Gadgetbridge may already have. Nothing it
//! has is sent again (except when it asks from earlier than the watch
//! thinks it has, which a fetch it never saw the end of can do: then from
//! there, in whole buckets as before).
//!
//! That matters because of what Gadgetbridge may hold: the sample a minute
//! that builds before 0.10.0-rc.4 sent, stamped at the minute's start,
//! about 3 minutes apart after its merging. A bucket sent over those would
//! overwrite the nearest one's steps with its own 5 minutes' total, while
//! the others in the same minutes keep theirs: counted twice (the first
//! fetch after the update to rc.4 resent ~2¾ hours over them, and Health
//! Connect showed 106 steps for the 64 the watch counted). Neither the
//! watch nor the fetch can tell whether Gadgetbridge holds those or
//! buckets, so the rule is safe for both:
//!
//! - Gadgetbridge asks from its own clock a minute before its last fetch
//!   ended, and that fetch sent at most the minute under way then: the one
//!   after the minute asked from, or with the watch's clock a little
//!   ahead of the phone's, the next ([`REACH`]). With no fetch of its own
//!   to go by, the watch starts at the first bucket after that, so no
//!   bucket shares a minute with a sample Gadgetbridge already has, nor
//!   lands within its 2 minutes (the nearest is 6 minutes off). That loses
//!   the minutes in between, up to 5, once: after an update from those
//!   builds, or after the watch restarts within a few minutes of a fetch
//!   (a crash; a switch-off keeps where it stopped).
//! - A fetch of its own since then says exactly where Gadgetbridge's copy
//!   ends, and that's never earlier than the first such fetch started, so
//!   never among the old samples.
//! - A phone that asks from its future (its clock was ahead at its last
//!   fetch, and has been put right since) says nothing of what it has. The
//!   watch sends from where it last stopped, if it knows; if not, nothing
//!   from before the bucket under way. That loses whatever it hadn't sent,
//!   once, but this takes the phone's clock being wrong at the last fetch
//!   before an update or a crash, and counting a day's steps twice would
//!   be worse.
//!
//! TODO: a phone whose last fetch never ended, as far as it's concerned
//! (the connection dropped before the end), asks from its fetch before
//! that, though it may have samples from it. Only a problem for the first
//! fetch after an update from the minute-sample builds; the watch can't
//! tell.
//!
//! **Re-fetching.** A bucket is only ever sent whole, so that sending it
//! again overwrites Gadgetbridge's copy with the same value, never a
//! smaller part of it:
//!
//! - A fetch starts at a bucket's start, not mid-bucket ([`Fetch::new`]).
//! - The bucket still under way isn't sent: it would go out partial, and
//!   while the next fetch's whole count would overwrite its steps,
//!   Gadgetbridge never overwrites a sample's kind. It goes out with the
//!   first fetch after it ends. So Gadgetbridge lags the watch by up to 5
//!   minutes.
//! - Nor is a bucket whose first minutes the watch has already dropped:
//!   Gadgetbridge may have it whole. (The history keeps 14½ hours, and
//!   the activity log in the SPI flash, which a fetch reads what's older
//!   from, 11 days: see `crate::spill`.)
//! - The log takes in whole buckets, and what the watch saves besides
//!   (before switching off, and for a crash) carries on from where the
//!   log ends (see `system_off`), so a restored history holds whole
//!   buckets.
//! - Nor is a bucket any of whose minutes' kinds may still change: a fetch
//!   in the night stops at the night's start, and the morning's sends it
//!   whole (see [`crate::sleep_hold`] and [`Fetch::hold`]).
//!
//! **A clock put right.** The phone sets the watch's clock to its own on
//! connecting, and when its own changes. A phone whose clock was ahead
//! (wrong, and since put right) has set the watch's ahead too, and its
//! fetches then took the watch's sync point ([`Fetch::synced_through`])
//! into what is, once the clock is put right, the future. Set back, the
//! clock files the minutes from the new time again: the history drops
//! those after it, and adds to its own (see [`crate::minute_ring`]). So
//! when the phone sets the clock, the sync point goes back to the minute
//! before at the latest ([`synced_after_set`]), and the next fetch sends
//! the bucket the clock was put right in, whole, and everything since.
//! Left where it was, the minutes from the new time on never reached the
//! phone, up to the old sync point or to the fetch after next, whichever
//! came first: the sync point claimed the phone had them, and a phone
//! asking from its future (as it does after its own clock was put right:
//! Gadgetbridge asks from its clock at its last fetch) got nothing before
//! the bucket under way, as [`start`] takes the earlier of the sync point
//! and after what the phone asked for.
//!
//! What Gadgetbridge already has at those times (the samples the watch
//! sent while its clock was ahead, of other minutes) is overwritten where
//! the new bucket's steps, heart rate or movement aren't 0, and its kind
//! may stay (see [`crate::sleep_hold`]): the watch can't take back what it
//! sent stamped wrong. A clock set back by seconds, as the phone does when
//! the watch's crystal has run fast, resends at most the last bucket the
//! watch sent, whole (with the steps of the seconds the clock took back,
//! if they were in the minute after: see `minute_ring::carried`).
//!
//! A clock set ahead needs nothing here: the sync point is behind it, and
//! the next fetch sends from there, the minutes before the jump at their
//! times. A jump far ahead (the watch was off for a day, say) has the
//! history let go of minutes at the next one the watch files (see
//! [`crate::minute_ring`]), and of those, the activity log may not have
//! the newest yet: the bucket that was under way, and what a fetch was
//! holding back. So the log takes them in first ([`loggable_end`]): by the
//! new time, the bucket is over, and what was held is older than any hold
//! lasts.
//!
//! TODO: a clock set back drops the history's minutes after the new time;
//! a bucket sent before that and again after it would overwrite
//! Gadgetbridge's steps with fewer (or more, the steps since). The phone
//! only corrects the clock by seconds, unless its own was wrong (and a
//! correction by seconds keeps its steps: see `minute_ring::carried`).

/// Minutes per bucket. Must be over 2 (see the module docs), and divide a
/// day, so buckets line up with midnight in every time zone (whose UTC
/// offsets are all whole quarter hours).
///
/// TODO: go back to one sample a minute (or at least shorter buckets) once
/// Gadgetbridge merges only samples much closer than 2 minutes apart:
/// <https://codeberg.org/Freeyourgadget/Gadgetbridge/issues/7295> (its
/// comments propose ~30 s). Sent a minute apart, two samples in three had
/// their steps overwritten: 4,206 of 10,346 steps reached Gadgetbridge.
pub const MINUTES: i64 = 5;

const _: () = assert!(MINUTES > 2 && 15 % MINUTES == 0);

/// The start of the bucket `minute` (UTC minutes since 1970) is in.
pub fn start_of(minute: i64) -> i64 {
    minute.div_euclid(MINUTES) * MINUTES
}

/// The first minute the activity log doesn't take in yet (see
/// `crate::spill`): the log holds what a fetch could send, whole buckets,
/// so this is where a fetch would stop.
///
/// Given the history's newest minute, the first minute a fetch holds
/// back as of it, if any (`sleep_hold::hold`), and the clock's minute
/// `now`. Those are the same minute but after a jump of the clock (see
/// the module docs' "A clock put right"), when the history hasn't filed
/// `now` yet:
///
/// - Running, it's the start of the bucket under way, or of the first
///   one held back.
/// - By `now`, nothing older than `cap` minutes (`sleep_hold::CAP`) is
///   held any more, as a fetch then sends it whatever its state; and if
///   that's all there is, the bucket that was under way is over too.
pub fn loggable_end(newest: i64, held_from: Option<i64>, now: i64, cap: i64) -> i64 {
    let running = match held_from {
        Some(from) => start_of(newest).min(start_of(from)),
        None => start_of(newest),
    };
    let capped = start_of(now - cap + 1).min(start_of(newest) + MINUTES);
    running.max(capped)
}

/// How many minutes past the one the phone asks from its copy may reach:
/// it asks from a minute before its last fetch ended (by its clock), which
/// sent up to the minute under way then (by the watch's), so the next
/// minute, or with the watch's clock up to a minute ahead, the one after.
pub const REACH: i64 = 2;

/// Where a fetch starts (see the module docs), at `now`, for a phone that
/// asked from `asked_ms` (UTC milliseconds, its `ts`), given where its
/// copy ends as far as the watch knows (`synced`, the last minute of it,
/// from [`Fetch::synced_through`]; minutes are UTC minutes since 1970):
/// where that ends, or if the phone asks from earlier (or the watch
/// doesn't know), the first bucket after all the phone may have.
pub fn start(asked_ms: i64, synced: Option<i64>, now: i64) -> i64 {
    // A phone asking from its future may have anything up to now.
    let reached = asked_ms.div_euclid(60_000).saturating_add(REACH).min(now);
    let after = start_of(reached) + MINUTES;
    synced.map_or(after, |synced| after.min(synced + 1))
}

/// Where the phone's copy ends (`synced`, as for [`start`]) once the phone
/// has set the clock to `now` (a UTC minute): no later than the minute
/// before. See the module docs' "A clock put right".
pub fn synced_after_set(synced: Option<i64>, now: i64) -> Option<i64> {
    synced.map(|synced| synced.min(now - 1))
}

/// One minute of the history, as a bucket takes it in.
#[derive(Clone, Copy, Debug)]
pub struct Minute<K> {
    pub steps: u16,
    pub movement: u16,
    /// 0 if not measured.
    pub heart_rate: u8,
    /// Whether the accelerometer recorded the minute (else `movement`
    /// means nothing).
    pub seen: bool,
    /// What Gadgetbridge is told the minute was (see the module docs).
    pub kind: K,
}

/// One bucket: what one `act` sample carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bucket<K> {
    /// Its first minute (UTC minutes since 1970).
    pub start: i64,
    pub steps: u32,
    pub movement: u16,
    pub heart_rate: u8,
    pub kind: K,
}

impl<K> Bucket<K> {
    /// The timestamp to send, in UTC milliseconds: its last second (see
    /// the module docs).
    pub fn timestamp_ms(&self) -> i64 {
        (self.start + MINUTES) * 60_000 - 1_000
    }
}

/// The buckets one fetch sends, in order.
pub struct Fetch {
    /// Where it starts.
    start: i64,
    /// Where the next bucket starts, at the earliest.
    next: i64,
    /// The start of the bucket under way: none from here on.
    end: i64,
}

impl Fetch {
    /// A fetch of the minutes from `since` on (see [`start`]), at `now`,
    /// of a history whose oldest minute is `oldest` (all UTC minutes since
    /// 1970): from the start of `since`'s bucket, or of the first bucket
    /// the history has whole if that's later, to the end of the last
    /// bucket that's over.
    ///
    /// A history whose oldest minute is after the bucket under way is one
    /// the clock went back on, further than it reaches, and that hasn't
    /// filed a minute since (which drops all of it, see
    /// `crate::minute_ring`): nothing to send, and nothing it says about
    /// where the phone's copy ends. Starting at its oldest minute would
    /// put that among the minutes still to come, where [`start`] can't
    /// use it (see the module docs' "A clock put right").
    pub fn new(since: i64, oldest: i64, now: i64) -> Self {
        let end = start_of(now);
        let start = start_of(since).max(start_of(oldest + MINUTES - 1).min(end));
        Self {
            start,
            next: start,
            end,
        }
    }

    /// Holds back the minutes from `from` on (see [`crate::sleep_hold`]):
    /// the fetch ends at the start of the bucket `from` is in, if that's
    /// sooner, so that a bucket is sent whole once all of it is settled.
    pub fn hold(&mut self, from: i64) {
        self.end = self.end.min(start_of(from));
    }

    /// The phone set the clock to `now` (a UTC minute) while the fetch was
    /// under way (its writes are served between buckets). The buckets from
    /// the one `now` is in were filed by a clock that was ahead, and the
    /// history files those minutes again (see the module docs' "A clock
    /// put right"): so the fetch ends there, if that's sooner, and claims
    /// no more for the phone's copy than what's before it, even if it
    /// started later ([`Fetch::synced_through`]). The next fetch sends
    /// that bucket whole, and everything since.
    ///
    /// Without this, the fetch's end would take the sync point back past
    /// where [`synced_after_set`] had just put it. (When a fetch kept the
    /// phone's writes waiting until it was done, the clock was only set
    /// after the sync point had moved, and that put it right.)
    ///
    /// A clock set to the bucket under way or later, as nearly all are,
    /// changes nothing: the fetch still ends where it was going to.
    pub fn clock_set(&mut self, now: i64) {
        self.end = self.end.min(start_of(now));
        self.start = self.start.min(self.end);
        self.next = self.next.min(self.end);
    }

    /// Where the fetch ends: the start of the first bucket it doesn't
    /// send.
    pub fn end(&self) -> i64 {
        self.end
    }

    /// Where to read the history from for [`Fetch::next`].
    pub fn from(&self) -> i64 {
        self.next
    }

    /// The last minute of the phone's copy once all the buckets are sent,
    /// for the next fetch's [`start`]: the minute before the bucket under
    /// way, or the first one held back, or where the fetch started, if
    /// that's later. What's before the start the phone has (or, after an
    /// update from minute samples, may have; see the module docs), even if
    /// a hold ends the fetch before it: a hold from earlier only holds back
    /// what went out before it began.
    pub fn synced_through(&self) -> i64 {
        self.end.max(self.start) - 1
    }

    /// The next bucket, from `minutes`: the history from [`Fetch::from`]
    /// on, oldest first, skipping minutes without data (as
    /// `activity::History::since` gives them). Reads only as far as the
    /// bucket's last minute (and one past it). `None` once the buckets
    /// that are over have all been sent.
    pub fn next<K: Copy + PartialEq>(
        &mut self,
        minutes: impl IntoIterator<Item = (i64, Minute<K>)>,
    ) -> Option<Bucket<K>> {
        let from = self.next;
        let mut minutes = minutes
            .into_iter()
            .skip_while(|&(minute, _)| minute < from)
            .peekable();
        let start = start_of(minutes.peek()?.0);
        if start >= self.end {
            self.next = self.end;
            return None;
        }
        self.next = start + MINUTES;

        let mut bucket = Bucket {
            start,
            steps: 0,
            movement: 0,
            heart_rate: 0,
            kind: minutes.peek()?.1.kind,
        };
        // Each minute's kind by its place in the bucket, for the vote.
        let mut kinds = [None; MINUTES as usize];
        let (mut movement, mut seen) = (0u32, 0u32);
        while let Some((minute, sample)) = minutes.next_if(|&(minute, _)| minute < start + MINUTES)
        {
            bucket.steps += u32::from(sample.steps);
            if sample.heart_rate != 0 {
                bucket.heart_rate = sample.heart_rate;
            }
            if sample.seen {
                movement += u32::from(sample.movement);
                seen += 1;
            }
            kinds[(minute - start) as usize] = Some(sample.kind);
        }
        if seen > 0 {
            bucket.movement = movement.div_ceil(seen) as u16;
        }
        // The kind most minutes have; from the latest minute back, so that
        // ties go to the later one.
        let votes = |kind| kinds.iter().filter(|&&k| k == Some(kind)).count();
        let mut most = 0;
        for kind in kinds.iter().rev().flatten() {
            if votes(*kind) > most {
                most = votes(*kind);
                bucket.kind = *kind;
            }
        }
        Some(bucket)
    }
}
