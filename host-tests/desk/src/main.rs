//! Host tests for the firmware's `desk` module. `cargo test` runs them.
//!
//! Most feed [`Desk`](desk::Desk) polls as `activity` does, every 2 s:
//! made-up ones, and real ones from the sealed watch's field logs
//! (`field.txt`): a desk, the charger, and two sleeping wrists as still
//! as a desk in all but one way.

#[path = "../../../src/desk.rs"]
#[allow(dead_code)]
mod desk;
#[path = "../../../src/sleep.rs"]
#[allow(dead_code)]
mod sleep;

fn main() {
    println!("run `cargo test`");
}

#[cfg(test)]
mod tests {
    use super::desk::*;

    /// How often the accelerometer polls, as in `activity`.
    const POLL: u32 = 2;
    /// A watch lying level and face up, as the sealed one on the desk.
    const LEVEL_UP: [i16; 3] = [-20, 14, -1031];

    /// What a run of polls changed: (seconds, change).
    fn feed(
        desk: &mut Desk,
        from: u32,
        polls: impl IntoIterator<Item = (u16, [i16; 3])>,
        pulse: Option<u32>,
    ) -> Vec<(u32, Change)> {
        let mut changes = Vec::new();
        for (i, (movement, sample)) in polls.into_iter().enumerate() {
            let now = from + i as u32 * POLL;
            if let Some(change) = desk.poll(now, movement, sample, pulse) {
                changes.push((now, change));
            }
        }
        changes
    }

    /// `seconds` of polls lying as `sample`, moving `movement` each.
    fn lying(seconds: u32, movement: u16, sample: [i16; 3]) -> Vec<(u16, [i16; 3])> {
        vec![(movement, sample); (seconds / POLL) as usize]
    }

    #[test]
    fn a_quiet_level_watch_is_on_a_desk_after_ten_minutes() {
        let mut desk = Desk::default();
        let changes = feed(&mut desk, 1000, lying(SETTLE + 60, 28, LEVEL_UP), None);
        assert_eq!(
            changes,
            [(1000 + SETTLE, Change::Down { since: 1000 })],
            "found once, dated from when it was put down"
        );
        assert!(desk.on());
    }

    #[test]
    fn a_stir_picks_it_up() {
        let mut desk = Desk::default();
        feed(&mut desk, 0, lying(SETTLE + POLL, 28, LEVEL_UP), None);
        assert!(desk.on());
        // A bump on the desk is no stir.
        assert_eq!(desk.poll(SETTLE + 2, STIR, LEVEL_UP, None), None);
        assert_eq!(
            desk.poll(SETTLE + 4, STIR + 1, LEVEL_UP, None),
            Some(Change::Up)
        );
        assert!(!desk.on());
        // And it's a new spell from there: ten minutes more.
        let from = SETTLE + 6;
        let changes = feed(&mut desk, from, lying(SETTLE + POLL, 28, LEVEL_UP), None);
        assert_eq!(changes, [(from + SETTLE, Change::Down { since: from })]);
    }

    #[test]
    fn tilted_or_turned_over_is_no_desk() {
        for sample in [
            // Tilted past level, either way.
            [LEVEL as i16 + 1, 0, -1028],
            [0, -(LEVEL as i16) - 1, -1028],
            // Face down, on its side.
            [0, 0, 1030],
            [1030, 0, 0],
        ] {
            let mut desk = Desk::default();
            assert_eq!(
                feed(&mut desk, 0, lying(2 * SETTLE, 25, sample), None),
                [],
                "{sample:?}"
            );
        }
        // Tilted off level while on a desk: picked up.
        let mut desk = Desk::default();
        feed(&mut desk, 0, lying(SETTLE + POLL, 28, LEVEL_UP), None);
        assert_eq!(
            desk.poll(SETTLE, 28, [200, 14, -1010], None),
            Some(Change::Up)
        );
    }

