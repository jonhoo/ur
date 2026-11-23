//! Host tests for the firmware's `workout` module (`cargo test`), and a
//! replay of a decoded field log through it, to tune the rules against
//! real days:
//!
//!     cargo run --release -- decoded.log...
//!
//! The log's accelerometer polls (`activity: N samples, movement M, K new
//! steps`, at `debug`) are gathered into minutes as `activity` gathers
//! them, and each minute goes through [`workout::detect`]. One line per
//! workout found: when (UTC), how long, its kinds, its steps and mean
//! movement a minute, and the heart rate readings the log has inside it
//! (how many, and their range). A log made by a build with workouts in it
//! says `workout: … started` itself; this says what the rules as they are
//! now would have made of the same wrist.

#[path = "../../../src/workout.rs"]
#[allow(dead_code)]
mod workout;

use std::collections::BTreeMap;

use workout::{Kind, Minute, WINDOW, detect};

/// What a log says of one minute.
#[derive(Default, Clone, Copy)]
struct Logged {
    steps: u64,
    /// Movement summed over the polls' samples, and those samples: the
    /// minute's mean weights each poll by them, as `activity` does.
    moved: u64,
    samples: u64,
    asleep: bool,
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        println!("run `cargo test`, or pass decoded field logs to replay");
        return;
    }
    let mut minutes: BTreeMap<i64, Logged> = BTreeMap::new();
    // Readings by UTC second.
    let mut readings: BTreeMap<i64, u8> = BTreeMap::new();
    for path in &paths {
        let text = std::fs::read(path).expect("a readable log");
        let text = String::from_utf8_lossy(&text);
        // (uptime seconds, UTC seconds), from `clock: <utc> UTC = ...`.
        let mut anchor: Option<(f64, i64)> = None;
        let mut asleep = false;
        for line in text.lines() {
            let Some((uptime, rest)) = line.split_once(' ') else {
                continue;
            };
            let Ok(uptime) = uptime.parse::<f64>() else {
                continue;
            };
            let message = rest.trim_start_matches(|c: char| c.is_ascii_uppercase() || c == ' ');
            if message.contains("booting") {
                // Uptime starts over; the classifier starts awake.
                anchor = None;
                asleep = false;
            }
            if let Some(rest) = message.strip_prefix("clock: ") {
                if let Some(utc) = rest.split(' ').next().and_then(|u| u.parse::<i64>().ok()) {
                    anchor = Some((uptime, utc));
                }
                continue;
            }
            match message {
                "sleep: asleep now" => asleep = true,
                "sleep: awake now" => asleep = false,
                _ => {}
            }
            let Some(utc) = anchor.map(|(at, utc)| utc + (uptime - at) as i64) else {
                continue;
            };
            // "heart: 56 BPM after 24 s (...)", or "heart: 56 BPM" for a
            // watched session's later ones.
            if let Some(rest) = message.strip_prefix("heart: ")
                && let Some((bpm, _)) = rest.split_once(" BPM")
                && let Ok(bpm) = bpm.parse::<u8>()
            {
                readings.insert(utc, bpm);
                continue;
            }
            // "activity: 24 samples, movement 1653, 0 new steps (counter 0)".
            // Only these: the task's warnings carry numbers too.
            let Some(rest) = message
                .strip_prefix("activity: ")
                .filter(|rest| rest.contains(" samples, movement "))
            else {
                continue;
            };
            let numbers: Vec<u64> = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse().ok())
                .collect();
            let [samples, movement, steps, ..] = numbers[..] else {
                continue;
            };
            let minute = minutes.entry(utc.div_euclid(60)).or_default();
            minute.steps += steps;
            minute.moved += movement * samples;
            minute.samples += samples;
            // As of the minute's last poll: the firmware asks at the next
            // minute's first, a moment later.
            minute.asleep = asleep;
        }
    }

    let (Some((&first, _)), Some((&last, _))) =
        (minutes.first_key_value(), minutes.last_key_value())
    else {
        println!("no accelerometer polls in the log (is `ur::activity` at debug?)");
        return;
    };
    let mut recent = [Minute::default(); WINDOW];
    let mut current = None;
    // The workout under way: the minute it was found in, and every kind
    // it has been.
    let mut under_way = None::<(i64, Vec<Kind>)>;
    let (mut workouts, mut total) = (0, 0);
    // A minute the log doesn't have is an empty one, as in `activity`'s
    // history (the watch was off, or the log has a hole).
    for minute in first..=last + 1 {
        let logged = minutes.get(&minute).copied().unwrap_or_default();
        recent.rotate_left(1);
        recent[WINDOW - 1] = Minute {
            steps: logged.steps.min(u64::from(u16::MAX)) as u16,
            movement: (logged.moved / logged.samples.max(1)).min(u64::from(u16::MAX)) as u16,
        };
        let now = if minute > last {
            None
        } else {
            detect(current, &recent, logged.asleep)
        };
        match (&mut under_way, now) {
            (None, Some(kind)) => under_way = Some((minute, vec![kind])),
            (Some((_, kinds)), Some(kind)) => {
                if kinds.last() != Some(&kind) {
                    kinds.push(kind);
                }
            }
            (Some((since, kinds)), None) => {
                // Found at the end of minute `since`, ended at the end of
                // this one.
                let span = *since + 1..=minute;
                let length = minute - *since;
                let (steps, moved, samples) = minutes.range(span.clone()).fold(
                    (0, 0, 0),
                    |(steps, moved, samples), (_, m)| {
                        (steps + m.steps, moved + m.moved, samples + m.samples)
                    },
                );
                let read: Vec<u8> = readings
                    .range((*since + 1) * 60..(minute + 1) * 60)
                    .map(|(_, &bpm)| bpm)
                    .collect();
                println!(
                    "{} UTC, {length:3} min, {kinds:?}: {} steps/min, movement {}; {} readings{}",
                    utc(*since + 1),
                    steps / length.max(1) as u64,
                    moved / samples.max(1),
                    read.len(),
                    match (read.iter().min(), read.iter().max()) {
                        (Some(low), Some(high)) => format!(", {low}-{high} BPM"),
                        _ => String::new(),
                    }
                );
                workouts += 1;
                total += length;
                under_way = None;
            }
            (None, None) => {}
        }
        current = now;
    }
    let days = minutes.len() as f64 / 1440.0;
    println!(
        "{workouts} workouts, {total} minutes, in {days:.1} days of logged minutes ({:.1} and {:.0} a day)",
        f64::from(workouts) / days,
        total as f64 / days
    );
}

