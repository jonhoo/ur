//! Sleep, from wrist movement: which minutes were spent asleep, restless,
//! awake, or with the watch off the wrist.
//!
//! This is actigraphy, the method of research sleep watches: during sleep
//! the wrist is still, apart from brief stirs. The rules, applied to each
//! minute's movement (from [`crate::activity`]) and to whether the heart
//! rate sensor last saw skin:
//!
//! - **Falling asleep** is [`ONSET`] minutes of stillness, give or take
//!   stirs of up to [`STIR`] minutes, that are a sleeper's and a wrist's
//!   (see "Lying awake" below): in at least [`CALM_MINUTES`] of them the
//!   wrist didn't so much as fidget, and one has a heart rate reading, or
//!   a stir. The sleep is dated from the start of those minutes, not the
//!   moment they qualified.
//!   Within a night (after a wake of up to [`SAME_NIGHT_GAP`] minutes),
//!   [`RESUME`] minutes of stillness are enough, [`RESUME_CALM`] of them
//!   without a fidget (see "Lying awake after waking" below).
//! - **Waking** is [`WAKE_RUN`] minutes of movement, [`MOVED`] of which
//!   moved themselves (the smoothing spreads one twitch over three), or
//!   walking (at least [`WAKE_STEPS`] steps in a minute). It's dated from
//!   the first of them.
//! - Shorter movement within sleep is a stir: up to [`STIR`] minutes stay
//!   sleep, as do longer runs that fewer than [`MOVED`] minutes made;
//!   otherwise longer is **restless** (Gadgetbridge's "awake during
//!   sleep").
//! - **Not worn** is when the heart rate sensor's last look found no skin.
//!
//! Sleep comes out of this as [`Kind::Light`]; [`crate::stages`] then
//! relabels it light, deep, or REM from the heart rate and the stirs. (This
//! used to call the inside of long, nearly motionless stretches deep, the
//! usual stand-in from movement alone; against a Garmin it was right about
//! a fifth of deep sleep, and saw no REM.)
//!
//! Since sleep is recognised only after the fact, classification runs over
//! a trailing window ([`WINDOW`] minutes, plus [`CONTEXT`] before it) every
//! minute, revising its recent past. Minutes older than the window are
//! final, as far as sleep and waking go (their stage can still change for a
//! while: see [`crate::sleep_hold`]).
//!
//! # Lying awake
//!
//! Stillness alone took lying awake in bed for sleep: on three of four
//! evenings of a trip (2026-11-16 to 19), a wearer lying still, awake,
//! "fell asleep" up to three hours early (32 and 44 minutes of it went to
//! Health Connect as light sleep), and the quiet night that started with
//! it ignored their taps. A minute's mean movement doesn't tell the two
//! apart (lying awake, 40 to 150; asleep, 30 to 50, and into the hundreds
//! at a twitch), but its polls do. Asleep, the wrist lies dead still for
//! minutes on end, every 2-second poll at the sensor's floor (25 to 80,
//! with breathing and the pulse), and then twitches or turns in one go.
//! Awake, it fidgets: a poll of 100 to 400 in most minutes, while the
//! minute's mean stays low. So each minute keeps whether any of its polls
//! moved [`FIDGET`] or more ([`Minute::fidgeted`]), and falling asleep
//! takes [`CALM_MINUTES`] of the stillness's first [`ONSET`] without one.
//!
//! In the field logs (13 nights, 2026-11-08 to 20, nine of them with a
//! Garmin alongside), the stretches of lying awake that passed for sleep
//! had at most 7 such minutes in any 20, but for one of 12 (lying down
//! for twenty minutes before getting up again, 2026-11-19 at 23:57); of
//! the 20-minute spans within real sleep, 99% had 8 or more, and 70% the
//! 14 asked for. Asking 14 removed all four stretches of false sleep in
//! those logs (124 minutes, three in the evening and one in an
//! afternoon), and every quiet night that started early (198 minutes of
//! quiet before the night's sleep), and moved 5 of the 13 onsets, 2 to 7
//! minutes later: to after the last settling movement, which is where the
//! Garmin has them (its onsets came 5.4 minutes from ours on average
//! before, and 4.4 after; one, 2026-11-16, went from 1 minute late to 7).
//! From any minute of those nights' sleep, the 20 minutes from it pass 7
//! times in 10; the wait for ones that do is at most 18 minutes for 9 in
//! 10, and 38 for 99 in 100. So a restless start to a night delays
//! its onset, and its quiet, by that much at worst. (Sleep in a moving
//! vehicle wouldn't pass at all. TODO: revisit should that matter.)
//!
//! A watch lying somewhere is as calm as a sleeper, and then some. The
//! heart rate sensor often can't say it's off the wrist (see
//! [`crate::desk`], which finds one lying level; on 2026-11-18 one lay
//! tilted while its wearer showered, and "fell asleep" after 20 minutes).
//! So those [`ONSET`] minutes must also show a wrist: a heart rate
//! reading in one of them (a pulse means skin; the watch tries every few
//! minutes at rest, and asleep 19 in 20 tries read), or failing that a
//! stir, which a nightstand doesn't make. The stir is for a night when
//! the sensor reads nothing (it has happened: 2026-11-08): sleep is then
//! found from its first stir on, rather than not at all.
//!
//! The heart rate's *level* plays no part. It was looked at: asking that
//! the readings in the stillness be 6 to 8 BPM under the median of the
//! six hours before also kept those evenings awake, with onsets at most
//! 3 minutes late. But it needs hours of readings from before the
//! stillness, which no pass's minutes hold (and a level taken while
//! already lying in bed leaves no drop to find: such a night would never
//! start), the margins were a beat or two (lying awake at 64 to 69 BPM;
//! asleep on a night after drinking, 67 for the first ten minutes), and
//! the drop comes after the onset, not before: by a Garmin's year of
//! nights, the rate is still 3.6 BPM over the resting rate ten minutes in.
//! TODO: as a second opinion, should fidgeting alone let lying awake
//! through: against a resting rate kept over several nights.
//!
//! # Lying awake after waking
//!
//! Falling asleep again within a night is quicker, and a false wake (a
//! cluster of stirs) shouldn't cost 20 minutes of sleep, so [`RESUME`]
//! minutes of stillness are enough then. They used to be enough fidgeting
//! or not, which took lying awake for sleep just as the evenings above
//! did: on 2026-11-21 a wearer woken by the alarm at 09:01 lay still but
//! awake, a poll of [`FIDGET`] or more in every minute (2 to 25 of each
//! minute's 30), and "fell asleep" at 09:20. That added 14 minutes to the
//! night, and started a doze's quiet in which taps did nothing for 17
//! minutes.
//!
//! So the stillness's first [`RESUME`] minutes must hold [`RESUME_CALM`]
//! without a fidget. In the field logs (14 nights, 2026-11-08 to 21, nine
//! of them with a Garmin alongside) sleep came back within a night 18
//! times for real (by the Garmin where there was one, else by a heart
//! rate back at the night's level and the hours of sleep that followed;
//! one was a doze after an alarm), and those ten minutes had 4 to 10
//! minutes without a fidget: 4 once, 5 twice, 6 or more otherwise. The
//! false one had none. Asking 5 keeps all 18, one of them 4 minutes later
//! (2026-11-13 at 07:38, to after its last settling movement); 6 moves
//! three, by 3, 4 and 14 minutes, the last far enough to end that quiet
//! night for 8 minutes; 1 to 4 move none.
//!
//! 5 rather than less, as that one false sleep, fidgeting every minute,
//! is no measure of lying awake. The evenings are: judging every
//! stillness in those logs by this rule, as if a wake had come just
//! before it, the 7 on the wrist that the full rule keeps awake (and that
//! weren't the night's own start) all pass for sleep asking up to 3 calm
//! minutes, 4 asking 4, 2 asking 5, and none asking 6. So 5 doesn't
//! keep out all lying awake, only most; it's as far as the nights' real
//! sleep lets it go. Of the 10-minute spans within that sleep, 92% have 5
//! or more such minutes (84% have 6); from any minute of it, the wait for
//! ten that do is at most 9 minutes for 99 in 100. A wearer who drifts
//! off again with a restless wrist is asleep those few minutes later, or,
//! with [`SAME_NIGHT_GAP`] gone by, by the full rule.
//!
//! No pulse is asked here: the watch was on a sleeper's wrist minutes
//! ago, and a watch taken off since is the desk's to find.
//!
//! The thresholds are first guesses in our movement units (Bangle.js'
//! 8192-per-g; ~25 lying on a desk, hundreds when handled), to be tuned from
//! field logs of real nights.
//!
//! [`Night`] builds on the classifier's verdict to decide when the quiet
//! night (no buzzing or lit screen for notifications) starts and ends.
//!
//! This module knows nothing about the hardware or the clock, so it can be
//! tested on a computer (see `host-tests/sleep`).