    #[test]
    fn a_pulse_says_skin() {
        // A reading during the spell: a wrist, however still.
        let mut desk = Desk::default();
        let changes = feed(&mut desk, 100, lying(3 * SETTLE, 28, LEVEL_UP), Some(400));
        assert_eq!(changes, []);
        // A reading from before the spell doesn't count.
        let mut desk = Desk::default();
        let changes = feed(&mut desk, 100, lying(SETTLE, 28, LEVEL_UP), Some(99));
        assert_eq!(changes, []);
        assert_eq!(
            desk.poll(100 + SETTLE, 28, LEVEL_UP, Some(99)),
            Some(Change::Down { since: 100 })
        );
        // A reading on a desk: it was a wrist after all, and the spell
        // can't be a desk again, until it moves and starts another.
        assert_eq!(
            desk.poll(102 + SETTLE, 28, LEVEL_UP, Some(101 + SETTLE)),
            Some(Change::Up)
        );
        let changes = feed(
            &mut desk,
            104 + SETTLE,
            lying(2 * SETTLE, 28, LEVEL_UP),
            Some(101 + SETTLE),
        );
        assert_eq!(changes, []);
    }

    #[test]
    fn noise_is_a_wrist() {
        // Every poll at the limit: a desk.
        let mut desk = Desk::default();
        assert_eq!(
            feed(&mut desk, 0, lying(SETTLE + 2, QUIET, LEVEL_UP), None).len(),
            1
        );
        // One more: a wrist (a level sleeping one moved 45).
        let mut desk = Desk::default();
        assert_eq!(
            feed(&mut desk, 0, lying(3 * SETTLE, QUIET + 1, LEVEL_UP), None),
            []
        );
        // The mean, not every poll: the odd bump is fine.
        let mut desk = Desk::default();
        let mut polls = lying(SETTLE + 2, 25, LEVEL_UP);
        polls[100].0 = 280;
        assert_eq!(feed(&mut desk, 0, polls, None).len(), 1);
    }

    #[test]
    fn a_drift_is_a_wrist_and_a_nudge_a_desk() {
        // Drifting by more than `STEADY` before it's a desk: a wrist
        // settling, or one breathing. The spell starts over each time,
        // every 200 s here, under the 600 s a desk takes.
        let mut desk = Desk::default();
        let drifting = (0..800).map(|i| (28, [-60 + (i / 100) as i16 * 17, 14, -1031]));
        assert_eq!(feed(&mut desk, 0, drifting, None), []);
        // Nudged on a desk: still one, from where it lies now.
        let mut desk = Desk::default();
        feed(&mut desk, 0, lying(SETTLE + POLL, 28, LEVEL_UP), None);
        let nudged = [20, 30, -1030];
        assert_eq!(feed(&mut desk, SETTLE, lying(60, 28, nudged), None), []);
        assert!(desk.on());
        // Then slowly back, a little at a time: still steady from there.
        assert_eq!(
            feed(&mut desk, SETTLE + 60, lying(60, 28, [10, 20, -1030]), None),
            []
        );
        assert!(desk.on());
    }

    // ==========================================================================

    // ==========================================================================
    // The field logs
    // ==========================================================================

    /// A window of `field.txt`: its polls (seconds, movement, sample), and
    /// its heart rate readings (seconds).
    struct Window {
        polls: Vec<(u32, u16, [i16; 3])>,
        pulses: Vec<u32>,
    }

    fn window(name: &str) -> Window {
        let text = include_str!("field.txt");
        let mut lines = text
            .lines()
            .skip_while(|line| line.split(' ').nth(1) != Some(name))
            .skip(1)
            .take_while(|line| !line.starts_with("window "));
        let mut window = Window {
            polls: Vec::new(),
            pulses: Vec::new(),
        };
        for line in lines.by_ref() {
            let fields: Vec<&str> = line.split(' ').collect();
            match fields[..] {
                ["pulse", at] => window.pulses.push(at.parse().expect("seconds")),
                [at, movement, x, y, z] => window.polls.push((
                    at.parse().expect("seconds"),
                    movement.parse().expect("movement"),
                    [x, y, z].map(|axis| axis.parse().expect("an axis")),
                )),
                _ => panic!("what's {line:?}?"),
            }
        }
        assert!(!window.polls.is_empty(), "no window {name}");
        window
    }

