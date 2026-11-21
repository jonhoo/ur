//! The activity log in the SPI flash: every minute a fetch could have
//! sent, appended as it becomes one, so that what the phone hasn't
//! fetched outlives both the hours the in-RAM history holds (see
//! [`crate::activity`]: a day, when this was written; 14½ hours since,
//! as it no longer has to hold what's here) and a reset.
//!
//! Before this, the minutes the phone hadn't fetched lived in RAM alone,
//! with a copy of all of them rewritten to the flash every couple of
//! minutes for a crash (see [`crate::snapshot`]). A phone that didn't
//! fetch for more than a day lost what was older: on a trip, 33 hours
//! without a fetch lost 9½ hours of a day, 5,906 steps and 28 heart rate
//! readings. And the rewriting grew with the hours since the last fetch,
//! to 8.6 KB every 2 minutes.
//!
//! Now the watch does to the flash what a fetch does to the phone: as
//! each 5-minute bucket becomes one a fetch would send (it's over, and no
//! sleep that may still change holds it back; see [`crate::gb_buckets`]
//! and [`crate::sleep_hold`]), its minutes are appended here, once, and
//! never rewritten. A fetch then reads from here first, oldest first, and
//! from RAM for what's newer. Only what a fetch would hold back, the
//! night under way, is still rewritten for a crash, as a checkpoint.
//!
//! This module is the log's format and its reading and writing, over any
//! flash ([`Flash`]), so that it runs on a computer, against a model of
//! NOR flash with resets in mid-write (`host-tests/spill`). Which minutes
//! go in, and when, is [`crate::system_off`]'s.
//!
//! ## Format
//!
//! A ring of erase sectors ([`Ring`]), each [`SLOTS`] entries of
//! [`ENTRY`] bytes: six of payload, a tag, and a check byte over those
//! seven. An entry never straddles a flash page, so each is one program,
//! and a reset in the middle of one leaves at most that entry wrong,
//! which its check byte then says.
//!
//! | Tag | Payload | |
//! |---|---|---|
//! | header | sequence (4), `uS` | a sector's first entry |
//! | at | minute (4), 0, 0 | the next sample is this minute's |
//! | sample | [`crate::activity::Sample`]'s bytes | a minute; the next sample is the minute after's |
//! | synced | minute (4), 0, 0 | the phone has everything up to this minute |
//!
//! Erased flash (all ones) is the log's end, and all zeros is an entry
//! taken back ([`truncate`]): NOR flash can always clear bits, so that
//! needs no erase. After either of those, or an entry that fails its
//! check, samples count for nothing until the next "at" says which
//! minute they are; the writer says so first thing in every sector (which
//! is also how a fetch finds the sector to start in), and after every
//! boot.
//!
//! Sectors are numbered as they're taken into use, the sequence in each
//! one's header; sequence `n` lives in sector `n` modulo the ring's length. The log is
//! the unbroken run of sequences ending at the highest, so the oldest
//! sector is the next to be erased, and a sector some other firmware
//! wrote over (an older build's checkpoints lived here) ends the run
//! there rather than joining it.
//!
//! Minutes with nothing recorded (the watch off, or the clock unknown)
//! take no entry. Samples are in order of their minutes: when the clock
//! goes back, the samples from the new time on are taken back first
//! ([`truncate`]), as the history drops them (see [`crate::minute_ring`]).
//!
//! ## Size and wear
//!
//! A sector holds 510 minutes at most, so the ring's 32 hold 11 days when
//! every minute has a sample, with one sector always about to be erased.
//! A sector is erased once each time the ring comes round: once in 11
//! days, against the 100,000 erases the chip is rated for.

/// The SPI flash's erase sector (`system_off` checks it's the driver's).
pub const SECTOR: u32 = 4096;
/// One entry.
pub const ENTRY: usize = 8;
/// One sample: [`crate::activity::Sample`]'s bytes.
pub const SAMPLE: usize = 6;
/// Entries in a sector, its header among them.
pub const SLOTS: u16 = (SECTOR as usize / ENTRY) as u16;