/// A UTC minute as `MM-DD HH:MM` (the year is the log's business).
fn utc(minute: i64) -> String {
    // Days since 1970 to a civil date (Howard Hinnant's algorithm).
    let days = minute.div_euclid(1440);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let _ = era;
    let of_day = minute.rem_euclid(1440);
    format!("{month:02}-{day:02} {:02}:{:02}", of_day / 60, of_day % 60)
}

#[cfg(test)]
mod tests {
    use super::workout::*;

    const STILL: Minute = Minute {
        steps: 0,
        movement: 30,
    };
    const DESK: Minute = Minute {
        steps: 0,
        movement: 600,
    };
    const POTTERING: Minute = Minute {
        steps: 30,
        movement: 1400,
    };
    const WALKING: Minute = Minute {
        steps: 110,
        movement: 3800,
    };
    const LIFTING: Minute = Minute {
        steps: 12,
        movement: 2600,
    };

    /// Runs `minutes` through [`detect`] from no workout, after a still
    /// window; what it said after each.
    fn run(minutes: &[Minute]) -> Vec<Option<Kind>> {
        run_asleep(minutes, |_| false)
    }

    fn run_asleep(minutes: &[Minute], asleep: impl Fn(usize) -> bool) -> Vec<Option<Kind>> {
        let mut recent = [STILL; WINDOW];
        let mut current = None;
        minutes
            .iter()
            .enumerate()
            .map(|(i, &minute)| {
                recent.rotate_left(1);
                recent[WINDOW - 1] = minute;
                current = detect(current, &recent, asleep(i));
                current
            })
            .collect()
    }

    fn repeat(minute: Minute, times: usize) -> Vec<Minute> {
        vec![minute; times]
    }

    #[test]
    fn a_walk_starts_after_three_minutes_and_ends_three_after_it_stops() {
        let day = [repeat(DESK, 5), repeat(WALKING, 20), repeat(DESK, 10)].concat();
        let said = run(&day);
        // Not yet at the walk's second minute; from its third.
        assert_eq!(said[6], None);
        assert_eq!(said[7], Some(Kind::Walk));
        // Through the walk, and two minutes of the desk after it: the
        // recovery.
        assert!(said[7..27].iter().all(|&kind| kind == Some(Kind::Walk)));
        assert_eq!(said[27], None);
        assert!(said[27..].iter().all(Option::is_none));
    }

