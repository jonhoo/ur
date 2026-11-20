//! Host tests for the firmware's `heart_schedule` module. `cargo test`
//! runs them.
//!
//! Most drive the schedule the way `heart::heart_task` does, through a
//! simulated wrist (see [`simulate`]): the accelerometer polls every 2 s,
//! sessions start once due and calm, and each one's outcome and length
//! come from the scenario.

#[path = "../../../src/heart_schedule.rs"]
#[allow(dead_code)]
mod heart_schedule;

fn main() {
    println!("run `cargo test`");
}

#[cfg(test)]
mod tests {
    use super::heart_schedule::*;

    const MINUTE: u32 = 60;
    const HOUR: u32 = 60 * MINUTE;
    /// How often the accelerometer polls, as in `activity`.
    const POLL: u32 = 2;

    /// The simulated wrist at one poll.
    #[derive(Clone, Copy)]
    struct Moment {
        /// Whether this poll saw steps.
        active: bool,
        /// Whether it saw vigorous movement without steps (gestures at a
        /// desk, say): active for the schedule, but not stepping.
        fidget: bool,
        /// Whether the wrist is calm enough to measure through.
        calm: bool,
        asleep: bool,
        /// How a session started now would end, and how long it'd take.
        outcome: Outcome,
        seconds: u32,
    }