/// Where the ring is: `sectors` erase sectors from `start`. (The watch
/// has one, `system_off::SPILL`; the host tests use small ones, to come
/// round often.)
#[derive(Clone, Copy, Debug)]
pub struct Ring {
    pub start: u32,
    pub sectors: u32,
}

// Each entry lies within one 256-byte page: a program can't cross one.
const _: () = assert!(256usize.is_multiple_of(ENTRY) && (SECTOR as usize).is_multiple_of(ENTRY));
const _: () = assert!(
    SLOTS.is_multiple_of(8),
    "`scan` reads eight entries at a time"
);

/// The flash the log is in: [`crate::spi_flash::SpiFlash`] on the watch.
// Only ever used with concrete types, in one executor: the futures needn't
// be `Send`, which is all the lint is about.
#[allow(async_fn_in_trait)]
pub trait Flash {
    type Error;
    async fn read(&mut self, address: u32, data: &mut [u8]);
    /// Programs `data`, which lies within one page of erased (or, to
    /// clear bits, programmed) flash.
    async fn program(&mut self, address: u32, data: &[u8]) -> Result<(), Self::Error>;
    /// Erases the sector at `address`.
    async fn erase(&mut self, address: u32) -> Result<(), Self::Error>;
}

// The tags: any but 0 (taken back) and 0xFF (erased).
const HEADER: u8 = 0x48;
const AT: u8 = 0x41;
const SAMPLE_TAG: u8 = 0x53;
const SYNCED: u8 = 0x59;
/// The rest of a header's payload.
const MAGIC: [u8; 2] = *b"uS";

/// What an entry's eight bytes say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    /// Erased: nothing written yet.
    Blank,
    /// Taken back, or cut short by a reset: nothing to read, and the
    /// samples after it are of unknown minutes.
    Void,
    Header(u32),
    At(u32),
    Sample([u8; SAMPLE]),
    Synced(u32),
}

/// The check byte over an entry's first seven: FNV-1a, folded. All that's
/// asked of it is to tell a whole entry from one a reset cut short.
fn check(bytes: &[u8]) -> u8 {
    let hash = crate::snapshot::checksum(crate::snapshot::CHECKSUM_SEED, bytes);
    (hash ^ (hash >> 8) ^ (hash >> 16) ^ (hash >> 24)) as u8
}

fn encode(tag: u8, payload: [u8; SAMPLE]) -> [u8; ENTRY] {
    let mut bytes = [0; ENTRY];
    bytes[..SAMPLE].copy_from_slice(&payload);
    bytes[SAMPLE] = tag;
    bytes[SAMPLE + 1] = check(&bytes[..SAMPLE + 1]);
    bytes
}

fn minute_payload(minute: u32) -> [u8; SAMPLE] {
    let [a, b, c, d] = minute.to_le_bytes();
    [a, b, c, d, 0, 0]
}

fn decode(bytes: &[u8; ENTRY]) -> Entry {
    if *bytes == [0xFF; ENTRY] {
        return Entry::Blank;
    }
    if bytes[SAMPLE + 1] != check(&bytes[..SAMPLE + 1]) {
        return Entry::Void;
    }
    let payload: [u8; SAMPLE] = bytes[..SAMPLE].try_into().expect("SAMPLE of ENTRY bytes");
    let [a, b, c, d, e, f] = payload;
    let word = u32::from_le_bytes([a, b, c, d]);
    match bytes[SAMPLE] {
        HEADER if [e, f] == MAGIC => Entry::Header(word),
        AT if [e, f] == [0, 0] => Entry::At(word),
        SYNCED if [e, f] == [0, 0] => Entry::Synced(word),
        SAMPLE_TAG => Entry::Sample(payload),
        _ => Entry::Void,
    }
}

/// Whether a sample holds anything: the accelerometer saw the minute, or
/// a heart rate was read in it (as `activity::History::since` asks).
pub fn recorded(sample: &[u8; SAMPLE]) -> bool {
    sample[5] & 0x40 != 0 || sample[4] != 0
}

/// Where entry `slot` of the sector with sequence `sequence` is.
fn address(ring: Ring, sequence: u32, slot: u16) -> u32 {
    ring.start + (sequence % ring.sectors) * SECTOR + u32::from(slot) * ENTRY as u32
}

