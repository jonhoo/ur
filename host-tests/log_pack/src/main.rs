//! Host tests for the firmware's `log_pack` module: the field log's packed
//! heart rate samples and minutes of polls, and which of the accelerometer's
//! polls through a heart rate session it leaves out. `cargo test` runs them. The
//! byte sequences here are also in `scripts/expand_log_test.py`, which
//! checks that `scripts/expand_log.py` unpacks them to the same samples.

#[path = "../../../src/log_pack.rs"]
#[allow(dead_code)]
mod log_pack;

fn main() {
    println!("run `cargo test`");
}

#[cfg(test)]
mod tests {
    use super::log_pack::*;

    /// The expander's unpacking, as `scripts/expand_log.py` does it.
    fn unpack(bytes: &[u8]) -> Vec<u16> {
        let mut samples: Vec<u16> = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == ESCAPE {
                samples.push(u16::from_le_bytes([bytes[i + 1], bytes[i + 2]]));
                i += 3;
            } else {
                let before = *samples.last().expect("a difference follows a sample");
                samples.push((i32::from(before) + i32::from(bytes[i] as i8)) as u16);
                i += 1;
            }
        }
        samples
    }

    fn pack(samples: &[u16]) -> Vec<u8> {
        let mut out = vec![0; PACKED_PER_SAMPLE * samples.len()];
        let len = pack_samples(samples, &mut out);
        out.truncate(len);
        out
    }

    /// Every case at the edges: the first sample whole, a difference past
    /// a byte (+129, -251), the largest that fit (-127, +127), and -128,
    /// which doesn't (it's the escape).
    const SAMPLES: [u16; 9] = [3731, 3860, 3856, 3851, 3600, 3601, 3474, 3601, 3473];
    const PACKED: [u8; 17] = [
        0x80, 0x93, 0x0e, 0x80, 0x14, 0x0f, 0xfc, 0xfb, 0x80, 0x10, 0x0e, 0x01, 0x81, 0x7f, 0x80,
        0x91, 0x0d,
    ];

    #[test]
    fn packs_the_shared_vector() {
        assert_eq!(pack(&SAMPLES), PACKED);
        assert_eq!(unpack(&PACKED), SAMPLES);
        assert_eq!(pack(&[]), Vec::<u8>::new());
    }

    /// Random walks of every step size, and jumps across the whole range,
    /// come back as they went in.
    #[test]
    fn round_trips() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..2000 {
            let len = (next() % 41) as usize;
            let step = [2, 16, 128, 300, 65_536][round % 5];
            let mut value = (next() % 65_536) as i64;
            let samples: Vec<u16> = (0..len)
                .map(|_| {
                    value = (value + (next() % step) as i64 - step as i64 / 2).clamp(0, 65_535);
                    value as u16
                })
                .collect();
            let packed = pack(&samples);
            assert!(packed.len() <= PACKED_PER_SAMPLE * samples.len());
            assert_eq!(unpack(&packed), samples);
        }
    }

    /// Too little room: whole samples only, never part of one.
    #[test]
    fn stops_at_a_sample_that_does_not_fit() {
        let mut out = [0u8; 4];
        let len = pack_samples(&SAMPLES, &mut out);
        assert_eq!(len, 3);
        assert_eq!(unpack(&out[..len]), SAMPLES[..1]);
    }

    #[test]
    fn polls_fill_a_minute() {
        let mut polls = Polls::new();
        for i in 0..POLLS as u64 {
            let full = polls.push(
                10_000 + 2_000 * i,
                24 + (i % 3) as u32,
                70_000 * i,
                i as u32,
            );
            assert_eq!(full, i + 1 == POLLS as u64);
        }
        assert_eq!(polls.movement().len(), POLLS);
        // Saturating, as the minute history does.
        assert_eq!(polls.movement()[1], 65_535);
        assert_eq!(polls.movement()[0], 0);
        assert_eq!(polls.steps()[29], 29);
        // Counted from 24.
        assert_eq!(&polls.samples()[..3], &[0, 1, 2]);
        assert_eq!(polls.span_ms(10_000 + 2_000 * 29), 58_000);
        polls.clear();
        assert!(polls.movement().is_empty());
        assert!(!polls.push(500, 24, 3, 0));
        assert_eq!(polls.span_ms(2_500), 2_000);
    }

    /// A watch on a still wrist, as the sealed watch logged it: a 1/64 g
    /// of noise on each axis.
    #[test]
    fn a_wrist_at_rest_is_still() {
        let poll = [[30, -1, -58], [29, -1, -57], [28, -1, -58], [31, -2, -56]];
        assert_eq!(still(&poll), Some([29, -2, -57]));
        assert_eq!(still(&[[0, 0, 64]; 24]), Some([0, 0, 64]));
    }

    /// Where it says the wrist was is within 8 of every sample, on each
    /// axis, for any poll it calls still.
    #[test]
    fn a_still_polls_samples_are_within_8_of_its_middle() {
        // Each axis at both ends of the widest range that's still, about
        // every offset an `i8` holds.
        for base in -128i16..=112 {
            let (low, high) = (base as i8, (base + 15) as i8);
            for axis in 0..3 {
                let mut poll = [[0i8, 0, 0]; 2];
                poll[0][axis] = low;
                poll[1][axis] = high;
                let Some(middle) = still(&poll) else {
                    // The magnitude ranged too far (it can, by up to 15,
                    // when one axis is all of it).
                    continue;
                };
                for sample in poll {
                    for (value, about) in sample.iter().zip(middle) {
                        assert!((i16::from(*value) - i16::from(about)).abs() <= 8);
                    }
                }
            }
        }
    }

    /// A walk: the arm's swing and each step change how hard the wrist is
    /// pulled, by far more than an eighth of a g.
    #[test]
    fn a_walking_wrist_is_not_still() {
        let poll = [[30, -1, -58], [44, 5, -70], [20, -8, -49], [35, 2, -66]];
        assert_eq!(still(&poll), None);
        // The magnitude alone: one axis, 8 apart.
        assert_eq!(still(&[[0, 0, 64], [0, 0, 72]]), None);
        assert!(still(&[[0, 0, 64], [0, 0, 71]]).is_some());
    }

    /// A wrist that turns keeps gravity's 1 g, so the magnitude holds
    /// steady: the axes' ranges catch it.
    #[test]
    fn a_turning_wrist_is_not_still() {
        // From face up to tilted 30°: 64 × (sin 30°, 0, cos 30°).
        let poll = [[0, 0, 64], [17, 0, 62], [32, 0, 55]];
        assert_eq!(still(&poll), None);
        // A quarter of a g on one axis, exactly, with the magnitude
        // within 2: not still; one less is.
        assert_eq!(still(&[[0, 0, 64], [16, 0, 62]]), None);
        assert!(still(&[[0, 0, 64], [15, 0, 62]]).is_some());
    }

    #[test]
    fn a_poll_without_samples_is_not_still() {
        assert_eq!(still(&[]), None);
    }

    /// The extremes an `i8` holds don't overflow the arithmetic.
    #[test]
    fn the_largest_samples_fit() {
        assert_eq!(still(&[[-128, -128, -128]; 36]), Some([-128, -128, -128]));
        assert_eq!(still(&[[127, 127, 127]; 36]), Some([127, 127, 127]));
        assert_eq!(still(&[[-128, -128, -128], [127, 127, 127]]), None);
    }

    /// Fewer samples than 24 wrap, and the expander's `(n + 24) % 256`
    /// undoes it for every count.
    #[test]
    fn sample_counts_wrap() {
        for count in 0..=255u32 {
            let mut polls = Polls::new();
            polls.push(0, count, 0, 0);
            assert_eq!((u32::from(polls.samples()[0]) + 24) % 256, count);
        }
    }
}