    /// A still, awake wrist that reads 60 BPM in 13 s, like the field log's
    /// typical session.
    fn resting(_: u32) -> Moment {
        Moment {
            active: false,
            fidget: false,
            calm: true,
            asleep: false,
            outcome: Outcome::Reading(60),
            seconds: 13,
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct Session {
        start: u32,
        end: u32,
        why: Why,
        /// Whether it started on a calm wrist or a busy one.
        how: Start,
    }

    /// Runs `schedule` through `world` for `until` seconds, as the heart
    /// task would: background sessions only, each started once due and
    /// [`Schedule::start`] says so (the wrist calm for
    /// [`START_CALM_POLLS`] polls, or [`CALM_WAIT`] after it was first
    /// found due). The accelerometer's counts start
    /// afresh, as at boot, except that the wearer is taken to have taken
    /// no steps for a while.
    fn simulate(
        schedule: &mut Schedule,
        from: u32,
        until: u32,
        world: impl Fn(u32) -> Moment,
    ) -> Vec<Session> {
        simulate_in_phase(schedule, from, until, None, world)
    }

    /// [`simulate`], on a watch that knows the time: `bucket` is when one
    /// of the phone's buckets starts (see `Schedule::due_in_phase`).
    fn simulate_in_phase(
        schedule: &mut Schedule,
        from: u32,
        until: u32,
        bucket: Option<u32>,
        world: impl Fn(u32) -> Moment,
    ) -> Vec<Session> {
        let mut sessions = Vec::new();
        let mut polls = 0u8;
        let mut calm = 0;
        let mut stepless = RESTING_STEPLESS_POLLS;
        let mut due_since = None;
        let mut t = from;
        while t < until {
            let moment = world(t);
            if moment.active || moment.fidget {
                polls = polls.saturating_add(1);
            }
            let run = |n: u8, counts: bool| if counts { n.saturating_add(1) } else { 0 };
            calm = run(calm, moment.calm);
            stepless = run(stepless, !moment.active);
            let (due, why) = schedule.due_in_phase(polls, moment.asleep, bucket);
            let mut why = why;
            let how = if t < due {
                due_since = None;
                // Not due, but a late reading takes a calm moment (see
                // `Schedule::seek`), as in `heart`.
                let seeking = schedule.seek(moment.asleep).is_some_and(|from| t >= from);
                if seeking && calm >= START_CALM_POLLS {
                    why = Why::Calm;
                    Start::Calm
                } else {
                    Start::Wait
                }
            } else {
                schedule.start(t, *due_since.get_or_insert(t), calm)
            };
            if how != Start::Wait {
                due_since = None;
                let end = t + moment.seconds;
                let before = Before {
                    active_polls: polls,
                    stepless_polls: stepless,
                    asleep: moment.asleep,
                };
                schedule.ended(end, moment.seconds, moment.outcome, true, before);
                polls = 0;
                sessions.push(Session {
                    start: t,
                    end,
                    why,
                    how,
                });
                t = end;
            }
            t += POLL;
        }
        sessions
    }

    /// The wait before each session after the first, from the end of the
    /// one before.
    fn gaps(sessions: &[Session]) -> Vec<u32> {
        sessions.windows(2).map(|w| w[1].start - w[0].end).collect()
    }

    /// LED seconds spent by sessions starting in `from..to`.
    fn led(sessions: &[Session], from: u32, to: u32) -> u32 {
        sessions
            .iter()
            .filter(|s| (from..to).contains(&s.start))
            .map(|s| s.end - s.start)
            .sum()
    }

    /// Rounds a simulated wait to the poll it fell on.
    fn about(wait: u32, expected: u32) -> bool {
        wait.abs_diff(expected) < POLL
    }

    /// A session's lead-up: awake, with `active_polls` since the last, but
    /// no steps for long.
    fn awake(active_polls: u8) -> Before {
        Before {
            active_polls,
            stepless_polls: u8::MAX,
            asleep: false,
        }
    }

    /// Asleep, likewise.
    fn sleeping() -> Before {
        Before {
            asleep: true,
            ..awake(0)
        }
    }

    #[test]
    fn the_state_stays_small() {
        // It lives in the heart task's static future, where RAM is tight.
        // (20 bytes until counting interruptions took one more, and
        // alignment three more; 28 with when the last reading came, 32
        // with the LED experiment's rounds, and no fewer for dropping
        // the interruptions again; the charger's byte fits in what was
        // padding.)
        assert!(size_of::<Schedule>() <= 32);
    }

    #[test]
    fn the_first_session_is_right_away() {
        let schedule = Schedule::new();
        assert_eq!(schedule.due(0, false), (0, Why::First));
        assert_eq!(schedule.due(200, true), (0, Why::First));
    }

    #[test]
    fn rest_backs_off_to_the_cap() {
        let mut schedule = Schedule::new();
        let sessions = simulate(&mut schedule, 0, 6 * HOUR, resting);
        let waits = gaps(&sessions);
        // Doubling from the minimum, then held at the cap.
        for (wait, expected) in waits.iter().zip([4, 8, 16, 20, 20, 20]) {
            assert!(about(*wait, expected * MINUTE), "{waits:?}");
        }
        assert!(waits.windows(2).all(|w| w[1] + POLL >= w[0]), "{waits:?}");
        assert!(waits[3..].iter().all(|&w| about(w, MAX_INTERVAL)));
        assert!(sessions[1..].iter().all(|s| s.why == Why::Rest));
        // About three sessions an hour once settled: ~40 s of LED, where
        // the fixed 10-minute schedule spent ~77.
        let settled = led(&sessions, 2 * HOUR, 6 * HOUR) / 4;
        assert!((26..=52).contains(&settled), "{settled} s an hour");
        assert_eq!(schedule.resting(), (60, 0));
    }

    #[test]
    fn movement_resets_the_back_off() {
        let mut schedule = Schedule::new();
        let mut sessions = simulate(&mut schedule, 0, 2 * HOUR, resting);
        let before = sessions.len();
        // A short walk: 30 s of steps, then sitting down again.
        let walk = 2 * HOUR + 5 * MINUTE;
        sessions.extend(simulate(&mut schedule, 2 * HOUR, 4 * HOUR, |t| Moment {
            active: (walk..walk + 30).contains(&t),
            calm: !(walk..walk + 30).contains(&t),
            ..resting(t)
        }));
        let after = &sessions[before..];
        // Measured at the first calm moment after the walk (the last
        // session was well over the minimum interval before), then backing
        // off again from the minimum.
        assert_eq!(after[0].why, Why::Moved);
        let settled = walk + 30 + u32::from(START_CALM_POLLS) * POLL;
        assert!(
            after[0].start >= walk + 30 && after[0].start < settled,
            "{after:?}"
        );
        let waits = gaps(after);
        for (wait, expected) in waits.iter().zip([4, 8, 16, 20]) {
            assert!(about(*wait, expected * MINUTE), "{waits:?}");
        }
    }

    #[test]
    fn movement_soon_after_a_session_waits_for_the_minimum_and_calm() {
        let mut schedule = Schedule::new();
        let sessions = simulate(&mut schedule, 0, HOUR, resting);
        let last = sessions.last().expect("an hour at rest measures").end;
        // Walking from right after the session, for three minutes.
        let walking = |t: u32| (last..last + 3 * MINUTE).contains(&t);
        let after = simulate(&mut schedule, last + POLL, last + HOUR, |t| Moment {
            active: walking(t),
            calm: !walking(t),
            ..resting(t)
        });
        // Due after two minutes, but the wrist is busy until three, inside
        // `CALM_WAIT`: it starts calm.
        assert_eq!((after[0].why, after[0].how), (Why::Moved, Start::Calm));
        assert!(after[0].start >= last + 3 * MINUTE);
        let settled = last + 3 * MINUTE + u32::from(START_CALM_POLLS) * POLL;
        assert!(after[0].start < settled, "{after:?}");

        // And with the walk already over by then, at the minimum.
        let mut schedule = Schedule::new();
        let sessions = simulate(&mut schedule, 0, HOUR, resting);
        let last = sessions.last().expect("an hour at rest measures").end;
        let walking = |t: u32| (last..last + 30).contains(&t);
        let after = simulate(&mut schedule, last + POLL, last + HOUR, |t| Moment {
            active: walking(t),
            calm: !walking(t),
            ..resting(t)
        });
        assert!(about(after[0].start - last, MIN_INTERVAL), "{after:?}");
    }

    #[test]
    fn a_flick_of_the_wrist_is_not_movement() {
        let mut schedule = Schedule::new();
        simulate(&mut schedule, 0, 2 * HOUR, resting);
        let (at, why) = schedule.due(MOVED_POLLS - 1, false);
        assert_eq!(why, Why::Rest);
        assert_eq!(
            schedule.due(MOVED_POLLS, false),
            (at - MAX_INTERVAL + MIN_INTERVAL, Why::Moved)
        );
    }

    #[test]
    fn movement_asleep_is_turning_over() {
        let mut schedule = Schedule::new();
        let asleep = |t: u32| Moment {
            asleep: true,
            outcome: Outcome::Reading(50),
            ..resting(t)
        };
        simulate(&mut schedule, 0, 2 * HOUR, asleep);
        assert_eq!(schedule.due(50, true).1, Why::Asleep);
        // Readings asleep teach the asleep estimate, not the awake one.
        assert_eq!(schedule.resting(), (0, 50));
        // Tossing and turning all night still measures at the asleep
        // interval, not the movement one.
        let sessions = simulate(&mut schedule, 2 * HOUR, 8 * HOUR, |t| Moment {
            active: t % 60 < 15,
            calm: t % 60 >= 15,
            ..asleep(t)
        });
        assert!(
            gaps(&sessions)
                .iter()
                .all(|&w| (ASLEEP_INTERVAL..ASLEEP_INTERVAL + 60).contains(&w)),
            "{:?}",
            gaps(&sessions)
        );
    }

    /// Asleep, readings come every [`ASLEEP_INTERVAL`] (for sleep staging),
    /// within the LED budget; falling asleep mid back-off doesn't wait out
    /// the long interval, and waking returns to the back-off.
    #[test]
    fn asleep_measures_every_five_minutes() {
        let mut schedule = Schedule::new();
        simulate(&mut schedule, 0, 2 * HOUR, resting);
        assert_eq!(schedule.due(0, false).1, Why::Rest);
        let last_awake = schedule.due(0, false).0 - MAX_INTERVAL;
        // Asleep now: due 5 minutes after the last reading, not 20.
        assert_eq!(
            schedule.due(0, true),
            (last_awake + ASLEEP_INTERVAL, Why::Asleep)
        );
        let night = |t: u32| Moment {
            asleep: true,
            outcome: Outcome::Reading(50),
            ..resting(t)
        };
        let sessions = simulate(&mut schedule, 2 * HOUR, 10 * HOUR, night);
        assert!(
            gaps(&sessions).iter().all(|&w| about(w, ASLEEP_INTERVAL)),
            "{:?}",
            gaps(&sessions)
        );
        // About 11 sessions of 13 s an hour: inside the budget.
        let per_hour = led(&sessions, 3 * HOUR, 9 * HOUR) / 6;
        assert!((130..=160).contains(&per_hour), "{per_hour} s/h");
        // Awake again: back to backing off.
        let morning = simulate(&mut schedule, 10 * HOUR, 12 * HOUR, resting);
        assert!(gaps(&morning).iter().any(|&w| about(w, MAX_INTERVAL)));
    }

    /// How far into its bucket `t` is, with a bucket starting at `bucket`.
    fn into_bucket(t: u32, bucket: u32) -> u32 {
        (t + BUCKET - bucket % BUCKET) % BUCKET
    }

    /// Whether `t` is in a bucket's slot for an asleep session's start.
    fn in_slot(t: u32, bucket: u32) -> bool {
        (BUCKET_SLOT..BUCKET_SLOT + BUCKET_SLOT_FOR).contains(&into_bucket(t, bucket))
    }

    #[test]
    fn the_slot_is_just_into_a_buckets_last_minute() {
        // Buckets from 0: the slot is 245..265 s into each, clear of the
        // minute's start (`MINUTE_CLEAR`).
        assert_eq!(in_phase(0, 0), 245);
        assert_eq!(in_phase(240, 0), 245);
        assert_eq!(in_phase(244, 0), 245);
        assert_eq!(in_phase(245, 0), 245);
        assert_eq!(in_phase(264, 0), 264);
        assert_eq!(in_phase(265, 0), 545);
        assert_eq!(in_phase(299, 0), 545);
        assert_eq!(in_phase(300, 0), 545);
        // Buckets from 37 s after boot, or from any other bucket's start,
        // one long before boot included.
        for bucket in [37, 337, 37 + 1000 * BUCKET] {
            assert_eq!(in_phase(10, bucket), 282);
            assert_eq!(in_phase(282, bucket), 282);
            assert_eq!(in_phase(302, bucket), 582);
            // The bucket that began before boot: its slot comes first.
            assert_eq!(in_phase(0, bucket + 100), 82);
        }
    }

    /// Asleep, on a watch that knows the time, sessions keep step with the
    /// phone's buckets: after the first, each starts as its bucket's last
    /// minute begins, so every bucket gets one reading, in that minute,
    /// five minutes after the last. (Without the time, they come five
    /// minutes after the last one *ended*, and drift through the buckets:
    /// see `asleep_measures_every_five_minutes`.)
    #[test]
    fn asleep_sessions_keep_step_with_the_buckets() {
        let night = |t: u32| Moment {
            asleep: true,
            outcome: Outcome::Reading(50),
            seconds: 11,
            ..resting(t)
        };
        for bucket in [0, 76, 250, 299] {
            let mut schedule = Schedule::new();
            let sessions = simulate_in_phase(&mut schedule, 0, 8 * HOUR, Some(bucket), night);
            // The first is at boot (and three calm polls), wherever that
            // falls.
            assert!(sessions[0].start <= 3 * POLL);
            assert!(sessions.len() >= 8 * 12 - 1, "{}", sessions.len());
            for pair in sessions[1..].windows(2) {
                assert!(about(pair[1].start - pair[0].start, BUCKET), "{pair:?}");
            }
            for session in &sessions[1..] {
                // At the slot's start, give or take the poll it's seen at,
                // and read well inside the bucket.
                assert!(into_bucket(session.start, bucket) - BUCKET_SLOT < POLL);
                assert!(into_bucket(session.end, bucket) < BUCKET - 30);
                assert_eq!(session.why, Why::Asleep);
            }
        }
    }

    /// A night with its troubles: turning over every 47 minutes (a busy
    /// wrist for three, so a session due then waits its `CALM_WAIT` and
    /// runs late), and a session without a reading now and then. Sessions
    /// fall back into step at once, each is due no more than 6 minutes 40
    /// after the last (sleep staging compares readings up to 7 apart; the
    /// wait for calm comes on top, as it always did), none comes within
    /// two minutes of the last, and there are no fewer than a night of
    /// drifting ones has.
    #[test]
    fn a_restless_night_falls_back_into_step() {
        let night = |t: u32| Moment {
            asleep: true,
            calm: t % (47 * MINUTE) >= 3 * MINUTE,
            outcome: if (t / 300) % 9 == 4 {
                Outcome::NoReading
            } else {
                Outcome::Reading(50)
            },
            seconds: if (t / 300) % 9 == 4 { 30 } else { 11 },
            ..resting(t)
        };
        let bucket = 131;
        let mut schedule = Schedule::new();
        let sessions = simulate_in_phase(&mut schedule, 0, 8 * HOUR, Some(bucket), night);
        let waits = gaps(&sessions);
        assert!(waits.iter().all(|&w| w >= MIN_INTERVAL), "{waits:?}");
        for (wait, session) in waits.iter().zip(&sessions[1..]) {
            let waited = if in_slot(session.start, bucket) {
                0
            } else {
                CALM_WAIT + POLL
            };
            assert!(*wait <= 400 + waited, "{waits:?}");
        }
        let late: Vec<_> = sessions[1..]
            .iter()
            .filter(|s| !in_slot(s.start, bucket))
            .collect();
        // Only the ones that had to wait for calm start out of the slot
        // (one each time the wearer turns over, at most), and the one
        // after each is back in it.
        assert!(!late.is_empty() && late.len() <= 11, "{late:?}");
        assert!(late.iter().any(|s| s.how == Start::Busy), "{late:?}");
        for pair in sessions[1..].windows(2) {
            assert!(
                in_slot(pair[0].start, bucket) || in_slot(pair[1].start, bucket),
                "{pair:?}"
            );
        }
        // Against the same night without the time.
        let mut drifting = Schedule::new();
        let unphased = simulate(&mut drifting, 0, 8 * HOUR, night);
        assert!(sessions.len() >= unphased.len(), "{}", sessions.len());
        assert!(sessions.len() <= unphased.len() + 6, "{}", sessions.len());
    }

    /// Only asleep, and only with the time: awake, a session is due when
    /// it would be anyway, whatever the buckets, as is the first after
    /// boot; and the budget still holds an asleep one back.
    #[test]
    fn only_asleep_sessions_are_timed_to_the_buckets() {
        assert_eq!(
            Schedule::new().due_in_phase(0, true, Some(7)),
            (0, Why::First)
        );
        let mut schedule = Schedule::new();
        schedule.ended(1000, 11, Outcome::Reading(50), true, sleeping());
        for bucket in [0, 99, 250] {
            for polls in [0, MOVED_POLLS] {
                assert_eq!(
                    schedule.due_in_phase(polls, false, Some(bucket)),
                    schedule.due(polls, false)
                );
            }
            assert_eq!(schedule.due_in_phase(0, true, None), schedule.due(0, true));
        }
        // Asleep: 1000 + 300 would be 100 s into a bucket from 0, so the
        // slot before it, at 1145; a bucket from 250 has its slot at 1095,
        // too soon after the last session, so the next, at 1395.
        assert_eq!(schedule.due_in_phase(0, true, Some(0)), (1145, Why::Asleep));
        assert_eq!(
            schedule.due_in_phase(0, true, Some(250)),
            (1395, Why::Asleep)
        );
        // A slot under way once two minutes have passed: then.
        assert_eq!(
            schedule.due_in_phase(0, true, Some(275)),
            (1120, Why::Asleep)
        );
        // A failure's retry is in step too, and an emptied budget holds
        // the session back past its slot.
        schedule.ended(1170, 30, Outcome::NoReading, true, sleeping());
        assert_eq!(schedule.due_in_phase(0, true, Some(0)), (1445, Why::Retry));
        schedule.ended(1200, 400, Outcome::NoReading, false, sleeping());
        let (at, why) = schedule.due_in_phase(0, true, Some(0));
        assert_eq!(why, Why::Budget);
        assert_eq!((at, why), schedule.due(0, true));
    }

    #[test]
    fn elevated_readings_keep_the_interval_short() {
        let mut schedule = Schedule::new();
        simulate(&mut schedule, 0, 3 * HOUR, resting);
        assert_eq!(schedule.resting().0, 60);
        // Sitting still but with the heart rate up (after a run, say),
        // coming down 1 BPM a minute from 105.
        let up = 3 * HOUR;
        let sessions = simulate(&mut schedule, up, 5 * HOUR, |t| {
            let bpm = 105 - ((t - up) / MINUTE).min(45) as u8;
            Moment {
                outcome: Outcome::Reading(bpm),
                ..resting(t)
            }
        });
        // The first is at the old resting interval: nothing said to look
        // sooner. Then every few minutes while the readings are 15 over
        // resting, then backing off again.
        let elevated: Vec<_> = sessions.iter().filter(|s| s.why == Why::Elevated).collect();
        assert!(elevated.len() >= 3, "{sessions:?}");
        let waits = gaps(&sessions);
        assert!(
            waits
                .iter()
                .take(elevated.len())
                .all(|&w| about(w, ELEVATED_INTERVAL)),
            "{waits:?}"
        );
        assert!(sessions.last().expect("measured").why == Why::Rest);
        // The resting estimate rose by a beat for each elevated reading,
        // and came back down after.
        assert!(schedule.resting().0 <= 62, "{:?}", schedule.resting());
    }

    #[test]
    fn asleep_readings_compare_against_the_asleep_estimate() {
        let mut schedule = Schedule::new();
        simulate(&mut schedule, 0, 3 * HOUR, resting);
        let asleep = |bpm| {
            move |t| Moment {
                asleep: true,
                outcome: Outcome::Reading(bpm),
                ..resting(t)
            }
        };
        // The first nights' readings, before an asleep estimate exists, go
        // against the awake one: sleep only lowers the heart rate.
        simulate(&mut schedule, 3 * HOUR, 9 * HOUR, asleep(48));
        assert_eq!(schedule.resting(), (60, 48));
        // 66 is ordinary awake, but well up asleep (a nightmare, say).
        let mut woken = schedule.clone();
        schedule.ended(9 * HOUR, 13, Outcome::Reading(66), true, sleeping());
        assert_eq!(schedule.due(0, true).1, Why::Elevated);
        woken.ended(9 * HOUR, 13, Outcome::Reading(66), true, awake(0));
        assert_eq!(woken.due(0, false).1, Why::Rest);
    }

    #[test]
    fn the_resting_estimate_tracks_the_median_and_falls_quickly() {
        let mut schedule = Schedule::new();
        // Booted mid-walk: the first reading is far above rest.
        let mut t = 0;
        let mut read = |schedule: &mut Schedule, bpm| {
            t += 20 * MINUTE;
            schedule.ended(t, 13, Outcome::Reading(bpm), true, awake(0));
        };
        read(&mut schedule, 100);
        // It's down near 60 within a few resting readings.
        for _ in 0..12 {
            read(&mut schedule, 60);
        }
        assert!(schedule.resting().0 <= 62, "{:?}", schedule.resting());
        // A run of scattered resting readings: it settles near their median,
        // and an occasional high one barely moves it.
        for bpm in [58, 61, 59, 60, 62, 85, 60, 59, 61, 60, 90, 60, 58, 61, 60] {
            read(&mut schedule, bpm);
        }
        assert!(
            (58..=62).contains(&schedule.resting().0),
            "{:?}",
            schedule.resting()
        );
    }

    #[test]
    fn readings_soon_after_steps_dont_teach_the_resting_estimate() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        schedule.ended(400, 13, Outcome::Reading(61), true, awake(0));
        assert_eq!(schedule.resting().0, 61);
        // Two minutes after a walk.
        let walked = Before {
            active_polls: MOVED_POLLS,
            stepless_polls: 60,
            asleep: false,
        };
        schedule.ended(700, 13, Outcome::Reading(110), true, walked);
        assert_eq!(schedule.resting().0, 61);
        assert_eq!(schedule.due(0, false).1, Why::Elevated);
    }

    /// At a desk, the hands move vigorously every minute or two, but the
    /// wearer is at rest: readings there teach the resting estimate (which
    /// stayed empty for a whole afternoon when any movement since the last
    /// session disqualified them), whether looked at or not.
    #[test]
    fn readings_at_a_desk_are_resting() {
        let mut schedule = Schedule::new();
        let desk = |t: u32| Moment {
            fidget: t % 120 < 10,
            calm: t % 120 >= 10,
            outcome: Outcome::Reading(66),
            ..resting(t)
        };
        simulate(&mut schedule, 0, 2 * HOUR, desk);
        assert_eq!(schedule.resting(), (66, 0));
        // A reading while looking at the watch counts too.
        let mut schedule = Schedule::new();
        schedule.ended(100, 30, Outcome::Reading(64), false, awake(MOVED_POLLS));
        schedule.ended(400, 30, Outcome::Reading(66), false, awake(MOVED_POLLS));
        assert_eq!(schedule.resting(), (65, 0));
    }

    /// One reading doesn't make an estimate: it starts once two agree.
    /// Otherwise a single misreading (a field log's desk afternoon had a 42
    /// against a Garmin's 67) would make every ordinary reading after it
    /// count as elevated for ~20 readings, measuring every four minutes.
    #[test]
    fn the_resting_estimate_starts_from_two_agreeing_readings() {
        let mut schedule = Schedule::new();
        let mut t = 0;
        let mut read = |schedule: &mut Schedule, bpm| {
            t += 10 * MINUTE;
            schedule.ended(t, 13, Outcome::Reading(bpm), true, awake(0));
        };
        read(&mut schedule, 42);
        assert_eq!(schedule.resting(), (0, 0));
        read(&mut schedule, 67);
        assert_eq!(schedule.resting(), (0, 0));
        assert_eq!(schedule.due(0, false).1, Why::Rest);
        read(&mut schedule, 64);
        assert_eq!(schedule.resting(), (66, 0));
        // And from then on as before.
        read(&mut schedule, 70);
        assert_eq!(schedule.resting(), (67, 0));
    }

    /// A session waits for the wrist to stay calm for a few seconds: at a
    /// desk, a hand rests a moment between movements, and a session started
    /// then is spent on a wrist that is soon moving again.
    #[test]
    fn sessions_wait_for_lasting_calm() {
        // Calm for one poll in three, all afternoon: never long enough, so
        // every session waits its `CALM_WAIT` and starts busy.
        let mut schedule = Schedule::new();
        let fidgety = |t: u32| Moment {
            calm: t.is_multiple_of(6),
            ..resting(t)
        };
        let sessions = simulate(&mut schedule, 0, 4 * HOUR, fidgety);
        assert_eq!(sessions[0].start, CALM_WAIT, "{sessions:?}");
        assert!(sessions.iter().all(|s| s.how == Start::Busy));
        // Calm for six seconds in thirty: starts at the third calm poll.
        let mut schedule = Schedule::new();
        let settling = |t: u32| Moment {
            calm: t % 30 >= 24,
            ..resting(t)
        };
        let sessions = simulate(&mut schedule, 0, HOUR, settling);
        assert_eq!((sessions[0].start, sessions[0].how), (28, Start::Calm));
    }

    /// An hour at rest, and then the next session's due time: what the
    /// tests of how long a due session waits for calm start from.
    fn rested() -> (Schedule, u32, u32) {
        let mut schedule = Schedule::new();
        let sessions = simulate(&mut schedule, 0, HOUR, resting);
        let last = sessions.last().expect("an hour at rest measures").end;
        let (due, why) = schedule.due(0, false);
        assert_eq!(why, Why::Rest);
        (schedule, last, due)
    }

    /// A wrist busy for ten minutes from just before a session is due (busy
    /// hands at a desk, not enough to count as movement): the session
    /// doesn't wait them out, but starts busy at `CALM_WAIT`.
    #[test]
    fn a_busy_wrist_waits_for_calm_only_so_long() {
        let (mut schedule, last, due) = rested();
        let busy = |t: u32| (due - MINUTE..due + 10 * MINUTE).contains(&t);
        let after = simulate(&mut schedule, last + POLL, due + HOUR, |t| Moment {
            calm: !busy(t),
            ..resting(t)
        });
        assert_eq!((after[0].why, after[0].how), (Why::Rest, Start::Busy));
        assert!(about(after[0].start, due + CALM_WAIT), "{after:?}");
    }

    /// But calm 30 s after it's due is worth waiting for: it starts then.
    #[test]
    fn a_due_session_starts_when_calm_comes_soon() {
        let (mut schedule, last, due) = rested();
        let busy = |t: u32| (due - MINUTE..due + 30).contains(&t);
        let after = simulate(&mut schedule, last + POLL, due + HOUR, |t| Moment {
            calm: !busy(t),
            ..resting(t)
        });
        assert_eq!((after[0].why, after[0].how), (Why::Rest, Start::Calm));
        let settled = due + 30 + u32::from(START_CALM_POLLS) * POLL;
        assert!((due + 30..settled).contains(&after[0].start), "{after:?}");
    }

    /// A wrist busy all the time and never reading (walking, say): each
    /// try waits its `CALM_WAIT` and starts busy, overdue or not, so the
    /// overdue ones come every `OVERDUE_RETRY` and `CALM_WAIT`.
    #[test]
    fn a_wrist_that_never_calms_still_gets_its_overdue_tries() {
        let mut schedule = Schedule::new();
        schedule.ended(0, 13, Outcome::Reading(60), true, awake(0));
        let sessions = simulate(&mut schedule, POLL, 3 * HOUR, |t| Moment {
            calm: false,
            outcome: Outcome::NoReading,
            seconds: 30,
            ..resting(t)
        });
        let overdue = sessions.iter().position(|s| s.start >= OVERDUE);
        let overdue = overdue.expect("three hours without a reading are overdue");
        assert!(sessions.iter().all(|s| s.how == Start::Busy));
        // The first at the resting back-off's 4 minutes, and two more.
        assert!(
            about(sessions[0].start, 4 * MINUTE + CALM_WAIT),
            "{sessions:?}"
        );
        assert_eq!(overdue, 3, "{sessions:?}");
        assert!(sessions[overdue..].iter().all(|s| s.why == Why::Overdue));
        assert!(
            gaps(&sessions[overdue..])
                .iter()
                .all(|&w| about(w, OVERDUE_RETRY + CALM_WAIT)),
            "{sessions:?}"
        );
        // No wrist holds an overdue try back past `CALM_WAIT`, and none
        // starts busy before it.
        let mut fresh = Schedule::new();
        fresh.ended(0, 13, Outcome::Reading(60), true, awake(0));
        let late = 2 * OVERDUE;
        assert_eq!(fresh.start(late, late, 0), Start::Wait);
        assert_eq!(fresh.start(late + CALM_WAIT - 1, late, 0), Start::Wait);
        assert_eq!(fresh.start(late + CALM_WAIT, late, 0), Start::Busy);
    }

    /// An overdue try waits for a calm wrist like any other, and starts
    /// the moment it has been calm for `START_CALM_POLLS`: a wrist calm
    /// for 20 s in every minute, where every session fails all the same.
    #[test]
    fn overdue_sessions_wait_for_calm_too() {
        let mut schedule = Schedule::new();
        schedule.ended(0, 13, Outcome::Reading(60), true, awake(0));
        let calm = |t: u32| t % 60 >= 40;
        let sessions = simulate(&mut schedule, POLL, 3 * HOUR, |t| Moment {
            calm: calm(t),
            outcome: Outcome::NoReading,
            seconds: 30,
            ..resting(t)
        });
        let overdue: Vec<_> = sessions.iter().filter(|s| s.start >= OVERDUE).collect();
        assert!(overdue.len() >= 10, "{sessions:?}");
        for session in overdue {
            assert_eq!(session.how, Start::Calm, "{sessions:?}");
            // At the third calm poll of a calm stretch.
            assert!((44..48).contains(&(session.start % 60)), "{sessions:?}");
        }
    }

    /// A desk that won't read: each session runs its full 30 s through
    /// movement and fails, as the sealed watch's desk mornings would.
    /// Tries come 5 and then 10 minutes apart, and from half an hour after
    /// the last reading, overdue (see `OVERDUE`), they go on every 5
    /// minutes rather than backing off to 20 and more, each at the next
    /// calm moment. The budget never holds one back, even with every try
    /// on the bright arm and a 13 s glance every 7.5 minutes: it can't pay
    /// for them all at 30 mA (72 budget seconds each, ~11 an hour), so
    /// `heart` starts some at 20 mA or 12.5, as the budget allows, and
    /// none waits for it.
    #[test]
    fn a_desk_that_wont_read_tries_every_five_minutes_from_half_an_hour() {
        let mut schedule = Schedule::new();
        schedule.ended(0, 13, Outcome::Reading(60), true, awake(0));
        let (mut t, mut glance) = (0, 450);
        let mut sessions = Vec::new();
        // Tries by the drive they started at, and what they cost, by hour.
        let (mut drives, mut cost_by_hour) = ([0; 4], [0; 8]);
        while t < 8 * HOUR {
            let (due, _) = schedule.due(0, false);
            while glance < due {
                schedule.ended(glance + 13, 13, Outcome::NoReading, false, awake(0));
                glance += 450;
            }
            let (due, why) = schedule.due(0, false);
            assert_ne!(why, Why::Budget, "{sessions:?}");
            // Three calm polls first.
            let due = t.max(due);
            assert_eq!(schedule.start(due, due, 0), Start::Wait);
            let (start, how) = (due + 6, Start::Calm);
            t = start;
            let drive = bright_start(0, 2500, schedule.budget(t)).unwrap_or(0);
            drives[usize::from(drive)] += 1;
            let start = t;
            t += 30;
            let cost = budget_seconds(drive, 30_000);
            if let Some(hour) = cost_by_hour.get_mut((start / HOUR) as usize) {
                *hour += cost;
            }
            schedule.ended(t, cost, Outcome::NoReading, true, awake(0));
            // (A glance during the session is that session, watched.)
            while glance < t {
                glance += 450;
            }
            sessions.push(Session {
                start,
                end: t,
                why,
                how,
            });
        }
        let waits = gaps(&sessions);
        // The first try at 4 minutes (the resting back-off), then 5 and 10
        // minutes on (and three calm polls each); the next when a reading
        // becomes overdue, where the back-off's wait would be 20 minutes;
        // then every 5 minutes, overdue, the same.
        assert!(about(sessions[0].start, 4 * MINUTE + 6), "{sessions:?}");
        assert!(about(waits[0], 5 * MINUTE + 6), "{waits:?}");
        assert!(about(waits[1], 10 * MINUTE + 6), "{waits:?}");
        assert!(waits[3..].iter().all(|&w| w == 5 * MINUTE + 6), "{waits:?}");
        assert_eq!(sessions[2].why, Why::Retry);
        assert_eq!(sessions[3].start, OVERDUE + 6, "{sessions:?}");
        assert!(sessions[3..].iter().all(|s| s.why == Why::Overdue));
        assert!(sessions.iter().all(|s| s.how == Start::Calm));
        // Ten or eleven tries an hour, 5 to 5.5 minutes of LED, and no
        // hour over the doubled budget by more than a try.
        for hour in 1..8 {
            let spent = led(&sessions, hour * HOUR, (hour + 1) * HOUR);
            assert!((300..=330).contains(&spent), "hour {hour}: {spent} s");
            let cost = cost_by_hour[hour as usize];
            assert!(cost <= OVERDUE_BUDGET + 72, "hour {hour}: {cost} budget s");
        }
        // Most still start brighter, and some at each drive.
        assert_eq!(drives[3], 0);
        assert!(drives[0] > 0 && drives[2] > drives[0], "{drives:?}");
    }

    /// What `OVERDUE_RETRY`'s table says an overdue hour on a wrist that
    /// won't read takes, at worst, at 12.5 mA: tries, and LED seconds.
    #[test]
    fn an_overdue_hour_is_bounded_whatever_the_wrist() {
        // (calm when due, seconds a session runs) -> (tries, LED) an hour.
        let hour = |calm: bool, seconds: u32| {
            let mut schedule = Schedule::new();
            schedule.ended(0, 13, Outcome::Reading(60), true, awake(0));
            let sessions = simulate(&mut schedule, POLL, 9 * HOUR, |t| Moment {
                calm,
                outcome: Outcome::NoReading,
                seconds,
                ..resting(t)
            });
            let tries = |h: u32| {
                let hour = h * HOUR..(h + 1) * HOUR;
                sessions.iter().filter(|s| hour.contains(&s.start)).count() as u32
            };
            let most = (1..9).map(tries).max().expect("hours");
            let led = |h: u32| led(&sessions, h * HOUR, (h + 1) * HOUR);
            // The worst hour, and the worst once the bucket's own store
            // is spent (it holds an hour's refill when a reading first
            // becomes overdue).
            (
                most,
                (1..9).map(led).max().expect("hours"),
                (3..9).map(led).max().expect("hours"),
            )
        };
        // Never calm: each try waits its two minutes. Giving up at 19 s
        // (20 s of LED, with the half second either side), or running out.
        assert_eq!(hour(false, 20), (9, 180, 180));
        assert_eq!(hour(false, 30), (8, 240, 240));
        // Calm whenever it looks, and busy as soon as it has started: the
        // worst there is, since a reading that's late takes every calm
        // moment (see `Schedule::seek`), two minutes after the last
        // session. Giving up at 19 s, that's the pace; running out its
        // 30 s, the doubled budget sets it instead: an hour's refill,
        // after a first hour that also spends what the bucket held. (Up
        // to 0.11.3-rc.4, with tries only when due: 12 tries and 240 s,
        // 11 and 330 s.)
        assert_eq!(hour(true, 20), (26, 520, 520));
        assert_eq!(hour(true, 30), (24, 720, OVERDUE_BUDGET));
    }

    /// A busy failure backs off like any other, from when the session ended,
    /// until half an hour after the last reading.
    #[test]
    fn busy_failures_back_off_until_overdue() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        let mut t = 160;
        for retry in [5, 10] {
            schedule.ended(t + 30, 30, Outcome::NoReading, true, awake(0));
            let (due, why) = schedule.due(0, false);
            assert_eq!((due, why), (t + 30 + retry * MINUTE, Why::Retry));
            t = due;
        }
        // 20 minutes would pass the half hour.
        schedule.ended(t + 30, 30, Outcome::NoReading, true, awake(0));
        assert_eq!(schedule.due(0, false), (100 + OVERDUE, Why::Overdue));
    }