/// What a minute was spent doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
#[repr(u8)]
pub enum Kind {
    /// No data (the watch was off, or the minute isn't classified yet).
    #[default]
    Unknown = 0,
    Awake = 1,
    Light = 2,
    Deep = 3,
    /// Awake for a while within sleep.
    Restless = 4,
    NotWorn = 5,
    /// Rapid-eye-movement (dreaming) sleep: see [`crate::stages`].
    Rem = 6,
}

impl Kind {
    pub fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Awake,
            2 => Self::Light,
            3 => Self::Deep,
            4 => Self::Restless,
            5 => Self::NotWorn,
            6 => Self::Rem,
            _ => Self::Unknown,
        }
    }

    pub fn is_sleep(self) -> bool {
        matches!(self, Self::Light | Self::Deep | Self::Rem | Self::Restless)
    }

    /// Whether this is a stage of actual sleep (not restless).
    pub fn is_stage(self) -> bool {
        matches!(self, Self::Light | Self::Deep | Self::Rem)
    }

    /// The name Gadgetbridge's `ActivityKind` uses, for activity samples.
    /// `None` for plain activity, which is Gadgetbridge's default.
    pub fn gadgetbridge_name(self) -> Option<&'static str> {
        match self {
            Self::Unknown | Self::Awake => None,
            Self::Light => Some("LIGHT_SLEEP"),
            Self::Deep => Some("DEEP_SLEEP"),
            // Gadgetbridge's Bangle.js support doesn't declare REM, but
            // takes any `ActivityKind` name: its charts draw REM (only the
            // legend leaves it out), and its Health Connect export writes
            // it as REM. See `crate::stages`.
            Self::Rem => Some("REM_SLEEP"),
            Self::Restless => Some("AWAKE_SLEEP"),
            Self::NotWorn => Some("NOT_WORN"),
        }
    }
}