async fn read_entry<F: Flash>(flash: &mut F, ring: Ring, sequence: u32, slot: u16) -> Entry {
    let mut bytes = [0; ENTRY];
    flash.read(address(ring, sequence, slot), &mut bytes).await;
    decode(&bytes)
}

/// Whether the sector for `sequence` holds it (and not an older turn of
/// the ring, or something else's bytes).
async fn holds<F: Flash>(flash: &mut F, ring: Ring, sequence: u32) -> bool {
    read_entry(flash, ring, sequence, 0).await == Entry::Header(sequence)
}

/// The log's sectors: the sequences of its oldest and newest, if it has
/// any.
async fn bounds<F: Flash>(flash: &mut F, ring: Ring) -> Option<(u32, u32)> {
    let mut newest = None;
    for sector in 0..ring.sectors {
        if let Entry::Header(sequence) = read_entry(flash, ring, sector, 0).await
            && sequence % ring.sectors == sector
            && newest.is_none_or(|newest| sequence > newest)
        {
            newest = Some(sequence);
        }
    }
    let newest = newest?;
    let mut oldest = newest;
    while oldest > 0 && newest - oldest < ring.sectors - 1 && holds(flash, ring, oldest - 1).await {
        oldest -= 1;
    }
    Some((oldest, newest))
}

// ==============================================================================
// Writing
// ==============================================================================

/// Where the log ends, for appending to it. Kept in RAM by the one task
/// that writes (16 bytes), and found again at boot by [`scan`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Writer {
    /// The newest sector's sequence (one before the first, with no log).
    sequence: u32,
    /// The minute the next sample entry would be read as, or 0 if an "at"
    /// has to say first.
    at: u32,
    /// Every minute up to this one that belongs in the log is in it; 0
    /// with nothing in it. The caller's to move on past minutes with
    /// nothing recorded ([`Writer::passed`]).
    through: u32,
    /// The first free entry of that sector ([`SLOTS`] if it's full).
    slot: u16,
}

impl Writer {
    /// A log with nothing in it yet (whatever the flash holds).
    pub const EMPTY: Self = Self {
        sequence: u32::MAX,
        at: 0,
        through: 0,
        slot: SLOTS,
    };

    /// A log that hasn't been read yet: [`scan`] first, as appending
    /// from [`Writer::EMPTY`] would start a new log over it.
    pub const UNREAD: Self = Self {
        slot: 0,
        ..Self::EMPTY
    };

    pub fn is_unread(&self) -> bool {
        self.slot == 0
    }

    /// The last minute the log is complete up to, if it holds any.
    pub fn through(&self) -> Option<u32> {
        (self.through != 0).then_some(self.through)
    }

    /// Notes that the minutes up to `minute` are all in the log that ever
    /// will be: those since the last sample had nothing recorded.
    pub fn passed(&mut self, minute: u32) {
        self.through = self.through.max(minute);
    }
}

impl Writer {
    /// Where the next entry goes, moving on to the one after whether or
    /// not the write then works: a slot that may be half-written is never
    /// written again, and reads as void.
    fn place(&mut self, ring: Ring) -> u32 {
        let address = address(ring, self.sequence, self.slot);
        self.slot += 1;
        address
    }

    /// After a write that failed: the next sample says its minute.
    fn failed<E>(&mut self, written: Result<(), E>) -> Result<(), E> {
        if written.is_err() {
            self.at = 0;
        }
        written
    }
}

// The writes below call the flash's `program` themselves rather than
// through a helper, as each `async fn` in between is a future around the
// next, with a copy of its arguments: bytes of the writing task's static
// RAM.