    #[test]
    fn failures_dont_spin() {
        // A wrist that never gives a reading (or a watch on a desk), calm
        // enough to try, and moving between tries: the worst case for
        // retrying.
        let mut schedule = Schedule::new();
        let sessions = simulate(&mut schedule, 0, 8 * HOUR, |t| Moment {
            active: t % 60 < 15,
            calm: t % 60 >= 15,
            outcome: Outcome::NoReading,
            seconds: 30,
            ..resting(t)
        });
        let waits = gaps(&sessions);
        // Five and ten minutes between the first tries, backing off, as
        // `Retry`. Twenty minutes in, a reading is late, and from then
        // on every calm moment is taken (see `Schedule::seek`): never
        // sooner than two minutes after the last session, and no more
        // of them than the budget allows, which is what paces them once
        // it has run down (30 s of LED every 150 s is its doubled refill).
        assert!(about(waits[0], FAILED_INTERVAL), "{waits:?}");
        assert!(about(waits[1], 2 * FAILED_INTERVAL), "{waits:?}");
        let sought = sessions
            .iter()
            .position(|s| s.why == Why::Calm)
            .expect("a sought session");
        assert!(sessions[1..sought].iter().all(|s| s.why == Why::Retry));
        assert!(sessions[sought].start >= SEEK_AFTER, "{sessions:?}");
        assert!(
            sessions[sought].start < SEEK_AFTER + 2 * MINUTE,
            "{sessions:?}"
        );
        assert!(waits.iter().all(|&w| w >= SEEK_GAP), "{waits:?}");
        assert!(sessions.iter().all(|s| s.how == Start::Calm));
        for hour in 0..8 {
            let spent = led(&sessions, hour * HOUR, (hour + 1) * HOUR);
            // (A full bucket's worth more in the first hours.)
            assert!(spent <= 2 * OVERDUE_BUDGET, "hour {hour}: {spent} s");
            if hour >= 2 {
                assert!(spent <= OVERDUE_BUDGET + 30, "hour {hour}: {spent} s");
            }
        }

        // Off the wrist and still (a nightstand), up to an hour: there's no
        // reading to be had, overdue or not.
        let mut schedule = Schedule::new();
        let sessions = simulate(&mut schedule, 0, 12 * HOUR, |t| Moment {
            outcome: Outcome::OffWrist,
            seconds: 4,
            ..resting(t)
        });
        let waits = gaps(&sessions);
        for (wait, expected) in waits.iter().zip([5, 10, 20, 40, 60, 60, 60]) {
            assert!(about(*wait, expected * MINUTE), "{waits:?}");
        }
        // (A wrist that won't read and offers no calm between its tries
        // keeps to the back-off and the overdue retries alone: see
        // `busy_failures_back_off_until_overdue` and
        // `an_overdue_hour_is_bounded_whatever_the_wrist`.)
    }