/// One minute's evidence.
#[derive(Clone, Copy, Debug, Default)]
pub struct Minute {
    /// Whether the watch recorded this minute at all.
    pub seen: bool,
    pub movement: u16,
    pub steps: u16,
    /// Whether the heart rate sensor last saw skin, if it has looked.
    pub worn: Option<bool>,
    /// Whether the wrist fidgeted: one of the minute's polls moved
    /// [`FIDGET`] or more.
    pub fidgeted: bool,
    /// Whether the heart rate was read in this minute: a pulse, so skin.
    pub pulse: bool,
}

// ==============================================================================
// Thresholds
// ==============================================================================

/// Movement (averaged over three minutes) below which a minute is still.
/// TODO: tune from field logs.
pub const STILL_LIMIT: u16 = 120;
/// Minutes of stillness that make sleep. Long enough that sitting still
/// through a TV episode with the odd fidget doesn't count.
pub const ONSET: usize = 20;
/// A poll's movement (as [`Minute::movement`], over 2 seconds rather than
/// a minute) from which the wrist fidgeted. A sleeping wrist's polls stay
/// under 80 or so between its stirs; a wrist lying awake has one of 100
/// to 400 in most minutes. (Anywhere from 100 to 150 told the field logs'
/// evenings from their nights.)
pub const FIDGET: u16 = 120;
/// How many of the [`ONSET`] minutes that make sleep must be without a
/// fidget. The field logs' lying awake had at most 7, and 12 once; 13 to
/// 16 all kept it out, 16 at up to 14 minutes' delay to a real onset,
/// and 14 at up to 7 (see the module docs).
pub const CALM_MINUTES: usize = 14;
/// Minutes of stillness that make sleep again soon after waking (within
/// [`SAME_NIGHT_GAP`]): falling back asleep in the night is quicker, and a
/// false wake (a cluster of stirs) shouldn't cost 20 minutes of sleep.
pub const RESUME: usize = 10;
/// How many of the [`RESUME`] minutes that make sleep again must be
/// without a fidget. Lying awake after an alarm had none; the field
/// logs' 18 real returns to sleep had 4 to 10. 5 keeps them all (one, 4
/// minutes later), and is the most that does without delaying a return
/// by a quarter of an hour (see "Lying awake after waking" in the module
/// docs).
/// TODO: revisit with more mornings in the field logs: there's one false
/// return to sleep to go by, and 6 would also keep out the two stretches
/// of an evening's lying awake that 5 lets through.
pub const RESUME_CALM: usize = 5;
/// Minutes of movement that end sleep. Two stirs in a row already make
/// five (see [`STIR`]), so this is one more.
pub const WAKE_RUN: usize = 6;
/// How many minutes of a run of movement must have moved themselves (see
/// [`moved`]) for the run to be waking or restless, rather than a stir.
/// Three: two twitches a few minutes apart, each smoothed into three
/// moving minutes, aren't waking; a wearer getting up moves in most
/// minutes of the run.
///
/// A minute only reads as moving if it or a neighbour moved itself, so a
/// run that fewer than three minutes made is at most six long: this
/// changes nothing for longer runs, nor for walking ([`WAKE_STEPS`]).
/// TODO: tune from more nights; four keeps one more false wake out, but
/// dated one real final wake 8 minutes late (2026-11-09).
pub const MOVED: usize = 3;
/// Steps in a minute that end sleep at once: that's getting up.
pub const WAKE_STEPS: u16 = 10;
/// Moving minutes within sleep that are just a stir (light sleep). Three,
/// because the three-minute smoothing turns one twitchy minute into three
/// moving ones.
pub const STIR: usize = 3;
/// How many recent minutes are re-classified (and rewritten) every minute.
/// Must cover the longest look-back of any rule (onset: [`ONSET`]), with
/// room to spare.
pub const WINDOW: usize = 90;
/// How many minutes before the window are classified along with it, as
/// context, but not rewritten. Without it, the oldest minutes of every pass
/// (the ones about to become final) would always look like the start of a
/// stretch, with no stillness or sleep before them to go on.
pub const CONTEXT: usize = 30;

// ==============================================================================
// Classification
// ==============================================================================