    #[test]
    fn a_red_light_does_not_end_a_walk() {
        let slow = Minute {
            steps: 20,
            movement: 900,
        };
        let day = [
            repeat(WALKING, 6),
            repeat(slow, 2),
            repeat(WALKING, 6),
            repeat(DESK, 3),
        ]
        .concat();
        let said = run(&day);
        assert!(said[2..16].iter().all(|&kind| kind == Some(Kind::Walk)));
        assert_eq!(said[16], None);
    }

    #[test]
    fn pottering_and_a_desk_are_no_workout() {
        let day = [
            repeat(DESK, 60),
            repeat(POTTERING, 60),
            // A few steps across a room at walking pace, never three
            // minutes of it.
            [WALKING, WALKING, DESK].repeat(10),
        ]
        .concat();
        assert!(run(&day).iter().all(Option::is_none));
    }

    #[test]
    fn hard_work_without_steps_starts_at_eight_minutes_of_ten() {
        let day = [repeat(DESK, 5), repeat(LIFTING, 20), repeat(STILL, 10)].concat();
        let said = run(&day);
        assert_eq!(said[11], None);
        assert_eq!(said[12], Some(Kind::Effort));
        // A rest between sets doesn't end it; five still minutes do.
        assert!(said[12..29].iter().all(|&kind| kind == Some(Kind::Effort)));
        assert_eq!(said[29], None);
    }

    #[test]
    fn rests_between_sets_keep_an_effort_going() {
        let mut day = repeat(LIFTING, 10);
        for _ in 0..5 {
            day.extend(repeat(STILL, 3));
            day.push(LIFTING);
        }
        day.extend(repeat(STILL, 5));
        let said = run(&day);
        assert!(said[7..30].iter().all(|&kind| kind == Some(Kind::Effort)));
        assert_eq!(said.last(), Some(&None));
    }

    #[test]
    fn an_effort_lapses_when_the_hard_minutes_stop_coming() {
        // Up and about after it, never quiet: over ten minutes after the
        // last hard one.
        let day = [repeat(LIFTING, 10), repeat(POTTERING, 30)].concat();
        let said = run(&day);
        assert!(said[7..19].iter().all(|&kind| kind == Some(Kind::Effort)));
        assert!(said[19..].iter().all(Option::is_none));
        // One hard minute in ten keeps it going, and one alone afterwards
        // doesn't start it again.
        let mut day = repeat(LIFTING, 10);
        for _ in 0..3 {
            day.extend(repeat(POTTERING, 9));
            day.push(LIFTING);
        }
        day.extend(repeat(POTTERING, 12));
        day.push(LIFTING);
        day.extend(repeat(POTTERING, 5));
        let said = run(&day);
        assert!(said[7..49].iter().all(|&kind| kind == Some(Kind::Effort)));
        assert!(said[49..].iter().all(Option::is_none));
    }

    // ------------------------------------------------------------------
    // The strength session of 2026-11-22
    // ------------------------------------------------------------------
    //
    // The one strength session a field log has (0.12.0-rc.1, the sealed
    // watch): each minute's mean movement and steps from 19:12 UTC, 85
    // minutes, as this crate's replay gathers them from the log.
    //
    // - 19:19-19:27 (from index 7): arriving and setting up, nine hard
    //   minutes. The watch said Effort as 19:26 ended.
    // - 19:27:44-19:54:13 (15 to 42): the session, by the training app's
    //   record. Sets and rests: 9 minutes of 27 are hard.
    // - 19:55-20:02 (43 to 50): packing up and leaving, all hard.
    // - 20:03 on (51): moving about at 540-1900 with one hard minute
    //   (20:22, index 70), then sitting down at 20:28 (76). The watch,
    //   which then ended an effort only on five quiet minutes in a row,
    //   was still measuring at 20:36, where the log ends.

    /// The session's first minute, and its last.
    const SESSION: std::ops::RangeInclusive<usize> = 15..=42;
    /// The last hard minute of leaving.
    const LEFT: usize = 50;