/// Takes the next sector into use if the newest is full: erases it (the
/// log's oldest, once the ring has come round) and writes its header,
/// and the minute after the last in the log.
async fn roll<F: Flash>(flash: &mut F, ring: Ring, writer: &mut Writer) -> Result<(), F::Error> {
    if writer.slot < SLOTS {
        return Ok(());
    }
    let sequence = writer.sequence.wrapping_add(1);
    let sector = address(ring, sequence, 0);
    // Out of the log before it's erased: an erase cut short by a reset
    // could leave its old header readable over entries that no longer
    // are, a hole in the oldest sector rather than its end. (Clearing a
    // header that's already erased, or not ours, does nothing.)
    flash.program(sector, &[0; ENTRY]).await?;
    flash.erase(sector).await?;
    let [a, b, c, d] = sequence.to_le_bytes();
    let header = encode(HEADER, [a, b, c, d, MAGIC[0], MAGIC[1]]);
    flash.program(sector, &header).await?;
    *writer = Writer {
        sequence,
        at: 0,
        through: writer.through,
        slot: 1,
    };
    // Every sector starts by saying which minute its first sample would
    // be: none of its samples is older, which is how a fetch finds where
    // to start reading ([`open`]) without reading all that's before.
    let next = writer.through + 1;
    let address = writer.place(ring);
    let written = flash
        .program(address, &encode(AT, minute_payload(next)))
        .await;
    writer.failed(written)?;
    writer.at = next;
    Ok(())
}

/// Appends `minute`'s sample. Minutes go in in order: `minute` is after
/// every sample in the log ([`truncate`] first, if the clock went back).
pub async fn append<F: Flash>(
    flash: &mut F,
    ring: Ring,
    writer: &mut Writer,
    minute: u32,
    sample: &[u8; SAMPLE],
) -> Result<(), F::Error> {
    loop {
        roll(flash, ring, writer).await?;
        // Its minute first, unless it follows the last sample's.
        let says = writer.at != minute;
        let entry = if says {
            encode(AT, minute_payload(minute))
        } else {
            encode(SAMPLE_TAG, *sample)
        };
        let address = writer.place(ring);
        let written = flash.program(address, &entry).await;
        writer.failed(written)?;
        if says {
            writer.at = minute;
            // Which may have been the sector's last entry.
            continue;
        }
        writer.at = minute + 1;
        writer.through = writer.through.max(minute);
        return Ok(());
    }
}

/// Notes in the log that the phone has everything up to `minute`: the
/// last such note is where the next fetch starts, after a reset too.
pub async fn synced<F: Flash>(
    flash: &mut F,
    ring: Ring,
    writer: &mut Writer,
    minute: u32,
) -> Result<(), F::Error> {
    roll(flash, ring, writer).await?;
    let address = writer.place(ring);
    let written = flash
        .program(address, &encode(SYNCED, minute_payload(minute)))
        .await;
    writer.failed(written)
}

/// Takes back the samples from minute `from` on, after the clock went
/// back to it: each is cleared in place, newest sector first, back to
/// the first sector with an older sample. Cut short (a reset, or the
/// flash failing), it leaves newer sectors done and older ones not; the
/// writer's `through` only goes back once all are, so the caller tries
/// again while the log reaches past the history.
pub async fn truncate<F: Flash>(
    flash: &mut F,
    ring: Ring,
    writer: &mut Writer,
    from: u32,
) -> Result<(), F::Error> {
    let mut sequence = writer.sequence;
    // No further back than the ring is long, nor into sectors that aren't
    // (or are no longer) the log's.
    for _ in 0..ring.sectors {
        if !holds(flash, ring, sequence).await {
            break;
        }
        // The newest sector began by saying that none of its minutes
        // would be before a certain one, which a fetch goes by ([`open`]).
        // If that's after `from`, the minutes appended next would make
        // it wrong, so it's taken back too: the sector then doesn't say.
        if sequence == writer.sequence
            && let Entry::At(first) = read_entry(flash, ring, sequence, 1).await
            && first > from
        {
            flash
                .program(address(ring, sequence, 1), &[0; ENTRY])
                .await?;
        }
        let (mut at, mut older) = (0, false);
        for slot in 1..SLOTS {
            match read_entry(flash, ring, sequence, slot).await {
                Entry::Blank => break,
                Entry::At(minute) => at = minute,
                Entry::Sample(_) if at != 0 => {
                    if at >= from {
                        let address = address(ring, sequence, slot);
                        flash.program(address, &[0; ENTRY]).await?;
                    } else {
                        older = true;
                    }
                    at += 1;
                }
                Entry::Void => at = 0,
                _ => {}
            }
        }
        if older {
            break;
        }
        sequence = sequence.wrapping_sub(1);
    }
    // The next sample follows entries taken back.
    writer.at = 0;
    writer.through = writer.through.min(from.saturating_sub(1));
    Ok(())
}