    /// Replays a window as `activity` does: the latest pulse before each
    /// poll goes with it.
    fn replay(name: &str) -> Vec<(u32, Change)> {
        let window = window(name);
        let mut desk = Desk::default();
        let mut changes = Vec::new();
        for &(at, movement, sample) in &window.polls {
            let pulse = window.pulses.iter().rev().find(|&&p| p <= at).copied();
            if let Some(change) = desk.poll(at, movement, sample, pulse) {
                changes.push((at, change));
            }
        }
        changes
    }

    /// The bug: put down at 17:07, bumped at 17:16, and "asleep" at 17:27
    /// (20 minutes still), a nap starting. Now a desk from 17:15:52 (the
    /// bump, before it had been one for 10 minutes, starting it over),
    /// found 10 minutes on, before sleep's 20; the 9 minutes before it
    /// can't make a sleep on their own.
    #[test]
    fn the_desk_in_the_field_is_found() {
        let changes = replay("desk");
        // 17:05 + 652 s = 17:15:52.
        assert_eq!(changes, [(652 + SETTLE + 1, Change::Down { since: 652 })]);
    }

    /// On a desk, then picked up and set on the charger (18:05), which
    /// is a desk too.
    #[test]
    fn the_charger_in_the_field_is_a_desk() {
        let changes = replay("charger");
        assert!(
            matches!(changes[..], [(_, Change::Down { since })] if (160..400).contains(&since)),
            "{changes:?}"
        );
    }

    /// The field logs' only spell of a sleeping wrist lying level and face
    /// up: never a desk. Steady for 6 minutes at most, moving 45 on
    /// average, a reading every 5 minutes.
    #[test]
    fn a_level_sleeping_wrist_is_no_desk() {
        assert_eq!(replay("level-sleep"), []);
    }

    /// A sleeping wrist as quiet (29) and as steady as a desk for 18
    /// minutes, but face down: never a desk.
    #[test]
    fn a_still_sleeping_wrist_is_no_desk() {
        assert_eq!(replay("still-sleep"), []);
    }
}

/// Proofs, for `cargo kani` (see the brief's cap on running them).
#[cfg(kani)]
mod proofs {
    use super::desk::*;

    /// From any state reached by three polls, at times that move on, of
    /// any movement, pose and pulse: a desk is found only on a level,
    /// face-up poll that's no stir, dated at least [`SETTLE`] back, with
    /// no reading since; changes alternate, as `on` says; and nothing
    /// panics or overflows.
    #[kani::proof]
    #[kani::unwind(4)]
    fn a_desk_is_found_only_as_documented() {
        let mut desk = Desk::default();
        let mut now = 0u32;
        let mut on = false;
        for _ in 0..3 {
            let (later, movement, sample, pulse): (u32, u16, [i16; 3], Option<u32>) = kani::any();
            kani::assume(now <= later);
            now = later;
            let change = desk.poll(now, movement, sample, pulse);
            match change {
                Some(Change::Down { since }) => {
                    assert!(!on);
                    assert!(since <= now && now - since >= SETTLE);
                    let [x, y, z] = sample;
                    assert!(x.unsigned_abs() <= LEVEL && y.unsigned_abs() <= LEVEL);
                    assert!(z <= FACE_UP && movement <= STIR);
                    assert!(pulse.is_none_or(|at| at < since));
                    on = true;
                }
                Some(Change::Up) => {
                    assert!(on);
                    on = false;
                }
                None => {}
            }
            assert_eq!(desk.on(), on);
        }
    }
}
