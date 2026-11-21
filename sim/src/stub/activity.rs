//! A stand-in for `src/activity.rs`, which reads the accelerometer: just
//! what the UI asks of it, from state the simulator sets (steps walked,
//! heart rates, last night's sleep, whether the wearer is asleep).
//!
//! The per-minute history is kept the way the real one is (UTC minutes,
//! steps and a heart rate each), so the Today screen adds it up through the
//! firmware's own [`crate::day::Day`]: by the minute for the 14½ hours the
//! watch's history holds, and through the firmware's hourly sums
//! ([`crate::aged`]) for what's older, as the watch does (see
//! [`day_so_far`]). All of it stays here, though, standing for the
//! watch's activity log too, which a fetch reads the older minutes from.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::clock;

/// A night's (or nap's) sleep: a copy of `activity::Sleep`, which the UI
/// shows on the Last night screen.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct Sleep {
    pub start_utc_minute: i64,
    /// The minute after the last asleep one.
    pub end_utc_minute: i64,
    pub asleep_minutes: u16,
    pub deep_minutes: u16,
    pub rem_minutes: u16,
}

#[derive(Default)]
struct State {
    /// UTC minute → (steps, heart rate or 0).
    history: BTreeMap<i64, (u16, u8)>,
    /// Today's steps: (local day since 1970, steps).
    today: Option<(i64, u32)>,
    /// As `activity::steps_counted`.
    counted: u32,
    last_sleep: Option<Sleep>,
    asleep: bool,
    movement: u16,
    /// As `activity::on_desk`.
    desk: bool,
    /// As `activity::light`: the minutes before were light sleep.
    light: bool,
    /// A dev kit's day, which `today` shows instead (see `devkit`).
    today_from_devkit: Option<Option<crate::day::Day>>,
    /// The minute the last [`poll`] saw, as the activity task's.
    polled: Option<i64>,
    /// The SPI flash's checkpoint slots, by slot (see `system_off`), as
    /// the firmware's own records (`snapshot`). Outlive a [`crash`], as
    /// flash does.
    checkpoints: BTreeMap<u32, Vec<u8>>,
    /// The planned record's as-of minute (see `snapshot::as_of_any`), once
    /// the watch has switched off: the simulator keeps the history across
    /// that wholesale, but the record's date still rules out the
    /// checkpoints from before it.
    planned: Option<u32>,
}