// ==============================================================================
// Reading it all, at boot
// ==============================================================================

/// Reads the whole log, oldest first: `each` gets every sample and its
/// minute, in order. Returns where to append from then on, and the last
/// note of what the phone has, if any.
pub async fn scan<F: Flash>(
    flash: &mut F,
    ring: Ring,
    mut each: impl FnMut(u32, &[u8; SAMPLE]),
) -> (Writer, Option<u32>) {
    let Some((oldest, newest)) = bounds(flash, ring).await else {
        return (Writer::EMPTY, None);
    };
    let mut writer = Writer {
        sequence: newest,
        // After a boot, an "at" first: the last entry may be one a reset
        // cut short in a way its check byte happens to miss.
        at: 0,
        through: 0,
        slot: 1,
    };
    let mut synced = None;
    // A page's worth of entries at a time: a full log is 16,000 of them,
    // and one read apiece would keep a boot waiting for seconds.
    const AT_ONCE: usize = 8;
    let mut bytes = [0; AT_ONCE * ENTRY];
    for sequence in oldest..=newest {
        let mut at = 0;
        'sector: for first in (0..SLOTS).step_by(AT_ONCE) {
            flash.read(address(ring, sequence, first), &mut bytes).await;
            for (slot, entry) in (first..).zip(bytes.as_chunks::<ENTRY>().0) {
                // The header, which `bounds` has read.
                if slot == 0 {
                    continue;
                }
                match decode(entry) {
                    Entry::Blank => {
                        // Written in order, so the rest is blank too.
                        // (Were it not, the entries appended over it
                        // would fail their checks, and cost those
                        // minutes, no more.)
                        break 'sector;
                    }
                    Entry::At(minute) => at = minute,
                    Entry::Sample(sample) if at != 0 => {
                        each(at, &sample);
                        writer.through = at;
                        at += 1;
                    }
                    Entry::Synced(minute) => synced = Some(minute),
                    Entry::Void => at = 0,
                    _ => {}
                }
                if sequence == newest {
                    writer.slot = slot + 1;
                }
            }
        }
    }
    (writer, synced)
}

// ==============================================================================
// Reading buckets, for a fetch
// ==============================================================================
//
// These run in the Bluetooth task, whose future (static RAM) has no room
// for more than its sends take: so each is one flat loop around the
// flash's read, with a few words of state.

/// A fetch's place in the log.
#[derive(Clone, Copy, Debug)]
pub struct Reader {
    sequence: u32,
    /// The minute of the next sample entry, or 0 before an "at" says.
    at: u32,
    slot: u16,
}

/// A reader for a fetch of the minutes from `from` on, and a minute that
/// none it will read is before: at or before `from`, unless the log
/// starts later, and then the log has nothing older (0 if it can't say:
/// see below). `None` if there's no log.
pub async fn open<F: Flash>(flash: &mut F, ring: Ring, from: u32) -> Option<(Reader, u32)> {
    let mut bytes = [0; ENTRY];
    // The newest sector's sequence plus one: 0 for none, to keep this
    // future's few words few.
    let mut after = 0u32;
    for sector in 0..ring.sectors {
        flash.read(ring.start + sector * SECTOR, &mut bytes).await;
        if let Entry::Header(sequence) = decode(&bytes)
            && sequence % ring.sectors == sector
            && sequence >= after
        {
            after = sequence + 1;
        }
    }
    // From the newest sector back, to the first that starts at or before
    // `from`: the minutes from `from` on are in it or after it.
    //
    // `lowest` is the least of the minutes those sectors start with: none
    // of their minutes is before it. 0 once one of them doesn't say.
    let (mut sequence, mut lowest) = (after, u32::MAX);
    while sequence > 0 && after - sequence < ring.sectors {
        flash.read(address(ring, sequence - 1, 0), &mut bytes).await;
        if decode(&bytes) != Entry::Header(sequence - 1) {
            break;
        }
        sequence -= 1;
        // The writer starts every sector by saying which minute is next.
        // One that doesn't start so (a reset cut that entry short, or it
        // was taken back: see [`truncate`]) may hold any minute: reading
        // may have to start there, but only an older sector can say it
        // needn't start earlier still.
        flash.read(address(ring, sequence, 1), &mut bytes).await;
        let first = match decode(&bytes) {
            Entry::At(minute) => minute,
            _ => 0,
        };
        lowest = lowest.min(first);
        if first != 0 && first <= from {
            break;
        }
    }
    // No sector read: no log.
    (sequence < after).then_some((
        Reader {
            sequence,
            at: 0,
            slot: 1,
        },
        lowest,
    ))
}