    /// As the watch cut the minutes. The longest stretch without a hard
    /// minute in the session is 4.
    #[rustfmt::skip]
    const STRENGTH: [(u16, u16); 85] = [
        (893, 16), (378, 0), (779, 0), (1515, 8), (1233, 0), (1085, 0),
        (1232, 6), (3103, 30), (3535, 26), (2647, 26), (2753, 64), (4621, 74),
        (4987, 128), (3944, 56), (5459, 52), (2567, 56), (1784, 22), (879, 10),
        (1794, 28), (2028, 10), (900, 0), (1050, 0), (1040, 16), (806, 6),
        (2057, 12), (2565, 46), (1246, 40), (600, 0), (2450, 28), (2212, 36),
        (1120, 12), (2613, 12), (1777, 6), (616, 0), (2674, 38), (1486, 10),
        (1645, 30), (1513, 40), (2601, 24), (1890, 28), (1749, 42), (1325, 6),
        (2285, 30), (2834, 80), (4034, 38), (6310, 42), (3608, 66), (4440, 116),
        (2672, 80), (2934, 18), (3247, 2), (1164, 22), (1189, 0), (1238, 20),
        (782, 0), (1236, 0), (539, 0), (1635, 24), (1463, 18), (1186, 6),
        (867, 0), (1011, 0), (803, 0), (1049, 12), (1094, 8), (1431, 16),
        (1581, 0), (1506, 0), (980, 0), (1078, 14), (4356, 42), (1349, 28),
        (1857, 12), (1865, 18), (1854, 26), (1101, 26), (555, 0), (679, 0),
        (416, 0), (247, 0), (1455, 0), (268, 0), (317, 0), (315, 0),
        (313, 0),
    ];

    /// The same polls with each minute cut 4 s earlier, which is as
    /// likely a place for the watch's clock to put them: the borderline
    /// minutes fall the other way (2028 becomes 1914, 2057 becomes 1773),
    /// and the session's first nine minutes have no hard one.
    #[rustfmt::skip]
    const STRENGTH_CUT_EARLIER: [(u16, u16); 85] = [
        (969, 22), (418, 0), (783, 0), (1419, 8), (1315, 0), (990, 0),
        (1243, 6), (3070, 30), (3410, 26), (2826, 24), (2357, 58), (4720, 72),
        (5026, 132), (4033, 60), (5293, 52), (2861, 58), (1918, 22), (793, 10),
        (1781, 28), (1914, 10), (961, 0), (1177, 0), (988, 16), (790, 6),
        (1773, 10), (2793, 48), (1409, 40), (476, 0), (2432, 28), (2248, 30),
        (1099, 18), (2662, 12), (1858, 6), (542, 0), (2649, 34), (1508, 14),
        (1575, 30), (1579, 40), (2568, 24), (1885, 22), (1677, 48), (1328, 6),
        (2393, 30), (2767, 74), (3749, 38), (6400, 44), (3650, 62), (4504, 116),
        (2747, 88), (2956, 12), (3205, 8), (1224, 22), (1204, 0), (1283, 20),
        (727, 0), (1280, 0), (469, 0), (1770, 24), (1187, 18), (1421, 6),
        (840, 0), (1044, 0), (836, 0), (1033, 12), (1078, 8), (1368, 16),
        (1558, 0), (1516, 0), (1075, 0), (1024, 14), (4393, 42), (1254, 28),
        (1937, 12), (1801, 18), (1837, 26), (1237, 26), (537, 0), (692, 0),
        (412, 0), (182, 0), (1477, 0), (312, 0), (320, 0), (313, 0),
        (314, 0),
    ];

    /// What [`detect`] says of a field log's minutes, each its mean
    /// movement and its steps.
    fn replay(minutes: &[(u16, u16)]) -> Vec<Option<Kind>> {
        let minutes: Vec<Minute> = minutes
            .iter()
            .map(|&(movement, steps)| Minute { steps, movement })
            .collect();
        run(&minutes)
    }

    #[test]
    fn the_strength_session_starts_as_the_watch_started_it() {
        for minutes in [&STRENGTH, &STRENGTH_CUT_EARLIER] {
            let said = replay(minutes);
            // Not on the way in; as 19:26 ends, a minute before the first
            // set. Never a walk: 128 steps came in one minute, not three.
            assert!(said[..14].iter().all(Option::is_none));
            assert_eq!(said[14], Some(Kind::Effort));
            assert!(!said.contains(&Some(Kind::Walk)));
        }
    }

    #[test]
    fn the_strength_session_lasts_through_its_rests() {
        for minutes in [&STRENGTH, &STRENGTH_CUT_EARLIER] {
            let said = replay(minutes);
            assert!(
                said[SESSION].iter().all(|&kind| kind == Some(Kind::Effort)),
                "{said:?}"
            );
        }
    }

