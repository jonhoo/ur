//! Host tests for the firmware's `log_pack` module: the field log's packed
//! heart rate samples and minutes of polls. `cargo test` runs them. The
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