/// Classifies `minutes` (oldest first) into `out`, given how long before
/// them the wearer was last asleep (see [`asleep_ago`]: final, from earlier
/// passes), and the evidence of the minute just before them (unseen if
/// there's none).
///
/// When the wearer was last asleep, rather than only whether they were in
/// the minute before: sleep comes back sooner within a night ([`RESUME`]),
/// and a pass that had forgotten the night, as its minutes started after
/// the last one asleep, asked the full [`ONSET`] of a stillness that an
/// earlier pass, which still saw the night, had taken at ten minutes. So
/// the same minutes fell asleep at one pass and were awake at a later one.
///
/// That minute's evidence is only the oldest's neighbour, for the
/// smoothing (see [`is_still`]). Without it, the oldest was smoothed over
/// itself and the minute after only, so it could be still at one pass and
/// not at the pass before, which had its neighbour: a stillness then grew
/// by a minute just by the window sliding on, enough to pass for sleep,
/// and the window's oldest minute, after it, turned to sleep once at that
/// pass and for good, long after it had gone to Gadgetbridge as awake.
pub fn classify(
    asleep_ago: Option<usize>,
    before_evidence: Minute,
    minutes: &[Minute],
    out: &mut [Kind],
) {
    assert_eq!(minutes.len(), out.len(), "one class per minute");
    let n = minutes.len();

    let still = |t: usize| is_still(before_evidence, minutes, t);
    // The length of the run of minutes from `t` for which `f` holds.
    let run = |t: usize, f: &dyn Fn(usize) -> bool| (t..n).take_while(|&u| f(u)).count();
    // The length of the stillness from `t`, bridging stirs of up to `STIR`
    // minutes, up to its last still minute.
    let stillness = |t: usize| {
        let mut end = t;
        let mut u = t;
        while u < n {
            if still(u) {
                u += 1;
                end = u;
            } else {
                let gap = run(u, &|v| {
                    minutes[v].seen && minutes[v].worn != Some(false) && !still(v)
                });
                // An unseen minute, or one off the wrist (gap 0), ends the
                // stillness too (see `still_among`).
                if gap == 0 || gap > STIR || u + gap == n {
                    break;
                }
                u += gap;
            }
        }
        end - t
    };

    let mut asleep = asleep_ago == Some(1);
    // When we were last asleep (as a minute index, -1 being just before
    // `minutes`), for the quicker return to sleep within a night.
    let mut last_asleep: Option<isize> = asleep_ago.map(|ago| -(ago as isize));
    let mut t = 0;
    while t < n {
        let minute = minutes[t];
        if !minute.seen {
            out[t] = Kind::Unknown;
            t += 1;
            continue;
        }
        if minute.worn == Some(false) {
            out[t] = Kind::NotWorn;
            asleep = false;
            t += 1;
            continue;
        }
        if !asleep {
            // Sleep starts here if the stillness from here lasts long
            // enough. (At the end of the window that's not known yet; this
            // minute stays awake for now, and a later pass revises it.)
            let length = if still(t) { stillness(t) } else { 0 };
            let same_night =
                last_asleep.is_some_and(|last| t as isize - last <= SAME_NIGHT_GAP as isize);
            // The stillness is at least as long as the minutes asked
            // about, so they're all here.
            let falls_asleep = if same_night {
                // Sooner than at the start of a night, but still only a
                // wrist that mostly doesn't fidget: lying awake after the
                // alarm is as still as dozing off again.
                length >= RESUME && calm(&minutes[t..t + RESUME]) >= RESUME_CALM
            } else {
                length >= ONSET && sleeper(&minutes[t..t + ONSET])
            };
            if falls_asleep {
                asleep = true;
                // Stirs inside are sorted out below, as within any sleep.
                out[t] = Kind::Light;
                t += 1;
            } else {
                out[t] = Kind::Awake;
                t += 1;
            }
            continue;
        }
        // Asleep.
        last_asleep = Some(t as isize);
        if minute.steps >= WAKE_STEPS {
            asleep = false;
            out[t] = Kind::Awake;
            t += 1;
            continue;
        }
        if still(t) {
            out[t] = Kind::Light;
            t += 1;
            continue;
        }
        let moving = run(t, &|u| {
            minutes[u].seen && minutes[u].worn != Some(false) && !still(u)
        });
        // This minute is seen, worn and not still (each was just checked),
        // so the run takes it in, and the loop moves on. A rule added
        // above that let another kind of minute through would otherwise
        // spin here for ever, with the history locked (a critical section).
        debug_assert!(moving >= 1, "minute {t} isn't in its own run");
        let ends_window = t + moving == n;
        // Whether enough of the run's minutes moved themselves, rather
        // than being a moving minute's neighbour: the smoothing makes
        // three moving minutes of one twitch, so two turns in bed three
        // minutes apart were a run of six, a wake, and falling back
        // asleep then took `RESUME` minutes without another. (Over eight
        // nights, 30 such wakes and restless spells made 187 minutes
        // awake against a Garmin's 55, 101 of them its REM sleep.)
        //
        // The run is all of it: sleep only ever starts at a still minute,
        // so no run is entered halfway. (But for one cut short by where
        // these minutes start, which reads as shorter too; `CONTEXT` is
        // there so that the minutes rewritten are well clear of that.)
        let moved = minutes[t..t + moving].iter().filter(|m| moved(**m)).count() >= MOVED;
        let kind = if moving >= WAKE_RUN && moved {
            asleep = false;
            Kind::Awake
        } else if (moving > STIR && moved) || ends_window {
            // At the window's end, unfinished movement is restless until
            // it's clear whether it becomes waking: also a run already
            // `WAKE_RUN` long that too few minutes made so far, which
            // one more moving minute would make a wake, from its start.
            Kind::Restless
        } else {
            Kind::Light
        };
        out[t..t + moving].fill(kind);
        t += moving;
    }
}

