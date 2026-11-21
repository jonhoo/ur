//! Which minutes the activity history keeps as the clock moves: the
//! arithmetic of [`crate::activity`]'s ring of per-minute samples, apart
//! from the samples, so it runs in host tests (`host-tests/minute_ring`).
//!
//! The ring has a slot per minute of its last [`MINUTES`], each minute
//! (UTC minutes since 1970) in slot `minute % len`, and remembers its
//! newest minute. A slot holds a minute between the newest minus `len` and the
//! newest; filing a sample for another minute first empties the slots
//! that no longer hold one of those.
//!
//! The clock normally only moves on, a minute at a time, or by a gap the
//! watch didn't see. But the phone can set it back: a few seconds (the
//! watch's crystal ran fast), or hours or days, a phone whose clock was
//! wrong being put right. Then the minutes after the new time are in the
//! future: their samples were filed under a clock that was ahead, and
//! kept, they'd go on being fetched (and shown, and slept through) as
//! times yet to come, while the minutes really recorded from then on
//! landed on top of them. Further back than the ring reaches, they landed
//! in other minutes' slots, filed under the wrong day. So going back
//! drops what's after the new time, and further back than the ring
//! reaches, all of it.
//!
//! Except the smallest step back, into the minute before the newest
//! ([`carried`]): that's no wrong clock put right, but the phone trimming
//! a clock a few seconds ahead of its own, which it does at about one
//! connection in six (the watch's crystal, and more often a time that
//! took the phone some seconds to deliver the connection before). One
//! time in ten or so that crosses back over the turn of a minute, with
//! the first seconds of the new minute already filed: steps taken and
//! perhaps a heart rate, really measured, seconds ago. Those aren't
//! dropped with their minute but go into the one the clock is back in.
//!
//! Moving on, the ring lets go of its oldest minutes ([`let_go`]). They
//! aren't lost: by then they're in the activity log in the SPI flash (see
//! `crate::spill`), for the phone, and the little the watch itself still
//! wants of them is kept in [`crate::aged`].

use core::ops::Range;

/// How many minutes the ring holds: 14½ hours.
///
/// It held a day, 8.6 KB of the 53 KB of static RAM, while it was the only
/// place for the minutes the phone hadn't fetched. Those are in the SPI
/// flash now (`crate::spill`), so what's left to hold is what the watch
/// still works on, each of which reaches back some way from the newest
/// minute:
///
/// | | minutes |
/// |---|---:|
/// | A workout being recognised (`crate::workout`) | 10 |
/// | The sleep classifier's window and context (`crate::sleep`) | 120 |
/// | The last sleep, restaged whole while its kinds may change, and what a fetch holds back of it (`crate::sleep_hold`) | the night, and up to 135 more |
/// | The cap on that hold (`sleep_hold::CAP`) | 840 |
/// | Today's chart since local midnight, and last night's summary | up to a day |
///
/// The last has no need of the minutes themselves: hourly sums and one
/// sleep's totals do, which [`crate::aged`] keeps of what leaves the ring
/// (128 bytes). So the ring is as long as the longest hold, and the hold
/// as long as the longest night: `sleep_hold::CAP` says how that was
/// chosen. The half hour on top is for the log to take a minute in
/// before the ring lets go of it: it's fetchable, and so due in the log,
/// once it's older than the cap, and the log is brought up to date every
/// couple of minutes (`snapshot::CHECKPOINT_MINUTES`).
///
/// Each hour here is 360 bytes of static RAM, in every build.
pub const MINUTES: usize = 14 * 60 + 30;

/// Filing a sample for `minute` in a ring of `len` minutes whose newest
/// is `newest` (if it has any): the minutes whose slots to empty first,
/// and the ring's newest minute after.
pub fn file(newest: Option<i64>, minute: i64, len: i64) -> (Range<i64>, i64) {
    let Some(newest) = newest else {
        return (minute..minute + 1, minute);
    };
    // The oldest minute the ring holds as of the newest.
    let oldest = newest - len + 1;
    if minute > newest {
        // On: the minutes skipped since the newest (all of them at most).
        ((newest + 1).max(minute - len + 1)..minute + 1, minute)
    } else if minute < newest {
        // Back: the minutes after this one, and this one too if it's
        // further back than the ring reaches (its slot is a newer
        // minute's, which goes with the rest).
        ((minute + 1).max(oldest)..newest + 1, minute)
    } else {
        (minute..minute, newest)
    }
}

/// The minutes a ring of `len` minutes whose newest is `newest` lets go
/// of for good as a sample is filed for `minute`: the oldest it held,
/// which it no longer reaches back to, oldest first. None when the clock
/// goes back, or stays: what [`file`] drops then are minutes after the
/// new time, which were never right.
///
/// What [`file`] empties when the clock moves on are the slots of the new
/// minutes, each of which held a minute `len` (or a multiple of it)
/// before: these.
pub fn let_go(newest: Option<i64>, minute: i64, len: i64) -> Range<i64> {
    match newest {
        Some(newest) if minute > newest => {
            let oldest = newest - len + 1;
            oldest..(minute - len + 1).min(newest + 1)
        }
        _ => minute..minute,
    }
}

/// The minute whose sample is carried into `minute`'s as that's filed,
/// rather than dropped with the rest after it: the newest, if `minute` is
/// the one right before it (see the module docs). Its steps were taken
/// in the last seconds, which the clock now says were `minute`'s.
///
/// Not from further back: a clock that was minutes or hours ahead filed
/// its samples under a time that wasn't, and the phone may have fetched
/// them so; a pile of them in one minute would be no truer.
pub fn carried(newest: Option<i64>, minute: i64) -> Option<i64> {
    newest.filter(|&newest| newest.checked_sub(minute) == Some(1))
}