fn as_entry(bytes: &[u8]) -> &[u8; ENTRY] {
    bytes.try_into().expect("ENTRY bytes")
}

/// What [`bucket`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// A bucket, from this minute.
    Bucket(u32),
    /// No more for now. The reader stays where it is, so a later call
    /// finds what's been appended since.
    End,
    /// The writer has erased the sector being read: a fetch of a full log
    /// that was slower than the ring turns. [`open`] again to carry on,
    /// from the log's oldest sector.
    Lost,
}

/// What [`bucket`] is asked for, and where it puts it: in one place, so
/// that the futures on the way there carry a pointer to it rather than
/// each a copy.
pub struct Want<'a, const N: usize> {
    pub reader: &'a mut Reader,
    /// Nothing before this minute,
    pub from: u32,
    /// nor from this one on.
    pub end: u32,
    /// The bucket's minutes' samples.
    pub out: [Option<[u8; SAMPLE]>; N],
}

/// The next bucket of `N` minutes (aligned to multiples of it) with
/// anything recorded at or after minute `want.from` and before
/// `want.end`: its first minute, with each of its minutes' samples in
/// `want.out`.
pub async fn bucket<F: Flash, const N: usize>(
    flash: &mut F,
    ring: Ring,
    want: &mut Want<'_, N>,
) -> Read {
    {
        let mut header = [0; ENTRY];
        flash
            .read(address(ring, want.reader.sequence, 0), &mut header)
            .await;
        if decode(&header) != Entry::Header(want.reader.sequence) {
            return Read::Lost;
        }
    }
    let mut found = Read::End;
    // A few entries at a read: each read is most of a millisecond, with
    // the phone waiting, and a bucket is up to N entries and a couple
    // more (a week's fetch is 2016 buckets).
    const AT_ONCE: u16 = 4;
    let mut bytes = [0; AT_ONCE as usize * ENTRY];
    'log: loop {
        let reader = &mut *want.reader;
        if reader.slot >= SLOTS {
            let next = reader.sequence.wrapping_add(1);
            flash
                .read(address(ring, next, 0), &mut bytes[..ENTRY])
                .await;
            if decode(as_entry(&bytes[..ENTRY])) != Entry::Header(next) {
                break;
            }
            let reader = &mut *want.reader;
            (reader.sequence, reader.slot, reader.at) = (next, 1, 0);
        }
        let reader = &mut *want.reader;
        let entries = AT_ONCE.min(SLOTS - reader.slot);
        let read = &mut bytes[..usize::from(entries) * ENTRY];
        flash
            .read(address(ring, reader.sequence, reader.slot), read)
            .await;
        let reader = &mut *want.reader;
        for entry in read.as_chunks::<ENTRY>().0 {
            match decode(entry) {
                Entry::Blank => break 'log,
                Entry::At(minute) => reader.at = minute,
                Entry::Sample(sample) if reader.at != 0 => {
                    let minute = reader.at;
                    // Left for the next call, or the next fetch.
                    if minute >= want.end
                        || matches!(found, Read::Bucket(first) if minute >= first + N as u32)
                    {
                        break 'log;
                    }
                    reader.at += 1;
                    if minute >= want.from && recorded(&sample) {
                        let first = match found {
                            Read::Bucket(first) => first,
                            _ => {
                                want.out = [None; N];
                                minute - minute % N as u32
                            }
                        };
                        found = Read::Bucket(first);
                        // In order, so never before the bucket's first.
                        if let Some(place) = want.out.get_mut((minute - first) as usize) {
                            *place = Some(sample);
                        }
                    }
                }
                Entry::Void => reader.at = 0,
                _ => {}
            }
            reader.slot += 1;
        }
    }
    found
}