/// Whether a stillness's first minutes (the [`ONSET`] that would make it
/// sleep) are a sleeper's, rather than someone's lying awake, or a watch
/// lying somewhere (see the module docs): at least [`CALM_MINUTES`] of
/// them without a fidget, and one of them with a pulse, or else a stir.
///
/// Of just those minutes, whatever came before or comes after: so every
/// pass that holds them decides the same, and a stillness that's
/// [`ONSET`] minutes old has been decided (as `crate::sleep_hold` takes
/// for granted, in what it holds back). A later start within the same
/// stillness may still pass: its minutes are others.
///
/// (A return to sleep within a night asks the same of its first
/// [`RESUME`] minutes, with [`RESUME_CALM`] and no pulse, and so is
/// decided once and for all too.)
fn sleeper(minutes: &[Minute]) -> bool {
    calm(minutes) >= CALM_MINUTES && minutes.iter().any(|minute| minute.pulse || moved(*minute))
}

/// How many of `minutes` passed without a fidget.
fn calm(minutes: &[Minute]) -> usize {
    minutes.iter().filter(|minute| !minute.fidgeted).count()
}

/// How long before a pass's minutes the wearer was last asleep, for
/// [`classify`]: 1 if in the minute just before them, and so on, given the
/// kind of the minute `ago` minutes before the pass's first (`Unknown`
/// outside the record). `None` if not within [`SAME_NIGHT_GAP`], after
/// which it no longer matters.
pub fn asleep_ago(kind: impl Fn(usize) -> Kind) -> Option<usize> {
    (1..=SAME_NIGHT_GAP).find(|&ago| kind(ago).is_sleep())
}

/// Whether a minute moved itself: its own movement, unsmoothed, is at
/// least [`STILL_LIMIT`], or it counted steps. A moving minute (one that
/// isn't still: see [`still_among`]) may be only the neighbour of one
/// that did; see [`MOVED`] for what goes by the difference.
/// `crate::sleep_hold` goes by this too, to tell where [`classify`] wakes.
pub fn moved(minute: Minute) -> bool {
    minute.movement >= STILL_LIMIT || minute.steps > 0
}

/// Whether minute `t` of `minutes` is still: seen, stepless, and with its
/// movement averaged with each neighbour's below [`STILL_LIMIT`], so one
/// twitchy minute in the middle of stillness (or one still minute while
/// active) doesn't flip the verdict. `before` is the oldest's neighbour
/// (unseen if none).
fn is_still(before: Minute, minutes: &[Minute], t: usize) -> bool {
    still_among(
        Some(t.checked_sub(1).map_or(before, |i| minutes[i])),
        minutes[t],
        minutes.get(t + 1).copied(),
    )
}

/// Whether `minute` is still, given its neighbours (`None` outside the
/// minutes classified), as [`classify`] sees it: for `crate::sleep_hold`,
/// which reads the history a minute at a time rather than as a slice.
///
/// A minute off the wrist isn't: whatever lies still then, it's not the
/// wearer, so it doesn't make a stillness, and ends one. Otherwise the
/// still minutes before a watch was set down on a desk (or found off by
/// the heart rate sensor) took its stillness for their own, and fell
/// asleep, if they and it made [`ONSET`] minutes: a wearer sitting still,
/// then taking the watch off, had a few minutes' nap in Gadgetbridge.
/// (Off-wrist minutes are awake as ever: see [`classify`].)
pub fn still_among(before: Option<Minute>, minute: Minute, after: Option<Minute>) -> bool {
    let seen = [before, Some(minute), after]
        .into_iter()
        .flatten()
        .filter(|m| m.seen);
    let (sum, count) = seen.fold((0u32, 0u32), |(s, c), m| (s + u32::from(m.movement), c + 1));
    let smoothed = (sum / count.max(1)) as u16;
    minute.seen && minute.steps == 0 && minute.worn != Some(false) && smoothed < STILL_LIMIT
}

/// How many of the newest minutes [`unsettled_onset`] needs: a stillness
/// [`ONSET`] minutes long is sleep already, so one that may yet become
/// sleep is shorter, and followed by at most a stir of up to [`STIR`]
/// minutes and the minute under way; with a minute to spare, and the
/// oldest only as its newer neighbour's (for the smoothing). (A longer
/// one that isn't sleep, as its first minutes weren't a sleeper's, may
/// yet be from a later minute: one less than [`ONSET`] minutes old, so
/// among these too.)
pub const ONSET_SPAN: usize = ONSET + STIR + 2 + 1;