    // ==========================================================================
    // Seeking calm
    // ==========================================================================

    /// A reading that's late (none for `SEEK_AFTER`) makes the next calm
    /// moment worth a session by itself, no sooner than `SEEK_GAP` after
    /// the last one: here, a desk morning whose tries have backed off to
    /// 20 minutes apart.
    #[test]
    fn a_late_reading_takes_the_next_calm_moment() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(62), true, awake(0));
        // A reading 13 s ago: nothing to seek for 20 minutes.
        assert_eq!(schedule.seek(false), Some(100 + SEEK_AFTER));
        // Three tries on a busy wrist, backing off 5, 10 and then 20
        // minutes, which a reading overdue cuts short.
        schedule.ended(430, 30, Outcome::NoReading, true, awake(0));
        schedule.ended(760, 30, Outcome::NoReading, true, awake(0));
        assert_eq!(schedule.due(0, false), (760 + 10 * MINUTE, Why::Retry));
        schedule.ended(1390, 30, Outcome::NoReading, true, awake(0));
        assert_eq!(schedule.due(0, false), (100 + OVERDUE, Why::Overdue));
        // But calm is taken from 20 minutes after the reading, and two
        // after that last try: 6 minutes before the schedule's own.
        assert_eq!(schedule.seek(false), Some(1390 + SEEK_GAP));
        const { assert!(1390 + SEEK_GAP > 100 + SEEK_AFTER) };
        // A sought session that fails: the next no sooner than two
        // minutes on.
        schedule.ended(1560, 20, Outcome::NoReading, true, awake(0));
        assert_eq!(schedule.seek(false), Some(1560 + SEEK_GAP));
        // One that reads ends the seeking for another 20 minutes.
        schedule.ended(1700, 13, Outcome::Reading(71), true, awake(0));
        assert_eq!(schedule.seek(false), Some(1700 + SEEK_AFTER));
        assert_eq!(schedule.due(0, false).1, Why::Rest);
    }

    /// Where calm isn't what was missing, it isn't sought: asleep (the
    /// night keeps its own times), before the first session, on the
    /// charger, off the wrist, and after a session that failed on a calm
    /// wrist (a desk is calm all day).
    #[test]
    fn calm_is_not_sought_where_it_would_not_help() {
        let mut schedule = Schedule::new();
        assert_eq!(schedule.seek(false), None);
        schedule.ended(100, 13, Outcome::Reading(62), true, awake(0));
        assert!(schedule.seek(false).is_some());
        assert_eq!(schedule.seek(true), None);

        // A calm failure: no seeking, until a busy one says the wrist
        // moves after all.
        schedule.ended(2000, 30, Outcome::CalmNoReading, true, awake(0));
        assert_eq!(schedule.seek(false), None);
        schedule.ended(2700, 30, Outcome::NoReading, true, awake(0));
        assert_eq!(schedule.seek(false), Some(2700 + SEEK_GAP));

        // Off the wrist: none, until a reading.
        schedule.ended(3100, 4, Outcome::OffWrist, true, awake(0));
        assert_eq!(schedule.seek(false), None);
        schedule.ended(3500, 30, Outcome::NoReading, true, awake(0));
        assert!(schedule.seek(false).is_some());

        // On the charger, and until it has settled off it.
        assert_eq!(schedule.charger(3600, true), Charger::Arrived);
        assert_eq!(schedule.seek(false), None);
        assert_eq!(schedule.charger(4000, false), Charger::Left(CHARGER_SETTLE));
        assert_eq!(schedule.seek(false), None);
        assert_eq!(
            schedule.charger(4000 + CHARGER_SETTLE, false),
            Charger::Away
        );
        assert_eq!(
            schedule.seek(false),
            Some(4000 + SEEK_GAP),
            "the last reading was an hour ago"
        );
    }

    /// The budget holds a sought session back as it does a due one.
    #[test]
    fn seeking_waits_for_the_budget() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(62), true, awake(0));
        // A long look at the clock face, in a hand, spends it all.
        schedule.ended(1500, 400, Outcome::NoReading, false, awake(0));
        assert_eq!(schedule.budget(1500), 0);
        let from = schedule.seek(false).expect("seeking");
        assert!(from > 1500 + SEEK_GAP);
        assert!(schedule.budget(from) >= SESSION_RESERVE);
        assert!(schedule.budget(from - 12) < SESSION_RESERVE);
    }

    /// A day at rest, reading every time, never seeks: the schedule's
    /// longest interval is where seeking starts. And a night never does.
    #[test]
    fn a_wrist_that_reads_is_never_sought() {
        let mut schedule = Schedule::new();
        let sessions = simulate(&mut schedule, 0, 8 * HOUR, resting);
        assert!(sessions.iter().all(|s| s.why != Why::Calm), "{sessions:?}");
        let night = simulate(&mut schedule, 8 * HOUR, 16 * HOUR, |t| Moment {
            asleep: true,
            // A night that reads every other time.
            outcome: if t / 300 % 2 == 0 {
                Outcome::Reading(52)
            } else {
                Outcome::NoReading
            },
            ..resting(t)
        });
        assert!(night.iter().all(|s| s.why != Why::Calm), "{night:?}");
        // Nor a night that doesn't read at all.
        let night = simulate(&mut schedule, 16 * HOUR, 20 * HOUR, |t| Moment {
            asleep: true,
            outcome: Outcome::NoReading,
            seconds: 30,
            ..resting(t)
        });
        assert!(night.iter().all(|s| s.why != Why::Calm), "{night:?}");
    }

    /// A busy desk that reads when it gets 20 s of calm, which it does
    /// for half a minute every seven: the schedule's tries, timed by the
    /// back-off, mostly miss those; seeking finds each, so no reading is
    /// ever much more than `SEEK_AFTER` after the last.
    #[test]
    fn seeking_finds_the_calm_between_the_schedules_tries() {
        let mut schedule = Schedule::new();
        let desk = |t: u32| {
            let calm = t % 420 < 30;
            Moment {
                fidget: !calm,
                calm,
                // A session that starts in the calm half minute reads;
                // one started busy (after its two minutes' wait) fails.
                outcome: if calm {
                    Outcome::Reading(70)
                } else {
                    Outcome::NoReading
                },
                seconds: if calm { 13 } else { 20 },
                ..resting(t)
            }
        };
        let sessions = simulate(&mut schedule, 0, 8 * HOUR, desk);
        let readings: Vec<u32> = sessions
            .iter()
            .filter(|s| s.how == Start::Calm)
            .map(|s| s.end)
            .collect();
        let longest = readings
            .windows(2)
            .map(|w| w[1] - w[0])
            .max()
            .expect("readings");
        assert!(longest <= SEEK_AFTER + 420, "{longest} s: {sessions:?}");
        assert!(sessions.iter().any(|s| s.why == Why::Calm));
    }

    /// Asleep, a reading is overdue after the same half hour: a night's
    /// failures (a restless spell, a faint pulse) back off 5 and 10
    /// minutes, then retry every 5 rather than waiting 20.
    #[test]
    fn a_night_without_readings_is_overdue_after_half_an_hour_too() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 10, Outcome::Reading(52), true, sleeping());
        assert_eq!(schedule.due(0, true), (100 + ASLEEP_INTERVAL, Why::Asleep));
        let mut t = 100 + ASLEEP_INTERVAL;
        let mut due = Vec::new();
        for _ in 0..4 {
            schedule.ended(t + 30, 30, Outcome::NoReading, true, sleeping());
            let next = schedule.due(0, true);
            due.push(next);
            t = next.0;
        }
        assert_eq!(
            due,
            [
                (430 + 5 * MINUTE, Why::Retry),
                (760 + 10 * MINUTE, Why::Retry),
                (100 + OVERDUE, Why::Overdue),
                (1930 + 5 * MINUTE, Why::Overdue),
            ]
        );
    }

    /// Lying still awake for an hour without a reading (a field log,
    /// 2026-11-09): calm failures retry every ten minutes, not 20, 40, 60,
    /// and once a reading is overdue, half an hour in, every five.
    #[test]
    fn calm_failures_retry_sooner_for_a_while() {
        let mut schedule = Schedule::new();
        let sessions = simulate(&mut schedule, 0, 6 * HOUR, |t| Moment {
            outcome: Outcome::CalmNoReading,
            seconds: 30,
            ..resting(t)
        });
        let waits = gaps(&sessions);
        let expected = [5, 10, 10, 5, 5, 5, 5, 5, 5];
        for (wait, expected) in waits.iter().zip(expected) {
            assert!(about(*wait, expected * MINUTE), "{waits:?}");
        }
        // The hour after the first failure takes 10 sessions, not 4, all
        // of them within the LED budget.
        let first_hour = sessions.iter().filter(|s| s.start < HOUR).count();
        assert_eq!(first_hour, 10, "{sessions:?}");
        assert!(sessions[1..4].iter().all(|s| s.why == Why::Retry));
        assert!(sessions[4..].iter().all(|s| s.why == Why::Overdue));

        // A busy failure in between starts the count again, from wherever
        // the ordinary back-off has got to.
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        for t in [400, 700] {
            schedule.ended(t, 12, Outcome::NoReading, true, awake(0));
        }
        assert_eq!(schedule.due(0, false), (700 + 10 * MINUTE, Why::Retry));
        // (Its 20 minutes give way to the overdue retry at half an hour.)
        schedule.ended(1300, 30, Outcome::NoReading, true, awake(0));
        assert_eq!(schedule.due(0, false), (100 + OVERDUE, Why::Overdue));
        // (As does a calm failure's 10, once overdue.)
        schedule.ended(1900, 30, Outcome::CalmNoReading, true, awake(0));
        assert_eq!(schedule.due(0, false), (1900 + 5 * MINUTE, Why::Overdue));
        // And a reading ends it.
        schedule.ended(3100, 13, Outcome::Reading(60), true, awake(0));
        schedule.ended(3400, 30, Outcome::CalmNoReading, true, awake(0));
        assert_eq!(schedule.due(0, false), (3400 + 5 * MINUTE, Why::Retry));
    }

    #[test]
    fn calm_throughout_allows_a_twitch() {
        assert!(calm_throughout(0, 30));
        assert!(calm_throughout(7, 30));
        assert!(!calm_throughout(8, 30));
        assert!(calm_throughout(0, 0));
        assert!(calm_throughout(u8::MAX / 4, u8::MAX));
        assert!(!calm_throughout(u8::MAX, u8::MAX));
    }

    /// A session without a sign of a pulse ends early if the wrist was
    /// busy at half its checks or more.
    #[test]
    fn hopeless_is_busy_at_half_the_checks() {
        assert!(hopeless(HOPELESS_CHECKS, HOPELESS_CHECKS));
        assert!(hopeless(10, 19));
        assert!(!hopeless(9, 19));
        assert!(!hopeless(0, 19));
        assert!(hopeless(u8::MAX, u8::MAX));
        // Busy too often for a calm failure, not often enough to give up:
        // it runs its full length.
        assert!(!calm_throughout(8, 19) && !hopeless(8, 19));
        // Never both: giving up is never a calm failure cut short.
        for busy in 0..=HOPELESS_CHECKS {
            assert!(!(hopeless(busy, HOPELESS_CHECKS) && calm_throughout(busy, HOPELESS_CHECKS)));
        }
    }

    #[test]
    fn brighter_seconds_cost_more_budget() {
        assert_eq!(budget_seconds(0, 30_000), 30);
        assert_eq!(budget_seconds(0, 29_001), 30);
        assert_eq!(budget_seconds(1, 10_000), 16);
        assert_eq!(budget_seconds(2, 15_000), 36);
        assert_eq!(budget_seconds(3, 15_000), 48);
        assert_eq!(budget_seconds(0, 0), 0);
    }

    #[test]
    fn a_reading_ends_the_failure_back_off() {
        let mut schedule = Schedule::new();
        for t in [0, 300] {
            schedule.ended(t + 30, 30, Outcome::NoReading, true, awake(0));
        }
        assert_eq!(schedule.due(0, false), (330 + 10 * MINUTE, Why::Retry));
        schedule.ended(2200, 13, Outcome::Reading(60), true, awake(0));
        // The resting back-off carries on from the minimum.
        assert_eq!(schedule.due(0, false), (2200 + 2 * MIN_INTERVAL, Why::Rest));
    }

    #[test]
    fn moving_a_watch_that_was_off_the_wrist_retries_sooner() {
        let mut schedule = Schedule::new();
        let mut t = 0;
        for _ in 0..5 {
            let (due, _) = schedule.due(0, false);
            t = t.max(due) + 10;
            schedule.ended(t, 10, Outcome::OffWrist, true, awake(0));
        }
        assert_eq!(
            schedule.due(0, false),
            (t + MAX_FAILED_INTERVAL, Why::Retry)
        );
        // Picked up (to be put on, most likely).
        assert_eq!(
            schedule.due(MOVED_POLLS, false),
            (t + FAILED_INTERVAL, Why::Retry)
        );
        // A wrist that gave no reading, though, is only held to the
        // resting interval (with a reading under half an hour ago, so not
        // overdue).
        schedule.ended(t + 100, 13, Outcome::Reading(60), true, awake(0));
        for after in [150, 300, 450, 600] {
            schedule.ended(t + after, 30, Outcome::NoReading, true, awake(0));
        }
        assert_eq!(
            schedule.due(MOVED_POLLS, false),
            (t + 600 + MAX_INTERVAL, Why::Retry)
        );
    }

    #[test]
    fn the_led_budget_caps_a_bad_day() {
        // The worst wrist: moving between sessions, and every reading far
        // above resting, each taking the full 30 s. Unbudgeted, that's a
        // session every two minutes: 15 minutes of LED an hour.
        let mut schedule = Schedule::new();
        schedule.ended(0, 13, Outcome::Reading(60), true, awake(0));
        let sessions = simulate(&mut schedule, 60, 10 * HOUR, |t| Moment {
            active: t % 60 < 15,
            calm: t % 60 >= 15,
            outcome: Outcome::Reading(120),
            seconds: 30,
            ..resting(t)
        });
        assert!(sessions.iter().any(|s| s.why == Why::Budget));
        // The bucket starts full, so the first hour may spend up to twice
        // the budget; after that, the budget an hour, give or take a
        // session's worth at the window's edges.
        assert!(led(&sessions, 0, HOUR) <= 2 * LED_BUDGET);
        for hour in 1..9 {
            let spent = led(&sessions, hour * HOUR, (hour + 1) * HOUR);
            assert!(
                spent <= LED_BUDGET + SESSION_RESERVE,
                "hour {hour}: {spent} s"
            );
            assert!(
                spent >= LED_BUDGET - SESSION_RESERVE,
                "hour {hour}: {spent} s"
            );
        }
        // And an hour later it's full again (and then some: without a
        // reading meanwhile, one is overdue by then).
        let t = sessions.last().expect("measured").end;
        assert!(schedule.budget(t + HOUR) >= LED_BUDGET);
    }

    #[test]
    fn watching_spends_the_budget_but_isnt_held_to_it() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        // Five minutes staring at the clock face, the watch in a hand.
        schedule.ended(500, 300, Outcome::NoReading, false, awake(0));
        assert_eq!(schedule.budget(500), 0);
        // The background waits for the budget to earn back a session, 6
        // minutes, rather than the 4 minutes it would otherwise.
        let (at, why) = schedule.due(0, false);
        assert_eq!(why, Why::Budget);
        assert_eq!(at, 500 + SESSION_RESERVE * 3600 / LED_BUDGET);
    }

    #[test]
    fn watching_before_the_first_session_holds_it_to_the_budget() {
        let mut schedule = Schedule::new();
        // The clock face looked at four times after boot, the watch on a
        // desk: none counts as the first session, but each spends the
        // budget.
        for end in [40, 80, 120, 160] {
            schedule.ended(end, 96, Outcome::OffWrist, false, awake(0));
        }
        assert_eq!(schedule.budget(160), 0);
        let (at, why) = schedule.due(0, false);
        assert_eq!(why, Why::Budget);
        assert_eq!(at, 160 + SESSION_RESERVE * 3600 / LED_BUDGET);
        // A single look leaves a session's worth: the first is at once.
        let mut schedule = Schedule::new();
        schedule.ended(40, 96, Outcome::OffWrist, false, awake(0));
        assert_eq!(schedule.due(0, false), (0, Why::First));
    }

    #[test]
    fn watching_without_a_reading_is_neither_a_measurement_nor_a_failure() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        let before = schedule.due(0, false);
        schedule.ended(200, 20, Outcome::NoReading, false, awake(0));
        schedule.ended(300, 20, Outcome::OffWrist, false, awake(0));
        assert_eq!(schedule.due(0, false), before);
        // But a reading while watching is a measurement.
        schedule.ended(400, 20, Outcome::Reading(61), false, awake(0));
        assert_eq!(schedule.due(0, false).0, 400 + 2 * 2 * MIN_INTERVAL);
    }

    /// A workout's session (see `heart`'s "Through a workout") is a
    /// watched one that lasts as long as the workout does. `heart` tells
    /// the schedule of it as one session's worth of LED, with no stepless
    /// polls before it, and as a reading only if it read in its last half
    /// minute. So half an hour's effort without a step neither empties
    /// the budget nor teaches the resting estimate, and what follows is the
    /// usual schedule after an elevated reading, to follow the recovery.
    #[test]
    fn a_workouts_session_costs_one_sessions_worth_and_is_never_resting() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        schedule.ended(400, 13, Outcome::Reading(61), true, awake(0));
        assert_eq!(schedule.resting().0, 61);
        let budget = schedule.budget(400);
        // Weights: no steps for half an hour, the wrist busy throughout.
        let workout = Before {
            active_polls: u8::MAX,
            stepless_polls: 0,
            asleep: false,
        };
        let end = 400 + 30 * MINUTE;
        schedule.ended(end, SESSION_RESERVE, Outcome::Reading(118), false, workout);
        assert_eq!(schedule.resting().0, 61);
        assert!(schedule.budget(end) >= budget.min(LED_BUDGET) - SESSION_RESERVE);
        // Still moving: back within two minutes. Once the wrist rests, at
        // the elevated interval.
        assert_eq!(
            schedule.due(MOVED_POLLS, false),
            (end + MIN_INTERVAL, Why::Moved)
        );
        assert_eq!(
            schedule.due(0, false),
            (end + ELEVATED_INTERVAL, Why::Elevated)
        );

        // One that last read long before it ended (`heart`'s
        // `Ended::Faded`) is no measurement at all: the schedule carries
        // on from where it was, a session's LED poorer.
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        let before = schedule.due(0, false);
        schedule.ended(end, SESSION_RESERVE, Outcome::NoReading, false, workout);
        assert_eq!(schedule.due(0, false), before);
        // And the night after it is as any other: in step with the
        // phone's buckets, five minutes apart.
        let asleep = Before {
            asleep: true,
            ..awake(0)
        };
        schedule.ended(end + 600, 12, Outcome::Reading(52), true, asleep);
        let (at, why) = schedule.due_in_phase(0, true, Some(0));
        assert_eq!(why, Why::Asleep);
        assert_eq!(at % BUCKET, BUCKET_SLOT);
        assert!(at - (end + 600) <= ASLEEP_INTERVAL + BUCKET - MIN_INTERVAL);
    }

    #[test]
    fn bright_starts_go_up_two_drives_as_far_as_headroom_and_budget_allow() {
        let plenty = LED_BUDGET;
        // The desk's median peak at 12.5 mA ×2 (2026-11-09): 30 mA.
        assert_eq!(bright_start(0, 3098, plenty), Some(2));
        // Its 90th percentile: 20 mA.
        assert_eq!(bright_start(0, 4035, plenty), Some(1));
        assert_eq!(bright_start(0, 5200, plenty), None);
        // A whole session at 30 mA is 72 s of budget, at 20 mA 48.
        assert_eq!(bright_start(0, 2000, 72), Some(2));
        assert_eq!(bright_start(0, 2000, 71), Some(1));
        assert_eq!(bright_start(0, 2000, 47), None);
    }

    /// The LED experiment's arms take turns by round: every try of a round
    /// (failed, retried) keeps its arm, and only a reading
    /// starts the next round, on the other arm. The field log that showed
    /// the need (2026-11-10): a round's first try, bright, stopped for a
    /// moving wrist after 6 s, and its retry read, dim; the next round's
    /// first try was bright again, and so on, so bright had the scheduled
    /// tries (often cut short) and dim the retries (at a still moment).
    #[test]
    fn every_try_of_a_round_keeps_its_arm() {
        let mut schedule = Schedule::new();
        let mut t = 0;
        let mut first_tries = Vec::new();
        // Each round: a first try that fails on a moving wrist, one that
        // fails calm, a retry that reads; or, every other round, a first
        // try that reads.
        for round in 1..=8 {
            let tries: &[Outcome] = if round % 2 == 1 {
                &[
                    Outcome::NoReading,
                    Outcome::CalmNoReading,
                    Outcome::Reading(60),
                ]
            } else {
                &[Outcome::Reading(60)]
            };
            let mut arms = Vec::new();
            for &outcome in tries {
                arms.push(schedule.begin(false));
                t += 300;
                schedule.ended(t, 13, outcome, true, awake(0));
            }
            assert!(
                arms.iter().all(|&a| a == arms[0]),
                "round {round}: {arms:?}"
            );
            assert_eq!(arms[0].0, round);
            first_tries.push(arms[0].1);
        }
        // Dim first, then turns, however many tries each round took.
        assert_eq!(first_tries, [Arm::Dim, Arm::Bright].repeat(4));
    }

    /// Asleep, a session is on neither arm, and doesn't take a turn: the
    /// first awake session of a round does, even if the round started
    /// asleep. A watched reading ends a round too (the next background
    /// session is due afresh), and a watched failure doesn't.
    #[test]
    fn rounds_asleep_or_watched_dont_take_a_turn() {
        let mut schedule = Schedule::new();
        assert_eq!(schedule.begin(false), (1, Arm::Dim));
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        // A night's rounds.
        for (round, t) in (2..6).zip((400..).step_by(300)) {
            assert_eq!(schedule.begin(true), (round, Arm::Asleep));
            schedule.ended(t, 13, Outcome::Reading(50), true, sleeping());
        }
        // Waking up mid-round: the awake retry takes the turn.
        assert_eq!(schedule.begin(true), (6, Arm::Asleep));
        schedule.ended(2000, 30, Outcome::NoReading, true, sleeping());
        assert_eq!(schedule.begin(false), (6, Arm::Bright));
        // Falling asleep again within the round, then waking, keeps it.
        schedule.ended(2300, 30, Outcome::NoReading, true, awake(0));
        assert_eq!(schedule.begin(true), (6, Arm::Asleep));
        schedule.ended(2600, 30, Outcome::NoReading, true, sleeping());
        assert_eq!(schedule.begin(false), (6, Arm::Bright));
        // Someone looks and gets no reading: still the same round.
        schedule.ended(2900, 30, Outcome::NoReading, true, awake(0));
        schedule.ended(3000, 20, Outcome::OffWrist, false, awake(0));
        assert_eq!(schedule.begin(false), (6, Arm::Bright));
        // Someone looks and gets one: the round is over.
        schedule.ended(3300, 30, Outcome::NoReading, true, awake(0));
        schedule.ended(3400, 20, Outcome::Reading(64), false, awake(0));
        assert_eq!(schedule.begin(false), (7, Arm::Dim));
    }

    // ==========================================================================
    // The charger
    // ==========================================================================

    /// A schedule backed off an hour for a watch off the wrist, as one on
    /// its way to the charger is: five failures off the wrist, the last
    /// ending at the time returned.
    fn off_the_wrist() -> (Schedule, u32) {
        let mut schedule = Schedule::new();
        schedule.ended(30, 13, Outcome::Reading(60), true, awake(0));
        let mut t = 30;
        for _ in 0..5 {
            let (due, _) = schedule.due(0, false);
            t = t.max(due) + 10;
            schedule.ended(t, 10, Outcome::OffWrist, true, awake(0));
        }
        assert_eq!(
            schedule.due(0, false),
            (t + MAX_FAILED_INTERVAL, Why::Retry)
        );
        (schedule, t)
    }

    /// On the charger, nothing is measured, and nothing about the schedule
    /// changes: when the next session would be due, and why, and the
    /// budget, stay as they were for as long as it charges.
    #[test]
    fn the_charger_holds_every_session_back_and_changes_nothing() {
        let (mut schedule, t) = off_the_wrist();
        let (due, moved) = (schedule.due(0, false), schedule.due(MOVED_POLLS, false));
        let budget = schedule.budget(t + 100);
        assert_eq!(schedule.charger(t + 100, true), Charger::Arrived);
        for later in [101, 200, 1000, 3 * HOUR] {
            assert_eq!(schedule.charger(t + later, true), Charger::On);
        }
        assert_eq!(schedule.due(0, false), due);
        assert_eq!(schedule.due(MOVED_POLLS, false), moved);
        assert_eq!(schedule.budget(t + 100), budget);
        // Away from it all along, it's the same as never having been told.
        let (mut away, _) = off_the_wrist();
        assert_eq!(away.charger(t + 100, false), Charger::Away);
        assert_eq!(away.due(0, false), due);
    }

    /// A session the charger cut short, without a reading, isn't a failure
    /// (nor off the wrist, nor calm): only its LED time comes out of the
    /// budget. And it tells the schedule of the charger, so the wait
    /// after it isn't another arrival.
    #[test]
    fn a_session_cut_short_by_the_charger_only_spends_its_led() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        schedule.ended(400, 30, Outcome::CalmNoReading, true, awake(0));
        let due = schedule.due(0, false);
        assert_eq!(due, (400 + FAILED_INTERVAL, Why::Retry));
        let budget = schedule.budget(700);
        let _ = schedule.begin(false);
        schedule.cut_short(700, 20);
        assert_eq!(schedule.due(0, false), due);
        assert_eq!(schedule.budget(700), budget - 20);
        assert_eq!(schedule.charger(701, true), Charger::On);
        // Even one that cut short a background session's very first try
        // after boot: the first is still due at once, off the charger.
        let mut schedule = Schedule::new();
        let _ = schedule.begin(false);
        schedule.cut_short(5, 3);
        assert_eq!(schedule.due(0, false), (0, Why::First));
        assert_eq!(schedule.budget(5), LED_BUDGET - 3);
    }

    /// Taken off the charger, nothing is measured for `CHARGER_SETTLE`;
    /// then the schedule starts over: a session two minutes after the
    /// power went, the back-off from the minimum after it, whatever it was
    /// backing off from before the charge (an hour off the wrist, here).
    #[test]
    fn off_the_charger_it_settles_then_starts_over() {
        let (mut schedule, t) = off_the_wrist();
        assert_eq!(schedule.charger(t + 100, true), Charger::Arrived);
        let off = t + 2 * HOUR;
        assert_eq!(schedule.charger(off, false), Charger::Left(CHARGER_SETTLE));
        assert_eq!(schedule.charger(off + 3, false), Charger::Settling(7));
        assert_eq!(schedule.charger(off + 9, false), Charger::Settling(1));
        assert_eq!(schedule.charger(off + 10, false), Charger::Away);
        assert_eq!(schedule.charger(off + 11, false), Charger::Away);
        assert_eq!(schedule.due(0, false), (off + MIN_INTERVAL, Why::Rest));
        // Movement can't bring it sooner, as the power going counts as the
        // last session.
        assert_eq!(
            schedule.due(MOVED_POLLS, false),
            (off + MIN_INTERVAL, Why::Moved)
        );
        // Asleep (a nap after a charge), as asleep.
        assert_eq!(schedule.due(0, true), (off + MIN_INTERVAL, Why::Asleep));
        // The last reading was hours ago: overdue, and it waits for a calm
        // wrist all the same (the watch is being strapped on).
        assert_eq!(
            schedule.start(off + MIN_INTERVAL, off + MIN_INTERVAL, 0),
            Start::Wait
        );
        // And a reading backs off from the minimum, as after movement.
        let at = off + MIN_INTERVAL + 13;
        schedule.ended(at, 13, Outcome::Reading(62), true, awake(0));
        assert_eq!(schedule.due(0, false), (at + 2 * MIN_INTERVAL, Why::Rest));
    }

    /// A watch that boots on the charger (an update, or charged back from
    /// empty) measures nothing until it's off, and then two minutes on,
    /// not at once: it's likely in a hand.
    #[test]
    fn a_boot_on_the_charger_waits_until_it_is_off() {
        let mut schedule = Schedule::new();
        assert_eq!(schedule.charger(1, true), Charger::Arrived);
        assert_eq!(schedule.charger(900, true), Charger::On);
        assert_eq!(schedule.charger(1000, false), Charger::Left(CHARGER_SETTLE));
        assert_eq!(schedule.charger(1010, false), Charger::Away);
        assert_eq!(schedule.due(0, false), (1000 + MIN_INTERVAL, Why::Rest));
    }

    /// A wobbly cradle, making and breaking contact: each break settles
    /// afresh, and contact back within the settling time is the same
    /// charge, not another arrival (which would end a round of tries), but
    /// says it's back, once, so a field log has an "on" for every "off".
    #[test]
    fn a_wobbly_cradle_is_one_charge() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        assert_eq!(schedule.charger(200, true), Charger::Arrived);
        assert_eq!(schedule.charger(201, false), Charger::Left(CHARGER_SETTLE));
        assert_eq!(schedule.charger(202, true), Charger::Back);
        assert_eq!(schedule.charger(202, true), Charger::On);
        assert_eq!(schedule.charger(203, false), Charger::Left(CHARGER_SETTLE));
        assert_eq!(schedule.charger(209, false), Charger::Settling(4));
        assert_eq!(schedule.charger(210, true), Charger::Back);
        assert_eq!(schedule.charger(500, false), Charger::Left(CHARGER_SETTLE));
        assert_eq!(schedule.charger(510, false), Charger::Away);
        assert_eq!(schedule.due(0, false), (500 + MIN_INTERVAL, Why::Rest));
        // Set down again later, it's a new arrival.
        assert_eq!(schedule.charger(600, true), Charger::Arrived);
    }

    /// The charger ends a round of tries under way, as a reading would:
    /// the tries after a charge are on the other arm, in a round of their
    /// own (see `Arm`).
    #[test]
    fn the_charger_ends_a_round() {
        let mut schedule = Schedule::new();
        assert_eq!(schedule.begin(false), (1, Arm::Dim));
        schedule.ended(30, 30, Outcome::NoReading, true, awake(0));
        assert_eq!(schedule.begin(false), (1, Arm::Dim));
        schedule.cut_short(340, 10);
        assert_eq!(schedule.charger(3000, false), Charger::Left(CHARGER_SETTLE));
        assert_eq!(schedule.begin(false), (2, Arm::Bright));
        // An arrival between sessions ends it too.
        schedule.ended(3200, 30, Outcome::NoReading, true, awake(0));
        assert_eq!(schedule.charger(3300, true), Charger::Arrived);
        assert_eq!(schedule.charger(5000, false), Charger::Left(CHARGER_SETTLE));
        assert_eq!(schedule.begin(false), (3, Arm::Dim));
    }

    /// An hour without a reading, the budget refills twice as fast, into a
    /// bucket twice as deep; a reading puts it back.
    #[test]
    fn an_overdue_reading_doubles_the_budget_until_one_comes() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        // Five minutes watching without a reading empties it.
        schedule.ended(400, 300, Outcome::NoReading, false, awake(0));
        assert_eq!(schedule.budget(400), 0);
        let overdue = 100 + OVERDUE;
        // Until then, at the usual rate (a second every 12), and no more
        // than the usual cap.
        assert_eq!(schedule.budget(400 + 600), 50);
        assert_eq!(schedule.budget(overdue - 100), 116);
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        schedule.ended(overdue - 300, 300, Outcome::NoReading, false, awake(0));
        assert_eq!(schedule.budget(overdue), 25);
        // Then twice as fast, up to twice as much.
        assert_eq!(schedule.budget(overdue + 900), 175);
        assert_eq!(schedule.budget(overdue + HOUR), OVERDUE_BUDGET);
        assert_eq!(schedule.budget(overdue + 10 * HOUR), OVERDUE_BUDGET);
        // Spending some, and earning it back, keeps the part-earned second.
        schedule.ended(overdue + HOUR, 100, Outcome::NoReading, true, awake(0));
        for t in 1..=100 {
            schedule.ended(
                overdue + HOUR + 5 * t,
                0,
                Outcome::NoReading,
                false,
                awake(0),
            );
        }
        assert_eq!(
            schedule.budget(overdue + HOUR + 500),
            OVERDUE_BUDGET - 100 + 83
        );
        // A reading: the usual cap again, and the usual rate.
        schedule.ended(overdue + 2 * HOUR, 13, Outcome::Reading(60), true, awake(0));
        assert_eq!(schedule.budget(overdue + 2 * HOUR), LED_BUDGET);
        schedule.ended(overdue + 2 * HOUR, 150, Outcome::NoReading, false, awake(0));
        assert_eq!(schedule.budget(overdue + 2 * HOUR + 600), 200);
    }

    /// The background waits for an overdue budget to hold a session at the
    /// faster rate, and only at the faster rate once it's overdue.
    #[test]
    fn an_overdue_budget_affords_sessions_sooner() {
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        let overdue = 100 + OVERDUE;
        // A reading that empties it puts off being overdue: the usual rate,
        // past when it would have been.
        schedule.ended(overdue - 400, 600, Outcome::Reading(60), false, awake(0));
        assert_eq!(schedule.budget(overdue + 200), 600 / 12);
        let mut schedule = Schedule::new();
        schedule.ended(100, 13, Outcome::Reading(60), true, awake(0));
        schedule.ended(overdue - 420, 600, Outcome::NoReading, false, awake(0));
        schedule.ended(overdue - 300, 30, Outcome::NoReading, true, awake(0));
        // 0 left at `overdue - 300`: 25 earned by `overdue`, 5 more in
        // 30 s.
        assert_eq!(schedule.due(0, false), (overdue + 30, Why::Budget));
    }

    /// A whole day overdue, every session failing calm at its full 30 s on
    /// the bright arm, at the brightest start the budget allows (72 budget
    /// seconds at 30 mA, 48 at 20 mA, 30 at 12.5), as `heart` does: the
    /// doubled budget caps the LED, the worst case docs/power-plan.md
    /// works from.
    #[test]
    fn the_doubled_budget_caps_an_overdue_day() {
        let mut schedule = Schedule::new();
        let (mut t, mut spent) = (0, Vec::new());
        while t < 24 * HOUR {
            let (due, _) = schedule.due(0, false);
            t = t.max(due);
            let drive = bright_start(0, 2500, schedule.budget(t)).unwrap_or(0);
            let cost = budget_seconds(drive, 30_000);
            t += 30;
            schedule.ended(t, cost, Outcome::CalmNoReading, true, awake(0));
            spent.push((t, cost));
        }
        for hour in 2..23 {
            let spent: u32 = spent
                .iter()
                .filter(|(t, _)| (hour * HOUR..(hour + 1) * HOUR).contains(t))
                .map(|(_, s)| s)
                .sum();
            assert!(spent <= OVERDUE_BUDGET + 72, "hour {hour}: {spent} s");
            assert!(spent > LED_BUDGET, "hour {hour}: {spent} s");
        }
    }
}