static STATE: Mutex<State> = Mutex::new(State {
    history: BTreeMap::new(),
    today: None,
    counted: 0,
    last_sleep: None,
    asleep: false,
    movement: 0,
    desk: false,
    light: false,
    today_from_devkit: None,
    polled: None,
    checkpoints: BTreeMap::new(),
    planned: None,
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().expect("nothing panics while holding it")
}

pub fn asleep() -> bool {
    state().asleep
}

/// Whether the accelerometer finds the watch perfectly still: never here,
/// as the simulated wearer has it on.
///
/// TODO: a command for a watch left lying on a desk, which the
/// temperature's wear model notices (see `temperature::Wear`).
pub fn still() -> bool {
    false
}

pub fn movement() -> u16 {
    state().movement
}

/// Whether the watch lies on a desk, off the wrist, as `desk` (the
/// command) says: the simulator has no accelerometer to find one.
pub fn on_desk() -> bool {
    state().desk
}

/// Whether the wrist stirred lately: here, while `movement` is a stir's.
pub fn stirring() -> bool {
    state().movement > crate::desk::STIR
}

pub fn light() -> bool {
    state().light
}

pub fn last_sleep() -> Option<Sleep> {
    state().last_sleep
}

/// Today's steps, once the date is known, by the firmware's rule (see
/// `day::steps_today`): none yet is 0, as on the watch, whose step
/// counter's every poll counts towards today. Brought up to date first,
/// as `activity::steps_today` does (the watch's polls do too, but the
/// simulator has none).
pub fn steps_today() -> Option<u32> {
    let now = now_minute()?;
    let mut state = state();
    state.today = on(&state, now);
    Some(state.today.map_or(0, |(_, steps)| steps))
}

/// As `activity::steps_counted`: a running count, for live reports.
pub fn steps_counted() -> u32 {
    state().counted
}

/// Now, as UTC minutes since 1970 and the UTC offset, once the time is
/// set.
fn now_minute() -> Option<(i64, i16)> {
    Some((
        clock::now_utc_seconds()?.div_euclid(60),
        clock::utc_offset_minutes()?,
    ))
}

/// Today's total as of `now`, as `activity::Today::on` has it: the
/// history's since local midnight on a new date.
fn on(state: &State, (utc_minute, offset): (i64, i16)) -> Option<(i64, u32)> {
    let (counted_on, steps) = state.today?;
    let day = crate::day::date(utc_minute, offset);
    let steps = crate::day::steps_today(Some(counted_on), steps, day, || {
        day_so_far(state, utc_minute, offset)
            .hours
            .iter()
            .map(|hour| u32::from(hour.steps))
            .sum()
    });
    Some((day, steps))
}

/// The hours whose sums the watch keeps of the minutes its history has
/// let go of (`activity::AGED_HOURS`).
const AGED_HOURS: usize = (24 * 60 - crate::minute_ring::MINUTES).div_ceil(60) + 1;

/// As `activity::History::day`: the day so far, hour by hour, from the
/// minutes the watch's history holds (the last
/// [`crate::minute_ring::MINUTES`]), and, for those it has let go of, the
/// sums [`crate::aged`] keeps of them.
///
/// The sums are made afresh here, of the minutes that would have left
/// the history by now, in the zone the wearer is in now: so, unlike the
/// watch's, they never straddle the hours after a move between zones
/// that aren't whole hours apart (see `aged`'s TODO).
fn day_so_far(state: &State, utc_minute: i64, offset: i16) -> crate::day::Day {
    let midnight = crate::day::midnight(utc_minute, offset);
    let kept_from = utc_minute - crate::minute_ring::MINUTES as i64 + 1;
    let mut aged = crate::aged::Aged::<AGED_HOURS>::new();
    // (Since midnight is as far back as today reaches.)
    let older = midnight..kept_from.max(midnight);
    for (&minute, &(steps, heart_rate)) in state.history.range(older) {
        aged.take(
            minute,
            offset,
            steps,
            heart_rate,
            crate::sleep::Kind::Unknown,
        );
    }
    let mut day = crate::day::Day::default();
    aged.add_to(&mut day, midnight, offset);
    for (&minute, &(steps, heart_rate)) in state.history.range(midnight.max(kept_from)..=utc_minute)
    {
        day.add((minute - midnight) as usize, steps, heart_rate);
    }
    day
}

/// As `activity::today`: the history since local midnight, hour by hour.
pub fn today() -> Option<crate::day::Day> {
    if let Some(day) = state().today_from_devkit {
        return day;
    }
    let (utc_minute, offset) = now_minute()?;
    Some(day_so_far(&state(), utc_minute, offset))
}

// ==============================================================================
// What the simulator sets
// ==============================================================================

/// Counts `steps` as walked now, as `activity::walk` does (in today's total
/// and this minute's history).
pub fn walk(steps: u32) {
    let now = now_minute();
    let mut state = state();
    state.counted = state.counted.wrapping_add(steps);
    // Before the time is known, nothing can be filed under a day.
    let Some(now) = now else {
        return;
    };
    add(&mut state, now, steps);
    let sample = state.history.entry(now.0).or_default();
    sample.0 = sample
        .0
        .saturating_add(steps.min(u32::from(u16::MAX)) as u16);
}

/// Adds steps to today's total only, for steps already in the history.
pub fn add_to_today(steps: u32) {
    let Some(now) = now_minute() else {
        return;
    };
    let mut state = state();
    let day = crate::day::date(now.0, now.1);
    state.today = match state.today {
        Some((counted_on, total)) if counted_on == day => Some((day, total + steps)),
        // Another day's total: today's is the history's, theirs included.
        Some(_) => on(&state, now),
        None => Some((day, steps)),
    };
}

/// Adds `steps` to today's total as of `now` (before they're in the
/// history, as `activity::add_steps_today` does).
fn add(state: &mut State, now: (i64, i16), steps: u32) {
    let (day, total) = on(state, now).unwrap_or((crate::day::date(now.0, now.1), 0));
    state.today = Some((day, total.saturating_add(steps)));
}

/// As `activity::record_heart_rate`: a reading in the current minute's
/// sample, if the time is known.
pub fn record_heart_rate(bpm: u8) {
    if let Some(second) = clock::now_utc_seconds() {
        record_heart_rate_at(second.div_euclid(60), bpm);
    }
}

/// Records a heart rate reading in a UTC minute's sample.
pub fn record_heart_rate_at(utc_minute: i64, bpm: u8) {
    state().history.entry(utc_minute).or_default().1 = bpm;
}

/// Records steps in a UTC minute's sample, without touching today's total
/// (for filling in a past day).
pub fn record_steps_at(utc_minute: i64, steps: u16) {
    let mut state = state();
    let sample = state.history.entry(utc_minute).or_default();
    sample.0 = sample.0.saturating_add(steps);
}

/// The history's minutes from `from` (UTC minute) on, oldest first: (steps,
/// heart rate or 0) each.
pub fn history_since(from: i64) -> Vec<(i64, (u16, u8))> {
    state()
        .history
        .range(from..)
        .map(|(&minute, &sample)| (minute, sample))
        .collect()
}

/// Shows a dev kit's day from now on, instead of the history here.
#[cfg(not(target_arch = "wasm32"))]
pub fn show_devkit_day(day: Option<crate::day::Day>) {
    state().today_from_devkit = Some(day);
}

pub fn set_last_sleep(sleep: Option<Sleep>) {
    state().last_sleep = sleep;
}

pub fn set_asleep(asleep: bool) {
    state().asleep = asleep;
}

pub fn set_movement(movement: u16) {
    state().movement = movement;
}

pub fn set_desk(desk: bool) {
    state().desk = desk;
}

pub fn set_light(light: bool) {
    state().light = light;
}

// ==============================================================================
// Surviving a crash
// ==============================================================================
//
// On the watch, `system_off` keeps the history in the SPI flash: every
// couple of minutes it appends the minutes a fetch could send to a log
// (`spill`), and checkpoints the rest (what a fetch holds back, and the
// bucket under way); a boot after a crash (or the watchdog) reads the log
// back, and restores the newest checkpoint on top, in the firmware's own
// format (`snapshot`).
//
// The simulator has no SPI flash, and no log: its checkpoint stands for
// both where they differ from RAM, holding everything the phone hasn't
// fetched (as the watch's did up to 0.11.3-rc.4), with the same code
// choosing where it goes and which one a boot restores; and a crash keeps
// the history from before it, as the log does. The samples are the
// simulator's history (steps and heart rates), and the minutes of the
// last night as its stages, as the watch's history holds them (light,
// deep and REM minutes, and restless ones at its end).
//
// TODO: model the log itself (`spill` runs on a computer): what a fetch
// holds back, the eleven days it keeps, and a fetch that reads it.

/// A `snapshot` sample's flags (as `activity::Sample`'s): recorded, and
/// worn.
const SEEN_AND_WORN: u8 = 0x40 | 0x10;

/// As the activity task does each minute: a checkpoint when one is due.
/// Call at every tick of the UI loop.
pub fn poll() {
    let Some(minute) = clock::now_utc_seconds().map(|second| second.div_euclid(60)) else {
        return;
    };
    let mut state = state();
    if state.polled.replace(minute) == Some(minute) || !crate::snapshot::checkpoint_due(minute) {
        return;
    }
    let slot = crate::snapshot::checkpoint_slot(minute);
    let record = record(&state, minute);
    state.checkpoints.insert(slot, record);
}

/// What `system_off::save` writes and the log holds past the phone's
/// copy, with `newest` the history's newest minute: the minutes from
/// where the phone's next fetch starts.
fn record(state: &State, newest: i64) -> Vec<u8> {
    use crate::snapshot::{self, Header};
    // The phone's copy standing for the log's end (see above).
    let synced = crate::script::synced_through().filter(|&synced| synced <= newest);
    let from = snapshot::unlogged_from(newest, synced, snapshot::MAX_MINUTES);
    let synced = synced.is_some();
    let from = crate::gb_buckets::start_of(from).max(newest - snapshot::MAX_MINUTES as i64 + 1);
    let kinds = night_kinds(state);
    let has_data = |minute: i64| state.history.contains_key(&minute) || kinds.contains_key(&minute);
    let (first, minutes) = match (from..=newest).find(|&minute| has_data(minute)) {
        Some(first) => (first, newest - first + 1),
        None => (i64::from(snapshot::empty_from(Some(newest))), 0),
    };
    let mut samples = Vec::new();
    for minute in first..first + minutes {
        let (steps, heart_rate) = state.history.get(&minute).copied().unwrap_or_default();
        let flags = match kinds.get(&minute) {
            Some(&kind) => SEEN_AND_WORN | kind as u8,
            None if has_data(minute) => SEEN_AND_WORN | crate::sleep::Kind::Awake as u8,
            None => 0,
        };
        let [s0, s1] = steps.to_le_bytes();
        samples.extend_from_slice(&[s0, s1, 0, 0, heart_rate, flags]);
    }
    let (day, steps) = state.today.unwrap_or((i64::from(snapshot::UNKNOWN_DAY), 0));
    let mut header = Header {
        first_minute: first as u32,
        minutes: minutes as u16,
        day: day as i32,
        steps,
        checksum: 0,
        synced,
    };
    header.checksum =
        header.expected_checksum(snapshot::checksum(snapshot::CHECKSUM_SEED, &samples));
    [&header.to_bytes()[..], &samples].concat()
}

/// The last night's minutes, as the watch's history holds them: deep,
/// then REM, then light, then the restless minutes (it's those
/// [`last_sleep`]'s counts are of).
fn night_kinds(state: &State) -> BTreeMap<i64, crate::sleep::Kind> {
    use crate::sleep::Kind;
    let Some(sleep) = state.last_sleep else {
        return BTreeMap::new();
    };
    let (deep, rem) = (i64::from(sleep.deep_minutes), i64::from(sleep.rem_minutes));
    let asleep = i64::from(sleep.asleep_minutes);
    (sleep.start_utc_minute..sleep.end_utc_minute)
        .map(|minute| {
            let into = minute - sleep.start_utc_minute;
            let kind = if into < deep {
                Kind::Deep
            } else if into < deep + rem {
                Kind::Rem
            } else if into < asleep {
                Kind::Light
            } else {
                Kind::Restless
            };
            (minute, kind)
        })
        .collect()
}

/// As `system_off::save_before_reset`, as the watch switches off: the
/// planned record, as of the minute after now.
pub fn save_planned() {
    let newest = clock::now_utc_seconds().map(|second| second.div_euclid(60));
    state().planned = Some(crate::snapshot::empty_from(newest));
}

/// The watch crashed and reset: what's in RAM is lost (the history, the
/// last night, whether the wearer's asleep), bar today's steps (in RAM
/// that resets keep, as the clock is: see `clock`); then, as the activity
/// task does at boot (`system_off::restore`), the log is read back (here:
/// the history from before the newest checkpoint stays, and where the
/// phone's copy of it ends), and the newest checkpoint is put back. Call
/// with the clock resumed. Says what it restored.
pub fn crash() -> String {
    use crate::snapshot::{self, HEADER_BYTES, Header, SAMPLE_BYTES};
    let mut state = state();
    state.last_sleep = None;
    state.asleep = false;
    state.light = false;
    state.counted = 0;
    state.polled = None;

    let now = clock::now_utc_seconds().map(|second| second.div_euclid(60));
    let header = |bytes: &[u8]| {
        Header::from_bytes(bytes[..HEADER_BYTES].try_into().expect("a header's worth"))
    };
    let checks_out = |bytes: &[u8], header: &Header| {
        let samples = &bytes[HEADER_BYTES..];
        samples.len() == usize::from(header.minutes) * SAMPLE_BYTES
            && header.expected_checksum(snapshot::checksum(snapshot::CHECKSUM_SEED, samples))
                == header.checksum
    };
    let mut best: Option<(&Vec<u8>, Header)> = None;
    for bytes in state.checkpoints.values() {
        if let Some(header) = header(bytes)
            && snapshot::newer_checkpoint(
                header.as_of(),
                best.map(|(_, best)| best.as_of()),
                state.planned,
                now,
            )
            && checks_out(bytes, &header)
        {
            best = Some((bytes, header));
        }
    }
    let Some((bytes, header)) = best.map(|(bytes, header)| (bytes.clone(), header)) else {
        // Not two minutes into the first run: no log yet either.
        state.history.clear();
        return "nothing to restore".into();
    };

    // What the log has: all that's before the checkpoint.
    let first = i64::from(header.first_minute);
    state.history.retain(|&minute, _| minute < first);
    let mut kinds = Vec::new();
    const _: () = assert!(SAMPLE_BYTES == 6);
    for (i, &[s0, s1, _, _, heart_rate, flags]) in
        bytes[HEADER_BYTES..].as_chunks::<6>().0.iter().enumerate()
    {
        let steps = u16::from_le_bytes([s0, s1]);
        if steps > 0 || heart_rate > 0 {
            state.history.insert(first + i as i64, (steps, heart_rate));
        }
        kinds.push(crate::sleep::Kind::from_bits(flags & 0x0F));
    }
    // As the watch works it out again from the history: its last sleep.
    state.last_sleep =
        crate::sleep::last_sleep(kinds.len(), |i| kinds[i]).map(|(start, end, _)| {
            let stages = crate::stages::Minutes::count(kinds[start..end].iter().copied());
            Sleep {
                start_utc_minute: first + start as i64,
                end_utc_minute: first + end as i64,
                asleep_minutes: stages.asleep(),
                deep_minutes: stages.deep,
                rem_minutes: stages.rem,
            }
        });
    if header.synced {
        crate::script::set_synced_through(Some(first - 1));
    }
    let night = state.last_sleep.map_or("no sleep".into(), |sleep| {
        format!(
            "sleep from UTC minute {} to {}",
            sleep.start_utc_minute, sleep.end_utc_minute
        )
    });
    format!(
        "restored {} minutes from UTC minute {} from a checkpoint, with {night}",
        header.minutes, header.first_minute
    )
}
