//! Host tests for the firmware's `spill` module: the log of activity
//! minutes in the SPI flash. `cargo test` runs them, against a model of
//! NOR flash whose power can be cut in the middle of any write or erase.

// `spill` takes its checksum from `snapshot`, by this path.
#[path = "../../../src/snapshot.rs"]
#[allow(dead_code)]
mod snapshot;
#[path = "../../../src/spill.rs"]
#[allow(dead_code)]
mod spill;

fn main() {
    println!("run `cargo test`");
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use quickcheck::{Arbitrary, Gen, QuickCheck, TestResult};

    use super::spill::{self, Flash, Read, Reader, Ring, SAMPLE, SECTOR, SLOTS, Want, Writer};

    /// 2026-11-09 12:00 UTC, in minutes since 1970.
    const NOON: u32 = 29_903_760;
    /// Gadgetbridge's buckets (`gb_buckets::MINUTES`).
    const BUCKET: usize = 5;
    /// Where the test rings start: anywhere but 0, to catch an address
    /// taken from the ring's start.
    const START: u32 = 3 * SECTOR;

    /// The model's futures never wait: one poll finishes them.
    fn block_on<T>(future: impl Future<Output = T>) -> T {
        let mut context = Context::from_waker(Waker::noop());
        match pin!(future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("the model flash never waits"),
        }
    }

    // ==========================================================================
    // NOR flash, as a model
    // ==========================================================================

    /// NOR flash: programming only clears bits, erasing sets a sector's.
    /// With `fuel`, the power goes after that many programs and erases:
    /// the one under way is done in part, and everything after fails,
    /// until [`Nor::power_on`].
    struct Nor {
        bytes: Vec<u8>,
        ring: Ring,
        fuel: Option<u32>,
        /// What decides how much of the write under way got done.
        noise: u64,
        /// Erases of each sector of the ring.
        erases: Vec<u32>,
        programs: u32,
    }

    impl Nor {
        fn new(sectors: u32) -> Self {
            let ring = Ring {
                start: START,
                sectors,
            };
            // A sector either side of the ring, to catch writes outside.
            let bytes = vec![0xA7; (START + (sectors + 1) * SECTOR) as usize];
            let mut nor = Self {
                bytes,
                ring,
                fuel: None,
                noise: 0x9E37_79B9_7F4A_7C15,
                erases: vec![0; sectors as usize],
                programs: 0,
            };
            // As the flash was left by whatever used it before: neither
            // erased nor ours.
            for (i, byte) in nor.bytes.iter_mut().enumerate() {
                *byte = (i * 31 % 251) as u8;
            }
            nor
        }

        fn noise(&mut self) -> u64 {
            // xorshift64.
            self.noise ^= self.noise << 13;
            self.noise ^= self.noise >> 7;
            self.noise ^= self.noise << 17;
            self.noise
        }

        /// Whether the power's still on for one more write; if not, how
        /// much of it gets done is the caller's to decide.
        fn spend(&mut self) -> bool {
            match &mut self.fuel {
                None => true,
                Some(0) => false,
                Some(fuel) => {
                    *fuel -= 1;
                    true
                }
            }
        }

        fn dead(&self) -> bool {
            self.fuel == Some(0)
        }

        fn power_on(&mut self) {
            self.fuel = None;
        }

        fn in_ring(&self, address: u32, len: usize) -> bool {
            let end = self.ring.start + self.ring.sectors * SECTOR;
            address >= self.ring.start && address + len as u32 <= end
        }
    }

    impl Flash for Nor {
        type Error = ();

        async fn read(&mut self, address: u32, data: &mut [u8]) {
            assert!(self.in_ring(address, data.len()), "read outside the ring");
            data.copy_from_slice(&self.bytes[address as usize..][..data.len()]);
        }

        async fn program(&mut self, address: u32, data: &[u8]) -> Result<(), ()> {
            assert!(
                self.in_ring(address, data.len()),
                "program outside the ring"
            );
            assert!(
                address % 256 + data.len() as u32 <= 256,
                "a program stays within one page"
            );
            let was_dead = self.dead();
            let whole = self.spend();
            if was_dead {
                return Err(());
            }
            self.programs += 1;
            for (i, &byte) in data.iter().enumerate() {
                // Cut short, each bit that should clear may or may not.
                let mask = if whole { 0 } else { self.noise() as u8 };
                self.bytes[address as usize + i] &= byte | mask;
            }
            if whole { Ok(()) } else { Err(()) }
        }

        async fn erase(&mut self, address: u32) -> Result<(), ()> {
            assert!(self.in_ring(address, SECTOR as usize) && address.is_multiple_of(SECTOR));
            let was_dead = self.dead();
            let whole = self.spend();
            if was_dead {
                return Err(());
            }
            self.erases[((address - self.ring.start) / SECTOR) as usize] += 1;
            for i in 0..SECTOR as usize {
                // Cut short, each bit that should set may or may not.
                let mask = if whole { 0xFF } else { self.noise() as u8 };
                self.bytes[address as usize + i] |= mask;
            }
            if whole { Ok(()) } else { Err(()) }
        }
    }

    // ==========================================================================
    // The obviously right log, and the real one beside it
    // ==========================================================================

    /// A sample that says which minute it's of, recorded (seen) or, one
    /// minute in 17, with only a heart rate.
    fn sample(minute: u32) -> [u8; SAMPLE] {
        let [a, b, c, _] = minute.to_le_bytes();
        if minute.is_multiple_of(17) {
            [a, b, c, 0, 60 + (minute % 100) as u8, 0]
        } else {
            [a, b, c, 1, 0, 0x40]
        }
    }

    /// Everything read back by [`spill::scan`].
    fn scan(flash: &mut Nor) -> (Writer, Option<u32>, Vec<u32>) {
        let mut minutes = Vec::new();
        let ring = flash.ring;
        let (writer, synced) = block_on(spill::scan(flash, ring, |minute, bytes| {
            assert_eq!(*bytes, sample(minute), "minute {minute} holds another's");
            minutes.push(minute);
        }));
        (writer, synced, minutes)
    }

    /// The buckets a fetch from `from` to `end` reads, as (first minute,
    /// the minutes it has).
    fn fetch(flash: &mut Nor, from: u32, end: u32) -> Vec<(u32, Vec<u32>)> {
        let ring = flash.ring;
        let Some((mut reader, _)) = block_on(spill::open(flash, ring, from)) else {
            return Vec::new();
        };
        read_on(flash, &mut reader, from, end)
    }

    /// Reads on as `ble::Fetching` does: a bucket at a time, each
    /// from after the last, opening the log again should the ring have
    /// overtaken the reader.
    fn read_on(flash: &mut Nor, reader: &mut Reader, from: u32, end: u32) -> Vec<(u32, Vec<u32>)> {
        let ring = flash.ring;
        let mut buckets = Vec::new();
        let mut from = from;
        loop {
            let mut want = Want {
                reader: &mut *reader,
                from,
                end,
                out: [None; BUCKET],
            };
            let first = match block_on(spill::bucket(flash, ring, &mut want)) {
                Read::Bucket(first) => first,
                Read::End => break,
                Read::Lost => match block_on(spill::open(flash, ring, from)) {
                    Some((again, _)) => {
                        *reader = again;
                        continue;
                    }
                    None => break,
                },
            };
            let out = want.out;
            assert_eq!(first % BUCKET as u32, 0);
            assert!(first + BUCKET as u32 > from && first < end);
            let minutes: Vec<u32> = (first..)
                .zip(out)
                .filter_map(|(minute, bytes)| {
                    let bytes = bytes?;
                    assert_eq!(bytes, sample(minute));
                    Some(minute)
                })
                .collect();
            assert!(!minutes.is_empty());
            buckets.push((first, minutes));
            from = first + BUCKET as u32;
        }
        buckets
    }

    /// The same, of `minutes` (what the log holds), worked out the plain
    /// way.
    fn buckets_of(minutes: &[u32], from: u32, end: u32) -> Vec<(u32, Vec<u32>)> {
        let mut buckets: Vec<(u32, Vec<u32>)> = Vec::new();
        for &minute in minutes.iter().filter(|&&m| m >= from && m < end) {
            let first = minute - minute % BUCKET as u32;
            match buckets.last_mut() {
                Some((start, bucket)) if *start == first => bucket.push(minute),
                _ => buckets.push((first, vec![minute])),
            }
        }
        buckets
    }

    #[derive(Clone, Debug)]
    enum Op {
        /// After `gap` minutes with nothing recorded, `run` minutes in a
        /// row are appended.
        Record { gap: u16, run: u16 },
        /// The phone fetched everything so far.
        Synced,
        /// The clock goes back by so many minutes.
        Back(u16),
        /// A reset: the writer is found again by a scan.
        Reboot,
        /// The power goes after this many more writes.
        Cut(u8),
    }

    impl Arbitrary for Op {
        fn arbitrary(g: &mut Gen) -> Self {
            match u8::arbitrary(g) % 16 {
                0..=7 => Op::Record {
                    gap: *g.choose(&[0, 0, 0, 1, 3, 90, 2000]).expect("not empty"),
                    run: u16::arbitrary(g) % 700,
                },
                8..=9 => Op::Synced,
                10..=11 => Op::Back(*g.choose(&[1, 2, 7, 100, 600, 5000]).expect("not empty")),
                12..=13 => Op::Reboot,
                _ => Op::Cut(u8::arbitrary(g) % 40),
            }
        }
    }

    /// Whether `part` is the end of `whole`.
    fn is_suffix(part: &[u32], whole: &[u32]) -> bool {
        whole.ends_with(part)
    }

    /// Runs `ops` on a ring of `sectors`, against a plain list of what
    /// should be in it, checking after every reset that the log reads
    /// back as the list's end (the ring forgets its oldest), whatever the
    /// power did.
    fn run(sectors: u32, ops: &[Op]) -> TestResult {
        let mut flash = Nor::new(sectors);
        let ring = flash.ring;
        let mut writer = Writer::EMPTY;
        // What should be in the log, the last note of a fetch, and the
        // next minute of the clock.
        let mut model: Vec<u32> = Vec::new();
        let mut synced: Option<u32> = None;
        let mut clock = NOON;

        for op in ops {
            // What the op under way may or may not have got done, should
            // the power go: a sample appended, a note made, or samples
            // from a minute on taken back.
            let mut maybe_sample = None;
            let mut maybe_synced = None;
            let mut maybe_back = None;
            let mut failed = false;
            match *op {
                Op::Record { gap, run } => {
                    clock += u32::from(gap);
                    for _ in 0..run {
                        let bytes = sample(clock);
                        if block_on(spill::append(&mut flash, ring, &mut writer, clock, &bytes))
                            .is_err()
                        {
                            maybe_sample = Some(clock);
                            failed = true;
                            break;
                        }
                        model.push(clock);
                        clock += 1;
                    }
                    if !failed {
                        writer.passed(clock - 1);
                        assert_eq!(writer.through(), Some(clock - 1));
                    }
                }
                Op::Synced => {
                    let minute = clock - 1;
                    if block_on(spill::synced(&mut flash, ring, &mut writer, minute)).is_err() {
                        maybe_synced = Some(minute);
                        failed = true;
                    } else {
                        synced = Some(minute);
                    }
                }
                Op::Back(minutes) => {
                    let from = clock - u32::from(minutes);
                    if block_on(spill::truncate(&mut flash, ring, &mut writer, from)).is_err() {
                        maybe_back = Some(from);
                        failed = true;
                    } else {
                        model.retain(|&minute| minute < from);
                        clock = from;
                        assert!(writer.through().is_none_or(|through| through < from));
                    }
                }
                Op::Reboot => failed = true,
                Op::Cut(fuel) => flash.fuel = Some(u32::from(fuel)),
            }
            if !failed {
                continue;
            }
            assert!(
                flash.dead() || matches!(op, Op::Reboot),
                "a write failed with the power on"
            );

            // The reset: everything's read back.
            flash.power_on();
            let (found, noted, minutes) = scan(&mut flash);
            writer = found;
            if let Some(minute) = maybe_sample
                && minutes.last() == Some(&minute)
            {
                model.push(minute);
            }
            if let Some(minute) = maybe_synced
                && noted == Some(minute)
            {
                synced = noted;
            }
            if let Some(from) = maybe_back {
                // Some of the samples from `from` on are gone, and none
                // of those before; the caller tries again (as
                // `system_off` does, while the log reaches past the
                // history), and then they all are.
                let before: Vec<u32> = minutes.iter().copied().filter(|&m| m < from).collect();
                model.retain(|&minute| minute < from);
                if !is_suffix(&before, &model) {
                    return TestResult::error(format!(
                        "a truncation cut short lost older minutes: {before:?} of {model:?}"
                    ));
                }
                block_on(spill::truncate(&mut flash, ring, &mut writer, from))
                    .expect("the power is on");
                clock = from;
                let (_, _, minutes) = scan(&mut flash);
                if !is_suffix(&minutes, &model) {
                    return TestResult::error(format!(
                        "after truncating again: {minutes:?} of {model:?}"
                    ));
                }
                continue;
            }
            if !is_suffix(&minutes, &model) {
                return TestResult::error(format!("read back {minutes:?}, not {model:?}"));
            }
            // A note can be lost with the sector it was in, but never
            // an older one found in its place.
            if noted.is_some() && noted != synced {
                return TestResult::error(format!("phone has {noted:?}, not {synced:?}"));
            }
            if writer.through().is_some_and(|t| Some(&t) != minutes.last()) {
                return TestResult::error("the writer's place isn't the last sample's");
            }
            // And a fetch reads the same minutes as the scan, in whole
            // buckets, from anywhere.
            let end = clock + 10;
            for back in [0, 1, 40, 700, 100_000] {
                let from =
                    (clock.saturating_sub(back * BUCKET as u32)) / BUCKET as u32 * BUCKET as u32;
                let read = fetch(&mut flash, from, end);
                let expected = buckets_of(&minutes, from, end);
                if read != expected {
                    return TestResult::error(format!(
                        "a fetch from {from} read {read:?}, not {expected:?}"
                    ));
                }
                // And where the reader says the log starts, when that's
                // after `from`, it does: a fetch goes by it (it sends
                // nothing from before the log's first whole bucket).
                if let Some((_, first)) = block_on(spill::open(&mut flash, ring, from))
                    && let Some(&earlier) = minutes.iter().find(|&&m| m >= from && m < first)
                {
                    return TestResult::error(format!(
                        "a fetch from {from} was told the log starts at {first}, \
                         but it has minute {earlier}"
                    ));
                }
            }
        }
        TestResult::passed()
    }

    #[test]
    fn the_log_reads_back_as_written_whatever_the_power_does() {
        // A ring that comes round within a case or two, and one that
        // mostly doesn't.
        fn small(ops: Vec<Op>) -> TestResult {
            run(3, &ops)
        }
        fn large(ops: Vec<Op>) -> TestResult {
            run(8, &ops)
        }
        for ring in [small, large] {
            // More with `SPILL_TESTS=100000`, after a change to the log.
            let tests = std::env::var("SPILL_TESTS")
                .ok()
                .and_then(|tests| tests.parse().ok())
                .unwrap_or(600);
            QuickCheck::new()
                .tests(tests)
                .quickcheck(ring as fn(Vec<Op>) -> TestResult);
        }
    }

    // ==========================================================================
    // Particular cases
    // ==========================================================================

    /// Appends the minutes `from..to`, each with a sample.
    fn record(flash: &mut Nor, writer: &mut Writer, minutes: std::ops::Range<u32>) {
        let ring = flash.ring;
        for minute in minutes {
            block_on(spill::append(flash, ring, writer, minute, &sample(minute)))
                .expect("the power is on");
        }
    }

    #[test]
    fn an_empty_flash_has_no_log() {
        let mut flash = Nor::new(4);
        let (writer, synced, minutes) = scan(&mut flash);
        assert_eq!(writer, Writer::EMPTY);
        assert_eq!(synced, None);
        assert!(minutes.is_empty());
        assert!(fetch(&mut flash, NOON, NOON + 100).is_empty());
        // Nor is anything written until there's something to write.
        assert_eq!(flash.programs, 0);
        assert_eq!(flash.erases, [0; 4]);
    }

    #[test]
    fn the_watchs_ring_holds_more_than_a_week_and_wears_evenly() {
        // The ring as `system_off::SPILL` has it, every minute recorded
        // for 30 days, the phone fetching every five minutes.
        let mut flash = Nor::new(32);
        let ring = flash.ring;
        let mut writer = Writer::EMPTY;
        let days = 30;
        for minute in NOON..NOON + days * 1440 {
            record(&mut flash, &mut writer, minute..minute + 1);
            if minute % 5 == 4 {
                block_on(spill::synced(&mut flash, ring, &mut writer, minute)).expect("power");
            }
        }
        let (_, synced, minutes) = scan(&mut flash);
        assert_eq!(synced, Some(NOON + days * 1440 - 1));
        // With a note of a fetch for every bucket, the least the ring
        // holds: 31 sectors' worth, at six entries to five minutes.
        let least = 31 * (u32::from(SLOTS) - 2) * 5 / 6;
        assert!(minutes.len() as u32 >= least, "{} minutes", minutes.len());
        assert!(least >= 9 * 1440, "nine days, fetching all the while");
        assert_eq!(minutes.last(), Some(&(NOON + days * 1440 - 1)));
        assert!(minutes.windows(2).all(|pair| pair[1] == pair[0] + 1));
        // Each sector is erased once a turn of the ring: 30 days are
        // under four turns.
        let (least, most) = (
            flash.erases.iter().min().expect("32"),
            flash.erases.iter().max().expect("32"),
        );
        assert!(most - least <= 1 && *most <= 4, "erases {:?}", flash.erases);
    }

    #[test]
    fn without_fetches_the_ring_holds_eleven_days() {
        let mut flash = Nor::new(32);
        let mut writer = Writer::EMPTY;
        record(&mut flash, &mut writer, NOON..NOON + 20 * 1440);
        let (_, _, minutes) = scan(&mut flash);
        assert!(minutes.len() >= 31 * 510, "{} minutes", minutes.len());
        assert!(minutes.len() >= 10 * 1440 + 1400);
        // And a fetch of all of it reads every bucket, oldest first.
        let end = NOON + 20 * 1440;
        let read = fetch(&mut flash, 0, end);
        assert_eq!(read, buckets_of(&minutes, 0, end));
        assert!(read.len() >= 31 * 510 / 5);
    }

    #[test]
    fn a_fetch_stops_at_its_end_and_carries_on_later() {
        let mut flash = Nor::new(4);
        let ring = flash.ring;
        let mut writer = Writer::EMPTY;
        let start = NOON - NOON % 5;
        record(&mut flash, &mut writer, start..start + 20);
        let (mut reader, first) = block_on(spill::open(&mut flash, ring, start)).expect("a log");
        // The first sector of all can't say what its first minute will
        // be; any after it does.
        assert_eq!(first, 1);
        // Held back from the third bucket on (a sleep that may change).
        let read = read_on(&mut flash, &mut reader, start, start + 10);
        assert_eq!(read.len(), 2);
        // Later, with more appended, the same reader reads on.
        record(&mut flash, &mut writer, start + 20..start + 25);
        let read = read_on(&mut flash, &mut reader, start + 10, start + 25);
        assert_eq!(
            read,
            buckets_of(
                &(start..start + 25).collect::<Vec<_>>(),
                start + 10,
                start + 25
            )
        );
    }

    #[test]
    fn a_fetch_overtaken_by_the_ring_carries_on_from_the_logs_oldest() {
        let mut flash = Nor::new(3);
        let ring = flash.ring;
        let mut writer = Writer::EMPTY;
        // Three sectors full, less a little: the fetch starts in the
        // oldest.
        let start = NOON - NOON % 5 + 2;
        let mut clock = start + 3 * 509 - 20;
        record(&mut flash, &mut writer, start..clock);
        let (mut reader, first) = block_on(spill::open(&mut flash, ring, 0)).expect("a log");
        assert_eq!(first, 1);
        let end = u32::MAX;
        let next = |flash: &mut Nor, reader: &mut Reader, from| {
            let mut want = Want {
                reader,
                from,
                end,
                out: [None; BUCKET],
            };
            block_on(spill::bucket(flash, ring, &mut want))
        };
        assert_eq!(next(&mut flash, &mut reader, 0), Read::Bucket(start - 2));
        // The writer moves on to a fourth sector: the oldest is erased
        // under the reader, which says so.
        record(&mut flash, &mut writer, clock..clock + 40);
        clock += 40;
        let (_, _, minutes) = scan(&mut flash);
        assert_eq!(minutes[0], start + 509, "the oldest sector went");
        let (_, first) = block_on(spill::open(&mut flash, ring, 0)).expect("a log");
        assert_eq!(first, start + 509);
        assert_eq!(next(&mut flash, &mut reader, start + 3), Read::Lost);
        // Opened again, the fetch carries on with all the log still has.
        let read = read_on(&mut flash, &mut reader, start + 3, clock);
        assert_eq!(read, buckets_of(&minutes, start + 3, clock));
        assert_eq!(read[0].1[0], minutes[0]);
    }

    #[test]
    fn a_sector_something_else_wrote_ends_the_log_there() {
        let mut flash = Nor::new(4);
        let mut writer = Writer::EMPTY;
        record(&mut flash, &mut writer, NOON..NOON + 3 * 510 + 100);
        // An older build's checkpoint lands on the second sector.
        let second = (START + SECTOR) as usize;
        for (i, byte) in flash.bytes[second..second + 4096].iter_mut().enumerate() {
            *byte = (i % 7) as u8;
        }
        let (mut found, _, minutes) = scan(&mut flash);
        // Only what's after it is the log; what was before it is lost
        // with it, rather than read as joined on.
        assert_eq!(minutes.first(), Some(&(NOON + 509 + 510)));
        assert_eq!(minutes.last(), Some(&(NOON + 3 * 510 + 99)));
        // And the log carries on, over that sector in its turn.
        record(&mut flash, &mut found, NOON + 3 * 510 + 100..NOON + 5 * 510);
        let (_, _, minutes) = scan(&mut flash);
        assert_eq!(minutes.last(), Some(&(NOON + 5 * 510 - 1)));
        assert!(minutes.windows(2).all(|pair| pair[1] == pair[0] + 1));
    }

    #[test]
    fn a_clock_set_back_past_a_sectors_start_still_fetches_from_before_it() {
        // Days with the clock a day ahead (the phone's was wrong), until
        // the ring has come round; then it's put right, and the watch
        // records on from the real time, in the newest sector.
        let mut flash = Nor::new(3);
        let ring = flash.ring;
        let mut writer = Writer::EMPTY;
        let ahead = NOON + 1440;
        record(&mut flash, &mut writer, ahead..ahead + 4 * 510);
        block_on(spill::truncate(&mut flash, ring, &mut writer, NOON)).expect("power");
        record(&mut flash, &mut writer, NOON..NOON + 30);
        // The sector began by saying its minutes start a day ahead, which
        // is no longer so: a fetch from the real time must not be told
        // that the log has nothing before then.
        let (_, first) = block_on(spill::open(&mut flash, ring, NOON)).expect("a log");
        assert!(first <= NOON, "told the log starts at {first}");
        let minutes: Vec<u32> = (NOON..NOON + 30).collect();
        assert_eq!(
            fetch(&mut flash, NOON, NOON + 30),
            buckets_of(&minutes, NOON, NOON + 30)
        );
        // Nor after a reset.
        let (mut found, _, scanned) = scan(&mut flash);
        assert_eq!(scanned, minutes);
        record(&mut flash, &mut found, NOON + 30..NOON + 40);
        let (_, first) = block_on(spill::open(&mut flash, ring, NOON)).expect("a log");
        assert!(first <= NOON, "told the log starts at {first}");
    }

    #[test]
    fn minutes_taken_back_go_in_again_as_they_are_then() {
        // Twenty minutes of a walk, logged as plain activity (worn: the
        // flags' bits 4 and 5 say 1). Then the watch recognises a
        // workout, marks its last ten minutes as exercise (those bits
        // say 3: see `activity::Sample::exercise`), and has the log take
        // them in again, from their bucket's start.
        const WORN: u8 = 0x40 | 0x10;
        const EXERCISE: u8 = 0x40 | 0x30;
        let walk = |minute: u32, flags: u8| [(minute % 200) as u8, 0, 9, 0, 0, flags];
        let mut flash = Nor::new(4);
        let ring = flash.ring;
        let mut writer = Writer::EMPTY;
        let start = NOON - NOON % 5;
        for minute in start..start + 20 {
            block_on(spill::append(
                &mut flash,
                ring,
                &mut writer,
                minute,
                &walk(minute, WORN),
            ))
            .expect("power");
        }
        block_on(spill::truncate(&mut flash, ring, &mut writer, start + 10)).expect("power");
        for minute in start + 10..start + 20 {
            let sample = walk(minute, EXERCISE);
            block_on(spill::append(
                &mut flash,
                ring,
                &mut writer,
                minute,
                &sample,
            ))
            .expect("power");
        }
        let expected: Vec<(u32, [u8; SAMPLE])> = (start..start + 20)
            .map(|minute| {
                (
                    minute,
                    walk(minute, if minute < start + 10 { WORN } else { EXERCISE }),
                )
            })
            .collect();

        // A boot reads each minute once, the later ones as exercise, flags
        // and all: the log keeps a sample's six bytes as they are.
        let mut read = Vec::new();
        block_on(spill::scan(&mut flash, ring, |minute, bytes| {
            read.push((minute, *bytes))
        }));
        assert_eq!(read, expected);

        // And so does a fetch.
        let (mut reader, _) = block_on(spill::open(&mut flash, ring, start)).expect("a log");
        let mut fetched = Vec::new();
        let mut from = start;
        loop {
            let mut want = Want {
                reader: &mut reader,
                from,
                end: start + 20,
                out: [None; BUCKET],
            };
            let Read::Bucket(first) = block_on(spill::bucket(&mut flash, ring, &mut want)) else {
                break;
            };
            fetched.extend(
                (first..)
                    .zip(want.out)
                    .filter_map(|(m, bytes)| Some((m, bytes?))),
            );
            from = first + BUCKET as u32;
        }
        assert_eq!(fetched, expected);
    }

    #[test]
    fn a_clock_set_back_takes_the_later_minutes_back() {
        let mut flash = Nor::new(4);
        let ring = flash.ring;
        let mut writer = Writer::EMPTY;
        record(&mut flash, &mut writer, NOON..NOON + 600);
        let erases = flash.erases.clone();
        // Back across a sector's start.
        block_on(spill::truncate(&mut flash, ring, &mut writer, NOON + 500)).expect("power");
        assert_eq!(writer.through(), Some(NOON + 499));
        // No erase: the entries are cleared in place.
        assert_eq!(flash.erases, erases);
        record(&mut flash, &mut writer, NOON + 500..NOON + 520);
        let (_, _, minutes) = scan(&mut flash);
        assert_eq!(minutes, (NOON..NOON + 520).collect::<Vec<_>>());
        assert_eq!(
            fetch(&mut flash, NOON, NOON + 520),
            buckets_of(&minutes, NOON, NOON + 520)
        );
    }

    // ==========================================================================
    // The log letting go of minutes, and the record that must hold them
    // ==========================================================================

    use super::snapshot;

    /// A record (the planned one, or a checkpoint) as far as this goes:
    /// the minutes it holds, up to the one it was saved as of. Its save
    /// is all or nothing (the header goes last: see `snapshot`).
    #[derive(Clone, Copy)]
    struct Record {
        first: u32,
        as_of: u32,
    }

    /// A checkpoint's room (`system_off::CHECKPOINT_ROOM`).
    const ROOM: usize =
        (snapshot::SLOT_BYTES as usize - snapshot::HEADER_BYTES) / snapshot::SAMPLE_BYTES - BUCKET;

    /// Saves a record of the history up to `newest`, from where the log
    /// ends (`logged`), if the power lasts.
    fn save(flash: &mut Nor, records: &mut Vec<Record>, newest: u32, logged: Option<u32>) {
        if flash.spend() {
            let first = snapshot::unlogged_from(newest.into(), logged.map(i64::from), ROOM);
            records.push(Record {
                first: first as u32,
                as_of: newest + 1,
            });
        }
    }

    /// One run of `system_off::serve` at which the log is due back to
    /// `back`, with nothing new to take in (it's all held): the record
    /// and the log's letting go, in the order `serve` has them
    /// (`record_first`), or the other way round, as it was. Stops where
    /// the power does.
    fn serve(
        flash: &mut Nor,
        writer: &mut Writer,
        records: &mut Vec<Record>,
        newest: u32,
        back: u32,
        record_first: bool,
    ) {
        let ring = flash.ring;
        let until = snapshot::taken_back(writer.through().map(i64::from), back.into());
        if record_first && let Some(until) = until {
            save(flash, records, newest, Some(until as u32));
        }
        if until.is_some() && block_on(spill::truncate(flash, ring, writer, back)).is_err() {
            return;
        }
        if !(record_first && until.is_some()) {
            save(flash, records, newest, writer.through());
        }
    }

    /// The minutes a boot puts back: the log's, and the newest record's.
    fn restored(flash: &mut Nor, records: &[Record]) -> Vec<u32> {
        let (_, _, mut minutes) = scan(flash);
        if let Some(record) = records.iter().max_by_key(|record| record.as_of) {
            minutes.extend(record.first..record.as_of);
        }
        minutes.sort_unstable();
        minutes.dedup();
        minutes
    }

    /// The morning after a night, settled and all in the log, with the
    /// last checkpoint holding only the bucket under way. Then a reset,
    /// which forgets the settling: at the boot's first run the night is
    /// held again, so the log is due back to the night's start (see
    /// `system_off::keep`). Whether a second reset anywhere in that run
    /// (the power going after any number of writes) leaves every minute
    /// in the log or a record, given the run's order.
    fn a_reset_while_the_log_lets_go_keeps_the_night(record_first: bool) -> bool {
        let night = NOON - NOON % 5;
        let logged = night + 8 * 60 + 45;
        let newest = logged + 3;
        let writes = {
            let mut flash = Nor::new(4);
            let mut writer = Writer::EMPTY;
            record(&mut flash, &mut writer, night..logged);
            let before = flash.programs;
            let mut records = Vec::new();
            serve(
                &mut flash,
                &mut writer,
                &mut records,
                newest,
                night,
                record_first,
            );
            // And the record's save.
            flash.programs - before + 1
        };
        (0..=writes + 1).all(|fuel| {
            let mut flash = Nor::new(4);
            let mut writer = Writer::EMPTY;
            record(&mut flash, &mut writer, night..logged);
            // The last checkpoint before the first reset: from where the
            // log ended.
            let mut records = Vec::new();
            save(&mut flash, &mut records, newest, writer.through());
            assert_eq!(records[0].first, logged);

            flash.fuel = Some(fuel);
            serve(
                &mut flash,
                &mut writer,
                &mut records,
                newest,
                night,
                record_first,
            );
            flash.power_on();
            restored(&mut flash, &records) == (night..=newest).collect::<Vec<_>>()
        })
    }

    #[test]
    fn a_reset_while_the_log_lets_go_of_a_night_loses_none_of_it() {
        assert!(a_reset_while_the_log_lets_go_keeps_the_night(true));
        // The log first, as it was: a reset before the record is saved
        // finds the night in neither. (What this test is for.)
        assert!(!a_reset_while_the_log_lets_go_keeps_the_night(false));
    }

    #[test]
    fn a_log_with_nothing_to_let_go_of_needs_no_record_first() {
        assert_eq!(snapshot::taken_back(None, 100), None);
        assert_eq!(snapshot::taken_back(Some(99), 100), None);
        // From its last minute on, or further back.
        assert_eq!(snapshot::taken_back(Some(100), 100), Some(99));
        assert_eq!(snapshot::taken_back(Some(700), 100), Some(99));
    }
}