/// Kani proofs (`cargo kani` here, one at a time with `--harness`: CBMC
/// takes memory): what the tests above check on simulated wrists, for
/// every history of a few sessions. Nothing panics or overflows; the
/// next session is due between two minutes and an hour after the last,
/// for the reason the module docs give, and not before the budget holds
/// a session's worth; the budget fills and empties as a token bucket
/// should; and the resting estimates stay among the readings they came
/// from.
///
/// `Schedule`'s fields are private, so a proof can't start from any state
/// at all: each starts from a boot and records a few sessions, any at
/// all, one after the other (see `any_history`). The uptime is under ten
/// years: `due` adds to it unchecked, and would overflow in the last
/// hours of a `u32` of seconds (136 years).
#[cfg(kani)]
mod proofs {
    use super::heart_schedule::*;

    /// Seconds since boot: up to ten years.
    const UPTIME: u32 = 10 * 365 * 86_400;

    /// A schedule, and what happened to it, as the module docs tell it.
    struct History {
        schedule: Schedule,
        /// When the last session ended, counted or not.
        now: u32,
        /// When the last counted session ended (a background one, or one
        /// with a reading), and how; `None` before any.
        counted: Option<(u32, Outcome)>,
        /// When the last reading came (0: none since boot).
        last_reading: u32,
        /// The lowest and highest readings so far, if any.
        readings: Option<(u8, u8)>,
    }