    #[test]
    fn the_strength_session_ends_ten_minutes_after_the_wearer_leaves() {
        for minutes in [&STRENGTH, &STRENGTH_CUT_EARLIER] {
            let said = replay(minutes);
            // Packing up is as hard as lifting, and part of it; then ten
            // minutes of moving about, none of them quiet enough to have
            // ended it before.
            assert!(
                said[*SESSION.end()..LEFT + EFFORT_LAPSE_MINUTES]
                    .iter()
                    .all(|&kind| kind == Some(Kind::Effort))
            );
            // And over from there on: the hard minute at 20:22 doesn't
            // start another.
            assert!(
                said[LEFT + EFFORT_LAPSE_MINUTES..]
                    .iter()
                    .all(Option::is_none),
                "{said:?}"
            );
        }
    }

    #[test]
    fn a_walk_takes_over_from_an_effort_which_starts_afresh_after_it() {
        // Working, then a walk, then working on.
        let day = [
            repeat(LIFTING, 10),
            repeat(WALKING, 10),
            repeat(LIFTING, 10),
        ]
        .concat();
        let said = run(&day);
        assert_eq!(said[9], Some(Kind::Effort));
        assert!(said[12..22].iter().all(|&kind| kind == Some(Kind::Walk)));
        // The walk's steps have stopped for three minutes. Its own
        // minutes moved hard enough to count, and don't: three of lifting
        // aren't an effort yet.
        assert!(said[22..27].iter().all(Option::is_none), "{said:?}");
        // Eight are, as from a desk.
        assert!(said[27..].iter().all(|&kind| kind == Some(Kind::Effort)));
    }

    #[test]
    fn a_walk_then_hard_work_without_steps_becomes_an_effort() {
        // Walking to the gym, then lifting: the effort is found by its
        // own minutes, eight after the walking, with the walk over three
        // minutes after its steps.
        let day = [repeat(DESK, 5), repeat(WALKING, 15), repeat(LIFTING, 30)].concat();
        let said = run(&day);
        assert!(said[7..22].iter().all(|&kind| kind == Some(Kind::Walk)));
        assert!(said[22..27].iter().all(Option::is_none), "{said:?}");
        assert!(said[27..].iter().all(|&kind| kind == Some(Kind::Effort)));
        // A walk that slows to a stop-and-go shuffle and never pauses
        // (so it's still under way) hands over directly once that's so.
        let shuffling = Minute {
            steps: 50,
            movement: 2600,
        };
        let day = [
            repeat(WALKING, 10),
            repeat(shuffling, 8),
            repeat(LIFTING, 5),
        ]
        .concat();
        let said = run(&day);
        assert!(said[2..20].iter().all(|&kind| kind == Some(Kind::Walk)));
        assert!(said[20..].iter().all(|&kind| kind == Some(Kind::Effort)));
    }

    #[test]
    fn hard_minutes_count_from_the_last_run_of_walking() {
        let window =
            |minutes: &[Minute]| -> [Minute; WINDOW] { minutes.try_into().expect("ten minutes") };
        // No walking: all of them.
        assert_eq!(hard_since_walking(&[LIFTING; WINDOW]), 10);
        assert_eq!(hard_since_walking(&[DESK; WINDOW]), 0);
        // A minute or two at pace is crossing a room, and counts as the
        // hard minute it is.
        let crossing = [repeat(LIFTING, 4), repeat(WALKING, 2), repeat(LIFTING, 4)].concat();
        assert_eq!(hard_since_walking(&window(&crossing)), 10);
        // Three in a row is a walk: only what follows it counts.
        let walked = [repeat(LIFTING, 3), repeat(WALKING, 3), repeat(LIFTING, 4)].concat();
        assert_eq!(hard_since_walking(&window(&walked)), 4);
        // The last such run, and a walk to the window's end leaves none.
        let twice = [
            repeat(WALKING, 3),
            repeat(LIFTING, 2),
            repeat(WALKING, 4),
            [LIFTING].to_vec(),
        ]
        .concat();
        assert_eq!(hard_since_walking(&window(&twice)), 1);
        assert_eq!(hard_since_walking(&[WALKING; WINDOW]), 0);
    }