/// Where, among the newest of `minutes` (oldest first; the newest still
/// under way, as at a classification: [`ONSET_SPAN`] of them are enough),
/// a sleep may yet be found to have started that [`classify`] doesn't see
/// yet: the first minute of the stillness they end with, which becomes
/// sleep, dated from its start, once it has lasted [`ONSET`] minutes (or
/// [`RESUME`]), if they're calm enough; else perhaps from a later minute
/// of it, which is after this one. Bridges stirs of up to [`STIR`] minutes, and a trailing
/// one that may yet turn out to be a stir. A minute off the wrist ends
/// the stillness, as an unseen one does, as in [`classify`]. `None` if the
/// newest minutes aren't still.
///
/// The newest minute is under way, so its own and its predecessor's
/// smoothed movement aren't final: they count as still if they may yet
/// turn out so, should the wrist not move for the rest of the minute,
/// nor the next. That next minute matters: a later pass smooths the
/// newest over three minutes, not the two this one has, so a newest
/// minute after a restless one (280, say, then nothing: 140 over two,
/// but 93 over three) may be still after all, and with it the stillness
/// before it, bridging the restless minutes as a stir. Without it, a
/// wearer settling down on the sofa read as not still here, and the
/// minutes before went to Gadgetbridge as awake, to become the start of
/// the night a few minutes later. (Its steps so far can only grow.)
///
/// Sleep that has started is a sleep's to settle (see `crate::sleep_hold`).
pub fn unsettled_onset(minutes: &[Minute]) -> Option<usize> {
    let n = minutes.len();
    // The newest three, the newest without movement from here on, and a
    // still minute after it, which is only its neighbour.
    let tail = n.saturating_sub(3);
    let mut ending = [Minute::default(); 4];
    let ending = &mut ending[..n - tail + 1];
    ending[..n - tail].copy_from_slice(&minutes[tail..]);
    if let [.., newest, next] = ending {
        newest.movement = 0;
        *next = Minute {
            seen: true,
            ..Minute::default()
        };
    }
    let still = |t: usize| {
        if t + 2 >= n {
            is_still(Minute::default(), ending, t - tail)
        } else {
            is_still(Minute::default(), minutes, t)
        }
    };

    let mut from = None;
    // Moving minutes in a row, back from the last still one.
    let mut moving = 0;
    // From the second-oldest: the oldest is only its neighbour.
    for t in (1..n).rev() {
        if !minutes[t].seen || minutes[t].worn == Some(false) {
            break;
        }
        if still(t) {
            moving = 0;
            from = Some(t);
        } else {
            moving += 1;
            if moving > STIR {
                break;
            }
        }
    }
    from
}

// ==============================================================================
// Summaries
// ==============================================================================

/// Awake minutes that still count as the same night (getting up for the
/// bathroom, lying awake for a while).
pub const SAME_NIGHT_GAP: usize = 45;

/// The most recent sleep in `minutes` minutes (oldest first), whose kinds
/// `kind` gives by index: (index of its first minute, index after its
/// last, minutes actually asleep). Wakes shorter than [`SAME_NIGHT_GAP`]
/// are part of it; sleep still going on counts.
///
/// The kinds come from a function rather than a slice so that the watch
/// can read them straight out of its history, instead of copying all
/// of them (14½ hours' worth, 0.9 KB) onto the stack first.
pub fn last_sleep(minutes: usize, kind: impl Fn(usize) -> Kind) -> Option<(usize, usize, usize)> {
    let end = (0..minutes).rev().find(|&i| kind(i).is_sleep())? + 1;
    let mut start = end - 1;
    let mut gap = 0;
    for i in (0..end).rev() {
        if kind(i).is_sleep() {
            start = i;
            gap = 0;
        } else {
            gap += 1;
            if gap > SAME_NIGHT_GAP {
                break;
            }
        }
    }
    let asleep = (start..end).filter(|&i| kind(i).is_stage()).count();
    // A sleep starts and ends asleep (so trimming it can't shorten it),
    // and holds no more sleep than minutes: what `activity`'s summary and
    // the staging of `start..end` take for granted.
    debug_assert!(start < end && end <= minutes);
    debug_assert!(kind(start).is_sleep() && kind(end - 1).is_sleep());
    debug_assert!(asleep <= end - start);
    Some((start, end, asleep))
}

// ==============================================================================
// The quiet night
// ==============================================================================