    impl History {
        fn boot() -> Self {
            Self {
                schedule: Schedule::new(),
                now: 0,
                counted: None,
                last_reading: 0,
                readings: None,
            }
        }

        /// Records any session at all, ending no earlier than the last;
        /// a background one begun first, as `heart` does, for its round.
        fn any_session(&mut self) {
            let (now, led_seconds, background): (u32, u32, bool) = kani::any();
            kani::assume(self.now <= now && now < UPTIME);
            let outcome = match kani::any::<u8>() & 3 {
                0 => Outcome::Reading(kani::any()),
                1 => Outcome::NoReading,
                2 => Outcome::CalmNoReading,
                _ => Outcome::OffWrist,
            };
            let before = Before {
                active_polls: kani::any(),
                stepless_polls: kani::any(),
                asleep: kani::any(),
            };
            if background {
                let _ = self.schedule.begin(before.asleep);
            }
            self.schedule
                .ended(now, led_seconds, outcome, background, before);

            self.now = now;
            if let Outcome::Reading(bpm) = outcome {
                self.last_reading = now;
                let (low, high) = self.readings.unwrap_or((bpm, bpm));
                self.readings = Some((low.min(bpm), high.max(bpm)));
            }
            if background || matches!(outcome, Outcome::Reading(_)) {
                self.counted = Some((now, outcome));
            }
        }

        /// When a reading is overdue.
        fn overdue_from(&self) -> u32 {
            self.last_reading + OVERDUE
        }
    }

    /// A boot, and then `sessions` sessions, any at all.
    fn any_history(sessions: usize) -> History {
        let mut history = History::boot();
        for _ in 0..sessions {
            history.any_session();
        }
        history
    }

    /// After any six sessions, whatever the wrist has done since: the
    /// first counted session is due at once, and every later one at
    /// least `MIN_INTERVAL` after the last (so sessions never run back to
    /// back); unless the budget holds it back, at most
    /// `MAX_FAILED_INTERVAL` after (so the schedule never stalls).
    ///
    /// The budget: at the time it's due, the bucket holds the
    /// `SESSION_RESERVE` a background session needs; and if it's the
    /// budget that holds it back, that's the first second it does, and
    /// within the 6 minutes that earn a session's worth (and 6 s). That
    /// goes for the first session after a boot too, which watched
    /// sessions without a reading (which don't count as the first) may
    /// have emptied the bucket for.
    ///
    /// And once due, it starts (see `Schedule::start`), whatever the
    /// wrist, within `CALM_WAIT` of when it was found due, overdue or
    /// not; it only waits, then, while the wrist hasn't been calm for
    /// `START_CALM_POLLS`, and starts calm when it has.
    #[kani::proof]
    #[kani::unwind(7)]
    fn a_session_is_due_within_the_hour_and_the_budget() {
        let history = any_history(6);
        let schedule = &history.schedule;
        let (at, why) = schedule.due(kani::any(), kani::any());

        let (found, now, calm_polls): (u32, u32, u8) = kani::any();
        kani::assume(at <= found && found <= now && now < UPTIME);
        let start = schedule.start(now, found, calm_polls);
        let calm = calm_polls >= START_CALM_POLLS;
        let waits = !calm && now - found < CALM_WAIT;
        assert_eq!(start == Start::Wait, waits);
        assert_eq!(start == Start::Calm, calm);

        assert!(schedule.budget(at) >= SESSION_RESERVE);
        if why == Why::Budget {
            assert!(schedule.budget(at - 1) < SESSION_RESERVE);
            // (And the part-earned second lost as a reading becomes
            // overdue: under 6 s at the doubled rate.)
            assert!(at < history.now + SESSION_RESERVE * 3600 / LED_BUDGET + 6);
        }
        let Some((last, _)) = history.counted else {
            assert!(why == Why::Budget || (at, why) == (0, Why::First));
            return;
        };
        assert!(why != Why::First);
        assert!(at >= last + MIN_INTERVAL);
        if why != Why::Budget {
            assert!(at <= last + MAX_FAILED_INTERVAL);
        }
    }