    // ------------------------------------------------------------------
    // The walk of 2026-11-23
    // ------------------------------------------------------------------
    //
    // A walk home on the sealed watch (0.12.0-rc.2): each minute's mean
    // movement and steps from 15:05 UTC, 45 minutes, as this crate's
    // replay gathers them from the log. Walking from 15:12 (index 7) to
    // 15:32 (27), slowing over the next two minutes, then sitting down.
    //
    // The watch found it as 15:16 ended, and as 15:34 did said `workout:
    // now Effort (steps 20, 20, 20; 9/10 min moving)`: seven of the walk's
    // minutes and two of slowing down. That effort kept the heart rate's
    // sensor on for another four minutes, until five quiet ones.
    #[rustfmt::skip]
    const WALK_HOME: [(u16, u16); 45] = [
        (864, 0), (675, 0), (842, 0), (751, 0), (675, 0), (406, 0),
        (1566, 0), (4805, 48), (4838, 68), (4985, 120), (5905, 128), (5620, 128),
        (5776, 126), (5767, 140), (6316, 148), (6328, 156), (6327, 146), (5641, 126),
        (6090, 142), (6169, 148), (6663, 144), (5674, 130), (5431, 130), (5638, 128),
        (5765, 132), (5563, 122), (5611, 124), (3875, 92), (2638, 20), (4819, 20),
        (848, 20), (384, 0), (128, 0), (513, 0), (259, 0), (84, 0),
        (998, 8), (1832, 12), (1624, 12), (275, 0), (46, 0), (50, 0),
        (159, 0), (151, 0), (79, 0),
    ];

    #[test]
    fn the_walk_home_ends_as_a_walk() {
        let said = replay(&WALK_HOME);
        assert!(said[..11].iter().all(Option::is_none));
        // From its third minute at pace until three minutes after its
        // last, and then nothing: no effort made of its own minutes.
        assert!(
            said[11..30].iter().all(|&kind| kind == Some(Kind::Walk)),
            "{said:?}"
        );
        assert!(said[30..].iter().all(Option::is_none), "{said:?}");
    }

    #[test]
    fn asleep_is_never_a_workout_and_ends_one() {
        let day = repeat(WALKING, 10);
        let said = run_asleep(&day, |i| i >= 6);
        assert_eq!(said[5], Some(Kind::Walk));
        assert!(said[6..].iter().all(Option::is_none));
    }

    #[test]
    fn a_workout_always_ends_once_the_wrist_is_still() {
        // From any kind, a window of still minutes says none: nothing in
        // the past holds a workout open.
        for current in [None, Some(Kind::Walk), Some(Kind::Effort)] {
            assert_eq!(detect(current, &[STILL; WINDOW], false), None);
        }
        // Every pattern of walking, lifting and still minutes over two
        // windows, from every kind: once followed by a still window, no
        // workout. (3^8 patterns of the first eight minutes are plenty:
        // the rules look at runs of at most ten.)
        for pattern in 0..3usize.pow(8) {
            for start in [None, Some(Kind::Walk), Some(Kind::Effort)] {
                let mut recent = [STILL; WINDOW];
                let mut current = start;
                let mut digits = pattern;
                for _ in 0..8 {
                    recent.rotate_left(1);
                    recent[WINDOW - 1] = [STILL, WALKING, LIFTING][digits % 3];
                    digits /= 3;
                    current = detect(current, &recent, false);
                }
                for _ in 0..WINDOW {
                    recent.rotate_left(1);
                    recent[WINDOW - 1] = STILL;
                    current = detect(current, &recent, false);
                }
                assert_eq!(current, None, "pattern {pattern} from {start:?}");
            }
        }
    }

    #[test]
    fn kinds_survive_a_byte() {
        for kind in [None, Some(Kind::Walk), Some(Kind::Effort)] {
            assert_eq!(Kind::from_bits(Kind::bits(kind)), kind);
        }
        assert_eq!(Kind::from_bits(200), None);
    }

    #[test]
    fn the_arms_swing_is_only_rejected_while_walking() {
        const LIMIT: u8 = 15;
        // Stepping in a walk: a resting rate is the arm.
        assert!(swing(58.0, Some(Kind::Walk), 0, LIMIT));
        assert!(swing(69.9, Some(Kind::Walk), 14, LIMIT));
        // A walking heart rate is taken.
        assert!(!swing(70.0, Some(Kind::Walk), 0, LIMIT));
        assert!(!swing(118.0, Some(Kind::Walk), 0, LIMIT));
        // Stopped for half a minute: recovery may read anything.
        assert!(!swing(58.0, Some(Kind::Walk), 15, LIMIT));
        // Not walking: a calm wrist between sets may well be at 60.
        assert!(!swing(58.0, Some(Kind::Effort), 0, LIMIT));
        assert!(!swing(58.0, None, 0, LIMIT));
    }
}