/// The quiet night: from falling asleep until the morning, during which the
/// UI keeps notifications from buzzing or lighting the screen (see
/// `crate::ui`). This decides only when it starts and ends.
///
/// It starts when the wearer falls asleep: when [`Night::update`] first
/// sees `asleep` turn true. It ends at the first of:
///
/// - **An alarm** ringing ([`Night::end`]): the morning has come. (Not
///   a timer: one ringing at night is no sign of morning.)
/// - **The wearer saying so**, with a double press of the button
///   ([`Night::end`] too): up before the alarm, they needn't wait out
///   [`QUIET_AWAKE`] for the watch to act as by day.
/// - **Being awake for longer than [`QUIET_AWAKE`]**: the morning came
///   without an alarm. Being awake per the classifier isn't enough by
///   itself: getting up and walking (to the bathroom, say) ends sleep at
///   once there, and the wearer is likely back in bed within minutes.
/// - **Local noon** ([`noon_between`]), a backstop for a classifier stuck
///   on "asleep" (e.g. a watch lying still off the wrist, before the heart
///   rate sensor has said so), so that quiet can't last all day. Noon,
///   because it's past any morning, and well before any night.
///
/// Starting needs `asleep` to *turn* true, not just be true: after an alarm
/// (or noon, or the button) ends the night with the wearer still asleep
/// (as the classifier sees it: lying still, reading, after the button),
/// it stays over until they've been awake and then fallen asleep again.
/// Otherwise the alarm would end the night only for it to start again the
/// next minute.
///
/// Using the watch in the night (reading it, holding a grid button) is
/// awake only through the movement it takes, as the classifier sees it:
/// nothing here hears of touches. A minute or two of it is a stir at
/// most. [`WAKE_RUN`] minutes of fidgeting with it are waking, which
/// starts the [`QUIET_AWAKE`] count; settling then, [`RESUME`] still
/// minutes, [`RESUME_CALM`] of them without a fidget, are asleep again,
/// well within it, and the night goes on. Only moving the wrist for
/// about `WAKE_RUN + QUIET_AWAKE - RESUME` (18) minutes without settling
/// ends it, as being up would, or lying there fidgeting for as long. A
/// wrist held dead still while reading counts as asleep.
///
/// A **nap** is quiet too, but ends sooner: sleep that starts in the
/// daytime ([`nap_hours`]) ends after [`NAP_AWAKE`] minutes awake, not
/// [`QUIET_AWAKE`]. Waking from a nap is waking up, not a trip to the
/// bathroom; and noon, which ends any night, would cut a nap short anyway.
/// (Taps work in a nap, too: see `crate::desk::touch_sleeps`.)
///
/// So does a **doze** after getting up: once an alarm has rung, or the
/// wearer has ended the night with the button, before noon ([`morning`]),
/// falling asleep again before noon is quiet as ever (the night resumes,
/// muted and dim, and the alarm, done for the day, doesn't ring again),
/// but ends after [`NAP_AWAKE`] minutes awake. The alarm or the wearer
/// said the morning had come, so waking from the doze is getting up, not
/// a trip to the bathroom. (In the field, the morning after a doze stayed
/// muted for [`QUIET_AWAKE`] minutes more after getting up.) Until noon,
/// as by then any night is over, and after it, sleep is a nap or the next
/// night, which keeps its own limit. Only in the morning: an alarm that
/// rings in the afternoon or evening (the night started after noon) is the
/// end of a nap, or of a night barely begun, and the night that follows is
/// a whole one. Every morning alarm counts, whether or not it ended a
/// quiet night: one that rings with the wearer already up (the night over,
/// awake) still says it's morning. Not being awake for [`QUIET_AWAKE`],
/// though: before dawn, that's as likely a long night-time wake as getting
/// up, and the sleep after it the rest of the night.
///
/// Nothing here survives a reset. That fails safe: after one, the night is
/// over, and starts again once the classifier (whose history is lost too)
/// sees [`ONSET`] minutes of sleep. So a reset at night costs ~20
/// minutes of the screen lighting up for notifications, and a reset can't
/// leave the watch quiet through the day.
///
/// Times are in monotonic seconds (since boot), as `u32` to keep this to 8
/// bytes in RAM; that lasts 136 years of uptime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Night {
    quiet: bool,
    /// Whether the wearer was asleep at the last update, to see them fall
    /// asleep.
    asleep: bool,
    /// When the wearer was last asleep. Meaningful only while quiet: it's
    /// set on falling asleep, which starts the night.
    last_asleep: u32,
    /// Which quiet it is: a nap (see [`nap_hours`]) or a doze after
    /// getting up end sooner, and only a nap lets taps through (see
    /// `crate::ui`).
    rest: Rest,
    /// Whether the wearer got up this morning (an alarm or the button, see
    /// [`Night::end`]), until noon: falling asleep is then a doze. (In
    /// what was `Night`'s padding: it stays 8 bytes.)
    up_this_morning: bool,
}

/// Minutes awake that end a quiet night without an alarm. Long enough for a
/// trip to the bathroom and back to sleep, short enough that a morning
/// without an alarm isn't silent for long. (The user's pick; shorter than
/// the classifier's [`SAME_NIGHT_GAP`], which only decides what counts as
/// one night in the summary.)
pub const QUIET_AWAKE: usize = 22;

/// Minutes awake that end a nap's quiet: once up from a nap, the wearer is
/// most likely up for the day, so a few minutes of being awake (enough to
/// tell a stir that the classifier calls waking from actually getting up)
/// are enough.
pub const NAP_AWAKE: usize = 7;