    /// After any six sessions: why the next is due is what the module
    /// docs say, given how the last counted session ended and what the
    /// wrist has done since.
    ///
    /// - After a reading: movement (awake) makes it `MIN_INTERVAL`;
    ///   otherwise an elevated reading `ELEVATED_INTERVAL`; otherwise
    ///   asleep at most `ASLEEP_INTERVAL`, and awake at most
    ///   `MAX_INTERVAL`.
    /// - After a failure it's a retry, within `CALM_RETRY_INTERVAL` if
    ///   the wrist was calm, and, on a wrist, within `OVERDUE_RETRY` once
    ///   a reading is overdue.
    #[kani::proof]
    #[kani::unwind(7)]
    fn why_a_session_is_due_is_what_happened() {
        let history = any_history(6);
        let (active_polls, asleep): (u8, bool) = kani::any();
        let (at, why) = history.schedule.due(active_polls, asleep);
        let Some((last, outcome)) = history.counted else {
            return;
        };
        let read = matches!(outcome, Outcome::Reading(_));
        let moved = !asleep && active_polls >= MOVED_POLLS;
        match why {
            Why::First => unreachable!("a session has been counted"),
            Why::Budget => return,
            Why::Moved => assert!(read && moved && at == last + MIN_INTERVAL),
            Why::Elevated => assert!(read && !moved && at == last + ELEVATED_INTERVAL),
            Why::Asleep => assert!(read && asleep && at <= last + ASLEEP_INTERVAL),
            Why::Rest => assert!(read && !asleep && !moved && at <= last + MAX_INTERVAL),
            Why::Retry => assert!(!read),
            Why::Overdue => assert!(!read && outcome != Outcome::OffWrist),
            Why::Calm => unreachable!("only `heart` says so, of a sought session"),
        }
        assert!(read || !moved || at <= last + MAX_INTERVAL);
        if outcome == Outcome::CalmNoReading {
            assert!(at <= last + CALM_RETRY_INTERVAL);
        }
        if !read && outcome != Outcome::OffWrist {
            let overdue = (last + OVERDUE_RETRY).max(history.overdue_from());
            assert!(at <= overdue);
            assert!(why == Why::Retry || at == overdue);
        }
    }

    /// After any three sessions, between any two later times: the budget
    /// never passes `LED_BUDGET`, or `OVERDUE_BUDGET` once a reading is
    /// overdue; it never shrinks with time alone; and it earns no faster
    /// than its rate (a second every 12 s, or every 6 s overdue), so no
    /// hour earns more than it says.
    #[kani::proof]
    #[kani::unwind(5)]
    fn the_budget_fills_at_its_rate_up_to_its_cap() {
        let history = any_history(3);
        let (earlier, later): (u32, u32) = kani::any();
        kani::assume(history.now <= earlier && earlier <= later && later < UPTIME);
        let schedule = &history.schedule;
        let (before, after) = (schedule.budget(earlier), schedule.budget(later));

        let overdue = later > history.overdue_from();
        assert!(after <= if overdue { OVERDUE_BUDGET } else { LED_BUDGET });
        assert!(before <= after);
        let every = 3600 / if overdue { OVERDUE_BUDGET } else { LED_BUDGET };
        assert!(after - before <= (later - earlier).div_ceil(every));
    }

    /// After any three sessions: bringing the budget up to date on the way
    /// (as every session's end does; here a watched one that spent
    /// nothing and read nothing, which does only that) changes nothing
    /// about what it holds later, so frequent sessions lose no time to
    /// rounding.
    #[kani::proof]
    #[kani::unwind(5)]
    fn refilling_the_budget_on_the_way_loses_nothing() {
        let history = any_history(3);
        let (on_the_way, later): (u32, u32) = kani::any();
        kani::assume(history.now <= on_the_way && on_the_way <= later && later < UPTIME);

        let mut refilled = history.schedule.clone();
        let before = Before {
            active_polls: kani::any(),
            stepless_polls: kani::any(),
            asleep: kani::any(),
        };
        refilled.ended(on_the_way, 0, Outcome::NoReading, false, before);
        assert_eq!(refilled.budget(later), history.schedule.budget(later));
    }

    /// After any three sessions, a fourth, watched or not, takes its LED
    /// seconds out of the budget, down to nothing; and a reading, which
    /// ends an overdue spell, leaves no more than `LED_BUDGET`.
    #[kani::proof]
    #[kani::unwind(5)]
    fn a_session_spends_its_led_seconds() {
        let mut history = any_history(3);
        let (now, led_seconds): (u32, u32) = kani::any();
        kani::assume(history.now <= now && now < UPTIME);
        let had = history.schedule.budget(now);
        let outcome = if kani::any() {
            Outcome::Reading(kani::any())
        } else {
            Outcome::NoReading
        };
        let before = Before {
            active_polls: kani::any(),
            stepless_polls: kani::any(),
            asleep: kani::any(),
        };
        history
            .schedule
            .ended(now, led_seconds, outcome, kani::any(), before);

        let left = had.saturating_sub(led_seconds);
        let has = history.schedule.budget(now);
        match outcome {
            Outcome::Reading(_) => assert_eq!(has, left.min(LED_BUDGET)),
            _ => assert_eq!(has, left),
        }
    }

    /// After any five sessions: each resting estimate is none (0), or
    /// lies among the readings so far, as a median's estimate must. So
    /// it can't wander off where no reading has been, and "elevated" is
    /// always judged against a heart rate the wearer has had.
    #[kani::proof]
    #[kani::unwind(6)]
    fn the_resting_estimates_stay_among_the_readings() {
        let history = any_history(5);
        let (awake, asleep) = history.schedule.resting();
        match history.readings {
            None => assert_eq!((awake, asleep), (0, 0)),
            Some((low, high)) => {
                // (A reading of 0 BPM is none, and so is an estimate.)
                assert!(awake == 0 || (low..=high).contains(&awake));
                assert!(asleep == 0 || (low..=high).contains(&asleep));
            }
        }
    }

    /// After any three sessions: the tries up to a reading are one round,
    /// on one arm; the next round has the next number, and, awake, the
    /// other arm. Asleep sessions are on neither, and don't use up a
    /// round's turn.
    #[kani::proof]
    #[kani::unwind(5)]
    fn rounds_take_turns_between_the_arms() {
        let mut history = any_history(3);
        let schedule = &mut history.schedule;
        let before = Before::default();

        // A round's first awake try, a sleeping one, and another awake.
        let (round, arm) = schedule.begin(false);
        assert!(arm == Arm::Dim || arm == Arm::Bright);
        schedule.ended(history.now, 30, Outcome::NoReading, true, before);
        assert_eq!(schedule.begin(true), (round, Arm::Asleep));
        schedule.ended(history.now, 30, Outcome::OffWrist, true, before);
        assert_eq!(schedule.begin(false), (round, arm));

        // A reading ends it, watched or not. The next round starts asleep
        // (which chooses no arm), and its first awake try takes the turn.
        let reading = Outcome::Reading(kani::any());
        schedule.ended(history.now, 13, reading, kani::any(), before);
        let next = round.wrapping_add(1);
        if kani::any() {
            assert_eq!(schedule.begin(true), (next, Arm::Asleep));
        }
        let (again, other) = schedule.begin(false);
        assert_eq!(again, next);
        assert!(other != arm && other != Arm::Asleep);
    }

    /// For every drive setting, and every length of LED time up to ten
    /// minutes (a session's most is half of one): what it costs the budget is its charge in seconds at
    /// the lowest drive (12.5 mA), rounded up, so never under, and under
    /// a second over; whole seconds at the lowest drive cost themselves;
    /// and a brighter drive never costs less.
    ///
    /// A drive at a time (the loop), so that CBMC multiplies by each
    /// current as a constant, and no longer than ten minutes: for any
    /// drive at once, or every `u32` of milliseconds, it has a 64-bit
    /// division to turn inside out, and hadn't in five minutes.
    #[kani::proof]
    #[kani::unwind(5)]
    fn budget_seconds_never_undercount() {
        let millis: u32 = kani::any();
        kani::assume(millis <= 600_000);
        let mut dimmer = 0;
        for drive in 0..4 {
            let cost = budget_seconds(drive, millis);
            // In tenths of a µC: tenths of a mA × ms, against seconds at
            // 12.5 mA, each 125,000 of them.
            let charge = u64::from(millis) * u64::from(DRIVE_CURRENT[usize::from(drive)]);
            let paid = u64::from(cost) * 125_000;
            assert!(charge <= paid && paid < charge + 125_000);
            assert!(drive > 0 || millis % 1000 != 0 || cost == millis / 1000);
            assert!(dimmer <= cost);
            dimmer = cost;
        }
    }

    /// What `bright_start` promises of the drive it steps up to, if any:
    /// it's one or two settings up, and still a setting (at most 3); and
    /// `peak`, scaled with the current, stays within `BRIGHT_PEAK_LIMIT`
    /// there.
    fn steps_up_within_the_peak_limit(drive: u8, peak: u32, next: u8) {
        assert!(drive < next && next <= drive + 2 && next <= 3);
        let (now, then) = (
            u64::from(DRIVE_CURRENT[usize::from(drive)]),
            u64::from(DRIVE_CURRENT[usize::from(next)]),
        );
        assert!(u64::from(peak) * then <= u64::from(BRIGHT_PEAK_LIMIT) * now + now - 1);
    }

    /// For every drive setting, expected peak and budget: a session starts
    /// brighter only at a drive [`steps_up_within_the_peak_limit`] allows,
    /// and only if the budget covers a whole session there.
    ///
    /// The drive is a setting, 0..=3, as `heart` always passes: at 254
    /// and 255 `step_up`'s `drive + 1` and `drive + 2` overflow (and the
    /// proof says so, without this bound).
    #[kani::proof]
    #[kani::unwind(4)]
    fn starting_bright_stays_within_the_peak_and_the_budget() {
        let (drive, peak, budget): (u8, u32, u32) = kani::any();
        kani::assume(drive <= 3);
        if let Some(next) = bright_start(drive, peak, budget) {
            steps_up_within_the_peak_limit(drive, peak, next);
            assert!(budget >= budget_seconds(next, SESSION_RESERVE * 1000));
        }
    }

    /// For every count of busy checks among a session's: calm means busy
    /// at no more than a quarter of them, without overflowing.
    #[kani::proof]
    #[kani::unwind(5)]
    fn calm_is_busy_at_a_quarter_of_the_checks_at_most() {
        let (busy, checks): (u8, u8) = kani::any();
        assert_eq!(calm_throughout(busy, checks), busy <= checks / 4);
    }

    /// For every count of busy checks among a session's: hopeless means
    /// busy at half of them or more, without overflowing; and with any
    /// checks at all, a wrist is never both that and calm throughout.
    #[kani::proof]
    #[kani::unwind(5)]
    fn hopeless_is_busy_at_half_of_the_checks_at_least() {
        let (busy, checks): (u8, u8) = kani::any();
        assert_eq!(hopeless(busy, checks), busy >= checks.div_ceil(2));
        assert!(checks == 0 || !(hopeless(busy, checks) && calm_throughout(busy, checks)));
    }

    /// For every time and every bucket: the slot found is at or after the
    /// time asked from, less than a bucket (and its slot's length) later,
    /// in a bucket's slot, and the same if asked again from there. (Times
    /// within ten years of boot, as everywhere here; the bucket's start
    /// may be any `u32`.)
    #[kani::proof]
    #[kani::unwind(2)]
    fn the_slot_found_is_the_next_one() {
        let (earliest, bucket): (u32, u32) = kani::any();
        kani::assume(earliest < UPTIME);
        let at = in_phase(earliest, bucket);
        assert!(earliest <= at && at <= earliest + BUCKET - BUCKET_SLOT_FOR);
        let into = (at % BUCKET + BUCKET - bucket % BUCKET) % BUCKET;
        assert!(BUCKET_SLOT <= into && into < BUCKET_SLOT + BUCKET_SLOT_FOR);
        assert_eq!(in_phase(at, bucket), at);
        // And none was passed over on the way, but the one `earliest`
        // was already too far into.
        if at != earliest {
            assert_eq!(into, BUCKET_SLOT);
        }
    }

    // ==========================================================================
    // From any state
    // ==========================================================================
    //
    // The proofs above start from a boot, so they say what holds after a
    // few sessions. These start from any schedule whose fields agree with
    // each other (`Schedule::consistent`), and the first shows that every
    // session leaves it so: by induction, what the others prove holds
    // after any number of sessions, on any day.

    /// A schedule in any consistent state, told of nothing later than
    /// `UPTIME`.
    fn any_consistent() -> Schedule {
        let schedule = Schedule::any(|| kani::any());
        kani::assume(schedule.consistent() && schedule.as_of() < UPTIME);
        schedule
    }

    /// Any session's end, at `now`: its outcome, whether the schedule
    /// started it, and what the wrist had been doing.
    fn any_end() -> (Outcome, bool, Before) {
        let outcome = match kani::any::<u8>() & 3 {
            0 => Outcome::Reading(kani::any()),
            1 => Outcome::NoReading,
            2 => Outcome::CalmNoReading,
            _ => Outcome::OffWrist,
        };
        let before = Before {
            active_polls: kani::any(),
            stepless_polls: kani::any(),
            asleep: kani::any(),
        };
        (outcome, kani::any(), before)
    }

    /// A boot is consistent, and from any consistent state, any session
    /// (begun first, if it's the schedule's) leaves the schedule
    /// consistent, and told of nothing later than its end: the induction
    /// step for everything below. (`ended` asserts as much itself, with
    /// debug assertions; this says so when they're off, too.)
    #[kani::proof]
    #[kani::unwind(2)]
    fn every_session_leaves_the_schedule_consistent() {
        assert!(Schedule::new().consistent());
        assert_eq!(Schedule::new().as_of(), 0);

        let mut schedule = any_consistent();
        let (now, led_seconds): (u32, u32) = kani::any();
        kani::assume(schedule.as_of() <= now && now < UPTIME);
        let (outcome, background, before) = any_end();
        if background {
            let _ = schedule.begin(before.asleep);
            assert!(schedule.consistent());
        }
        schedule.ended(now, led_seconds, outcome, background, before);
        assert!(schedule.consistent());
        assert!(schedule.as_of() <= now);
    }