/// Kani proofs (`cargo kani` here, one at a time with `--harness`).
///
/// TODO: a proof that any samples `pack_samples` packs unpack (as
/// `scripts/expand_log.py` does) to themselves. Kani 0.68 finds a
/// counterexample to it that the same inputs, run concretely, don't
/// reproduce; that looks like a Kani modelling bug, and is being looked
/// into apart from this.
#[cfg(kani)]
mod proofs {
    use super::log_pack::*;

    /// What `still` promises: every sample within 8 (half of
    /// `STILL_AXIS_RANGE`) of the middle it gives, on each axis, for any
    /// poll of up to three samples.
    #[kani::proof]
    #[kani::unwind(5)]
    fn a_still_polls_samples_are_within_8_of_its_middle() {
        let samples: [[i8; 3]; 3] = kani::any();
        let n: usize = kani::any();
        kani::assume(n <= 3);
        let middle = still(&samples[..n]);
        let mut furthest = 0;
        if let Some(middle) = middle {
            for sample in &samples[..n] {
                for axis in 0..3 {
                    let d = (i16::from(sample[axis]) - i16::from(middle[axis])).abs();
                    assert!(d <= STILL_AXIS_RANGE / 2, "{d}");
                    furthest = furthest.max(d);
                }
            }
        }
        kani::cover!(n == 3 && middle.is_some(), "three samples at rest");
        // The bound is tight: a range of 15 puts its top 8 from the middle.
        kani::cover!(furthest == STILL_AXIS_RANGE / 2, "a sample 8 away");
    }
}