/// Whether falling asleep at `local_second` (local seconds since 1970)
/// makes a nap rather than a night: between 09:00 and 20:00. Clock hours,
/// not daylight: an Oslo summer has the sun up until 23:00, and its winter
/// sets at 15:30. Unknown time is a night (the longer quiet).
pub fn nap_hours(local_second: Option<i64>) -> bool {
    const HOUR: i64 = 60 * 60;
    local_second.is_some_and(|s| (9 * HOUR..20 * HOUR).contains(&s.rem_euclid(24 * HOUR)))
}

/// Whether `local_second` (local seconds since 1970) is before noon: the
/// morning, when getting up makes sleep until noon a doze (see
/// [`Night::end`]). Unknown time isn't: without it, noon (see
/// [`noon_between`]) never comes to end the morning.
pub fn morning(local_second: Option<i64>) -> bool {
    const DAY: i64 = 24 * 60 * 60;
    const NOON: i64 = 12 * 60 * 60;
    local_second.is_some_and(|s| s.rem_euclid(DAY) < NOON)
}

/// Which quiet starts as the wearer falls asleep.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Rest {
    /// A night, which ends after [`QUIET_AWAKE`] minutes awake.
    #[default]
    Night,
    /// A nap ([`nap_hours`]), which ends after [`NAP_AWAKE`].
    Nap,
    /// A doze after getting up in the morning (see [`Night`]), which ends
    /// after [`NAP_AWAKE`] too. A doze in nap hours is a doze: it's having
    /// got up that says why it ends sooner.
    Doze,
}

/// How a quiet night changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// The wearer fell asleep: the night (or a nap, or a doze) starts.
    Quiet(Rest),
    /// The night ended this way.
    Over(Over),
}

/// Why a night ended (other than an alarm or the button, which
/// [`Night::end`] reports).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Over {
    /// Awake for longer than [`QUIET_AWAKE`] (or [`NAP_AWAKE`]).
    Awake,
    /// Local noon came.
    Noon,
}

impl Night {
    /// Whether it's the quiet night now.
    pub fn quiet(&self) -> bool {
        self.quiet
    }

    /// Which quiet it is now, if it's quiet.
    pub fn rest(&self) -> Option<Rest> {
        self.quiet.then_some(self.rest)
    }

    /// Call regularly (every second or minute; it only compares times) with
    /// whether the wearer is asleep (`crate::activity::asleep`), the time in
    /// monotonic seconds, whether local noon has passed since the last
    /// call ([`noon_between`]), and whether falling asleep now would make a
    /// nap ([`nap_hours`]). Returns what changed, if anything.
    pub fn update(
        &mut self,
        asleep: bool,
        now: u32,
        noon: bool,
        nap_hours: bool,
    ) -> Option<Change> {
        // Monotonic time: a clock that went back (the wall clock, passed
        // by mistake) would read as hours awake below, and end the night.
        debug_assert!(
            !self.quiet || self.last_asleep <= now,
            "now {now}, but asleep at {}",
            self.last_asleep
        );
        let fell_asleep = asleep && !self.asleep;
        self.asleep = asleep;
        if asleep {
            self.last_asleep = now;
        }
        // Noon ends the morning that getting up began, whether or not the
        // night is quiet: after it, sleep is a nap or the next night.
        if noon {
            self.up_this_morning = false;
        }
        if !self.quiet {
            let rest = if self.up_this_morning {
                Rest::Doze
            } else if nap_hours {
                Rest::Nap
            } else {
                Rest::Night
            };
            self.quiet = fell_asleep;
            self.rest = rest;
            return fell_asleep.then_some(Change::Quiet(rest));
        }
        let awake_limit = if self.rest == Rest::Night {
            QUIET_AWAKE
        } else {
            NAP_AWAKE
        };
        let over = if noon {
            Over::Noon
        } else if now.wrapping_sub(self.last_asleep) > awake_limit as u32 * 60 {
            Over::Awake
        } else {
            return None;
        };
        self.quiet = false;
        Some(Change::Over(over))
    }

    /// Call when the wearer is up: an alarm rings (not a timer), or they
    /// end the night by hand (a double press of the button). The night is
    /// over, and if it's the [`morning`], falling asleep again before noon
    /// is a doze, which ends sooner (see [`Night`]). Returns whether it
    /// was quiet until now.
    pub fn end(&mut self, morning: bool) -> bool {
        self.up_this_morning |= morning;
        core::mem::replace(&mut self.quiet, false)
    }
}

/// Whether local noon falls in `(before, now]` (local seconds since 1970,
/// as in `crate::alarm`).
pub fn noon_between(before: i64, now: i64) -> bool {
    const DAY: i64 = 24 * 60 * 60;
    const NOON: i64 = 12 * 60 * 60;
    // The last noon at or before `now`.
    let noon = (now - NOON).div_euclid(DAY) * DAY + NOON;
    before < noon
}