    /// From any consistent state, after any counted session (a background
    /// one, or one with a reading), whatever the wrist does next: the
    /// next session is due at least `MIN_INTERVAL` after it, and, unless
    /// the budget holds it back, at most `MAX_FAILED_INTERVAL` after; the
    /// budget holds a session's worth by then, and holds it back no longer
    /// than it takes to earn one; and the reason is what the session and
    /// the wrist make it (as `why_a_session_is_due_is_what_happened` has
    /// it, but for what only the history knows: the resting estimate, and
    /// when the last reading came, if this session had none).
    #[kani::proof]
    #[kani::unwind(2)]
    fn after_any_session_the_next_is_two_minutes_to_an_hour_on() {
        let mut schedule = any_consistent();
        let (now, led_seconds): (u32, u32) = kani::any();
        kani::assume(schedule.as_of() <= now && now < UPTIME);
        let (outcome, background, before) = any_end();
        let read = matches!(outcome, Outcome::Reading(_));
        kani::assume(background || read);
        schedule.ended(now, led_seconds, outcome, background, before);

        let (active_polls, asleep): (u8, bool) = kani::any();
        let (at, why) = schedule.due(active_polls, asleep);
        let moved = !asleep && active_polls >= MOVED_POLLS;
        assert!(at >= now + MIN_INTERVAL);
        assert!(schedule.budget(at) >= SESSION_RESERVE);
        match why {
            Why::First => unreachable!("a session has been counted"),
            Why::Budget => {
                assert!(schedule.budget(at - 1) < SESSION_RESERVE);
                assert!(at < now + SESSION_RESERVE * 3600 / LED_BUDGET + 6);
            }
            Why::Moved => assert!(read && moved && at == now + MIN_INTERVAL),
            Why::Elevated => assert!(read && !moved && at == now + ELEVATED_INTERVAL),
            Why::Asleep => assert!(read && asleep && at <= now + ASLEEP_INTERVAL),
            Why::Rest => assert!(read && !asleep && !moved && at <= now + MAX_INTERVAL),
            Why::Retry => assert!(!read && at <= now + MAX_FAILED_INTERVAL),
            // Sooner than the retry would be, but never within
            // `OVERDUE_RETRY` of the failure.
            Why::Overdue => {
                assert!(!read && outcome != Outcome::OffWrist);
                assert!(at >= now + OVERDUE_RETRY && at < now + MAX_FAILED_INTERVAL);
            }
            Why::Calm => unreachable!("only `heart` says so, of a sought session"),
        }
        if why != Why::Budget {
            assert!(read || !moved || at <= now + MAX_INTERVAL);
            assert!(outcome != Outcome::CalmNoReading || at <= now + CALM_RETRY_INTERVAL);
        }
    }

    /// From any consistent state, after any counted session: timing the
    /// next to the phone's buckets (`due_in_phase`) changes nothing awake,
    /// or without the time. Asleep, the session is still due at least
    /// `MIN_INTERVAL` after the last, and not before the budget holds a
    /// session's worth; and unless the budget holds it back, it's due in
    /// a bucket's slot, no more than 3 minutes sooner than it would have
    /// been and no more than 2 minutes 40 later.
    #[kani::proof]
    #[kani::unwind(2)]
    fn an_asleep_session_is_due_in_a_slot_near_when_it_would_be() {
        let mut schedule = any_consistent();
        let (now, led_seconds): (u32, u32) = kani::any();
        kani::assume(schedule.as_of() <= now && now < UPTIME);
        let (outcome, background, before) = any_end();
        kani::assume(background || matches!(outcome, Outcome::Reading(_)));
        schedule.ended(now, led_seconds, outcome, background, before);

        let (active_polls, asleep, bucket): (u8, bool, u32) = kani::any();
        let (at, why) = schedule.due(active_polls, asleep);
        assert_eq!(schedule.due_in_phase(active_polls, asleep, None), (at, why));
        let (phased, phased_why) = schedule.due_in_phase(active_polls, asleep, Some(bucket));
        if !asleep {
            assert_eq!((phased, phased_why), (at, why));
            return;
        }
        assert!(phased >= now + MIN_INTERVAL);
        assert!(schedule.budget(phased) >= SESSION_RESERVE);
        if phased_why == Why::Budget {
            return;
        }
        let into = (phased % BUCKET + BUCKET - bucket % BUCKET) % BUCKET;
        assert!(BUCKET_SLOT <= into && into < BUCKET_SLOT + BUCKET_SLOT_FOR);
        // (Where the budget held the unphased session back, a later slot
        // may be one it no longer does; then there's no "would be" to be
        // near.)
        if why != Why::Budget {
            assert_eq!(phased_why, why);
            assert!(phased + 180 >= at && phased <= at + 160);
        }
    }

    /// The wait for a session, as `heart::heart_task` makes it between
    /// sessions with no one watching: one look at the schedule at `now`,
    /// with `due_since` kept between looks. `None` to look again later,
    /// or how the session starts.
    ///
    /// TODO: a copy of the task's loop, which Kani can't reach (it's
    /// async, and reads the wrist from `activity`'s statics). Were the
    /// loop's decision a function of `heart_schedule`, this would call it.
    fn look(
        schedule: &Schedule,
        now: u32,
        due_since: &mut Option<u32>,
        (active_polls, asleep, calm_polls): (u8, bool, u8),
    ) -> Option<Start> {
        let (at, _) = schedule.due(active_polls, asleep);
        if now < at {
            *due_since = None;
            return None;
        }
        let since = *due_since.get_or_insert(now);
        match schedule.start(now, since, calm_polls) {
            Start::Wait => None,
            start => Some(start),
        }
    }

    /// From any consistent state: movement only ever brings the next
    /// session forward (so does a wrist that stops being asleep, or
    /// starts, by no more than to the later of the two), so there's a time
    /// from which it's due whatever the wrist does. From then on a wait
    /// isn't started afresh: a look that finds it due, and doesn't start
    /// it, keeps since when; and any look `CALM_WAIT` after that starts it,
    /// however busy the wrist.
    /// So no wrist can hold a session back for longer than `CALM_WAIT`
    /// (and the 3 s between looks) past that time, as one did for 18
    /// minutes before 0.10.0-rc.5.
    #[kani::proof]
    #[kani::unwind(2)]
    fn no_wrist_holds_a_due_session_back_for_long() {
        let schedule = any_consistent();
        let wrist = || -> (u8, bool, u8) { kani::any() };

        // Movement never puts a session off.
        let (active_polls, asleep, _) = wrist();
        let (at, _) = schedule.due(active_polls, asleep);
        assert!(at <= schedule.due(0, asleep).0);
        let whatever = schedule.due(0, false).0.max(schedule.due(0, true).0);
        assert!(at <= whatever);

        // A look from then on, at a session found due at or before it (or
        // not yet found due).
        let (first, second): (u32, u32) = kani::any();
        kani::assume(whatever <= first && first <= second && second < UPTIME);
        let mut due_since: Option<u32> = kani::any();
        kani::assume(due_since.is_none_or(|since| since <= first));
        let found = due_since.unwrap_or(first);
        if look(&schedule, first, &mut due_since, wrist()).is_some() {
            return;
        }
        assert_eq!(due_since, Some(found));

        // And a later one.
        let start = look(&schedule, second, &mut due_since, wrist());
        assert_eq!(due_since, Some(found));
        if second - found >= CALM_WAIT {
            assert!(start.is_some());
        }
    }

    /// From any consistent state, the charger (see `Schedule::charger`)
    /// leaves the schedule consistent, and changes nothing but what it
    /// says it does. On it, settling after it, or away from it, when the
    /// next session is due and why, and the budget, are as they were; and
    /// whether it's on is whether its power is present. Taken off it, the
    /// next session is due `MIN_INTERVAL` after the power went, unless the
    /// budget holds it back (no sooner, either way), after
    /// `CHARGER_SETTLE`. A session the charger cut short (`cut_short`)
    /// takes its LED seconds out of the budget, and is, to when the next
    /// is due, a watched session without a reading.
    #[kani::proof]
    #[kani::unwind(2)]
    fn the_charger_changes_only_what_it_says() {
        let mut schedule = any_consistent();
        let (now, present, led_seconds): (u32, bool, u32) = kani::any();
        kani::assume(schedule.as_of() <= now && now < UPTIME);
        let (active_polls, asleep): (u8, bool) = kani::any();
        let before = schedule.clone();
        let (was_at, was_why) = before.due(active_polls, asleep);

        if kani::any() {
            schedule.cut_short(now, led_seconds);
            assert!(schedule.consistent() && schedule.as_of() <= now);
            let left = before.budget(now).saturating_sub(led_seconds);
            assert_eq!(schedule.budget(now), left);
            // To when the next is due, it's a watched session without a
            // reading: the LED time spent, and nothing else.
            let mut watched = before.clone();
            let wrist = Before {
                active_polls,
                stepless_polls: kani::any(),
                asleep,
            };
            watched.ended(now, led_seconds, Outcome::NoReading, false, wrist);
            assert_eq!(
                schedule.due(active_polls, asleep),
                watched.due(active_polls, asleep)
            );
            return;
        }

        let charger = schedule.charger(now, present);
        assert!(schedule.consistent() && schedule.as_of() <= now);
        assert_eq!(
            present,
            matches!(charger, Charger::Arrived | Charger::On | Charger::Back)
        );
        assert_eq!(schedule.budget(now), before.budget(now));
        let (at, why) = schedule.due(active_polls, asleep);
        match charger {
            Charger::Left(wait) => {
                assert_eq!(wait, CHARGER_SETTLE);
                assert!(at >= now + MIN_INTERVAL);
                if why != Why::Budget {
                    assert_eq!(at, now + MIN_INTERVAL);
                }
            }
            Charger::Settling(wait) => {
                assert!(0 < wait && wait <= CHARGER_SETTLE);
                assert_eq!((at, why), (was_at, was_why));
            }
            Charger::Away | Charger::Arrived | Charger::On | Charger::Back => {
                assert_eq!((at, why), (was_at, was_why));
            }
        }
    }

    /// From any consistent state, between any two later times: the budget
    /// never passes `OVERDUE_BUDGET`, never shrinks with time alone, and
    /// earns no faster than a second every 6 s (the overdue rate; a
    /// second every 12 s for as long as no reading is overdue, which
    /// `the_budget_fills_at_its_rate_up_to_its_cap` shows from a boot,
    /// where it knows when the last reading came). And for half an hour
    /// after a reading, it holds no more than `LED_BUDGET`.
    #[kani::proof]
    #[kani::unwind(2)]
    fn the_budget_stays_within_its_caps_from_any_state() {
        let mut schedule = any_consistent();
        let (earlier, later): (u32, u32) = kani::any();
        kani::assume(schedule.as_of() <= earlier && earlier <= later && later < UPTIME);
        let (before, after) = (schedule.budget(earlier), schedule.budget(later));
        assert!(before <= after && after <= OVERDUE_BUDGET);
        assert!(after - before <= (later - earlier).div_ceil(3600 / OVERDUE_BUDGET));

        let before = Before {
            active_polls: kani::any(),
            stepless_polls: kani::any(),
            asleep: kani::any(),
        };
        let reading = Outcome::Reading(kani::any());
        schedule.ended(earlier, kani::any(), reading, kani::any(), before);
        if later <= earlier + OVERDUE {
            assert!(schedule.budget(later) <= LED_BUDGET);
        }
    }

    /// From any consistent state: calm is never sought asleep; and when
    /// it is, not before a reading is `SEEK_AFTER` late, nor within
    /// `SEEK_GAP` of the last session, nor before the budget holds a
    /// session's worth (at any time from then on, as nothing but a
    /// session spends it).
    #[kani::proof]
    #[kani::unwind(2)]
    fn calm_is_only_sought_late_apart_and_within_the_budget() {
        let mut schedule = any_consistent();
        assert!(schedule.seek(true).is_none());
        // A session, so that the times it's counted from are known.
        let (now, later): (u32, u32) = kani::any();
        kani::assume(schedule.as_of() <= now && now <= later && later < UPTIME);
        let (outcome, background, before) = any_end();
        schedule.ended(now, kani::any(), outcome, background, before);
        let Some(from) = schedule.seek(false) else {
            return;
        };
        if background || matches!(outcome, Outcome::Reading(_)) {
            assert!(from >= now + SEEK_GAP);
            // No skin, or a calm failure: what was missing wasn't calm.
            assert!(!matches!(
                outcome,
                Outcome::OffWrist | Outcome::CalmNoReading
            ));
        }
        if matches!(outcome, Outcome::Reading(_)) {
            assert!(from >= now + SEEK_AFTER);
        }
        if from <= later {
            assert!(schedule.budget(later) >= SESSION_RESERVE);
        }
    }

    /// From any consistent state: a session takes its LED seconds out of
    /// the budget, down to nothing (and a reading leaves no more than
    /// `LED_BUDGET`); and one that spends nothing and counts for nothing
    /// (watched, without a reading) changes nothing about what the budget
    /// holds later, so bringing it up to date often loses no time to
    /// rounding.
    #[kani::proof]
    #[kani::unwind(2)]
    fn the_budget_is_spent_and_refilled_exactly_from_any_state() {
        let schedule = any_consistent();
        let (now, later, led_seconds): (u32, u32, u32) = kani::any();
        kani::assume(schedule.as_of() <= now && now <= later && later < UPTIME);
        let (outcome, background, before) = any_end();

        let mut spent = schedule.clone();
        spent.ended(now, led_seconds, outcome, background, before);
        let left = schedule.budget(now).saturating_sub(led_seconds);
        match outcome {
            Outcome::Reading(_) => assert_eq!(spent.budget(now), left.min(LED_BUDGET)),
            _ => assert_eq!(spent.budget(now), left),
        }

        let mut refilled = schedule.clone();
        refilled.ended(now, 0, Outcome::NoReading, false, before);
        assert_eq!(refilled.budget(later), schedule.budget(later));
    }
}
