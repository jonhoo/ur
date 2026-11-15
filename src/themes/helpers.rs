//! What any theme is likely to need, whatever it looks like: mechanics
//! that choose no colour, position, size or font (each takes those from
//! the theme), so that a theme carries only what's its own. Every theme
//! may use these, as it may `crate::canvas`, `crate::font`,
//! `crate::palette` and the data a screen shows; never another theme's
//! module (see AUTHORING-THEMES.md).
//!
//! - Formatting: a time, a countdown, a duration, an uptime, a date,
//!   tenths, a percentage, volts, the time to full, hex, a Bluetooth
//!   address, in the watch's words.
//! - Text: one line cut to fit with an ellipsis, centred lines, and how
//!   far text overflows its room (for `Theme::call_overflow` and the
//!   like; the marquee's own timing is `crate::canvas`'s, shared with
//!   the UI).
//! - Taps: which of a stack of rows a tap is on, given the theme's rows.
//! - The 5×7 digit glyph for a digit.
//!
//! The dim look's rule, that it keeps to the colours the panel's idle
//! mode shows, is `crate::palette::Palette::idle_mode_safe`, a `const
//! fn` for a theme to assert on its dim palette.
//!
//! What stays the theme's: layout, colours, which font where, and what a
//! screen shows.

use core::fmt::Write as _;

use crate::canvas::{Canvas, Color};
use crate::font;
use crate::ui::Fits;

// ==============================================================================
// Formatting
// ==============================================================================

/// The time to full on the charger, as a label and its value: "FULL IN"
/// "0:40" (hours and minutes), "FULL" "SOON" once that's past, or ""
/// "FULL" once the charger says so.
pub fn to_full(to_full: crate::battery::ToFull) -> (&'static str, heapless::String<8>) {
    use crate::battery::ToFull;
    let mut text = heapless::String::new();
    let label = match to_full {
        ToFull::Minutes(minutes) => {
            write!(text, "{}:{:02}", minutes / 60, minutes % 60).fits();
            "FULL IN"
        }
        ToFull::Soon => {
            text.push_str("SOON").fits();
            "FULL"
        }
        ToFull::Full => {
            text.push_str("FULL").fits();
            ""
        }
    };
    (label, text)
}

/// Time left, as short as it reads well: "M:SS" under 10 minutes (where
/// seconds matter), "42 MIN" under an hour, "2H05" beyond.
pub fn countdown(seconds: i64) -> heapless::String<8> {
    let mut text = heapless::String::new();
    // Hours stop at 99, as the editor's two digits do.
    let seconds = seconds.clamp(0, 100 * 3600 - 1);
    let result = if seconds < 10 * 60 {
        write!(text, "{}:{:02}", seconds / 60, seconds % 60)
    } else if seconds < 60 * 60 {
        write!(text, "{} MIN", (seconds + 59) / 60)
    } else {
        write!(text, "{}H{:02}", seconds / 3600, seconds / 60 % 60)
    };
    result.fits();
    text
}

/// `minutes` (after midnight, or a duration) as HH:MM.
pub fn hhmm(minutes: i64) -> heapless::String<6> {
    // Two digits of hours, and never negative: a time of day, or a
    // night's sleep. Every caller lays the text out as five characters
    // wide; more would still fit the buffer (to "999:59"), and so pass
    // unnoticed, shifted or cut on the screen.
    debug_assert!((0..100 * 60).contains(&minutes), "not a time: {minutes}");
    let mut text = heapless::String::new();
    write!(text, "{:02}:{:02}", minutes / 60, minutes % 60).fits();
    text
}

/// A timer's duration (not counting down): "3 MIN", "1H30".
pub fn duration(minutes: u16) -> heapless::String<8> {
    let mut text = heapless::String::new();
    let result = if minutes < 60 {
        write!(text, "{minutes} MIN")
    } else {
        write!(text, "{}H{:02}", minutes / 60, minutes % 60)
    };
    result.fits();
    text
}

/// An uptime, as precise as is worth reading at its length: HH:MM:SS
/// under a day ("07:42:05"), then its two largest units, each rounded
/// down: "3D 4H", "2W 3D", "5MO 1W", "1Y 2MO". Past a day, the
/// minutes and seconds no longer say anything, and hours by the
/// thousand ("6534:00:00") mean nothing at a glance.
///
/// A month is a twelfth of a 365-day year (30 days and 10 hours), so a
/// year is always twelve of them, and the months never read "12MO";
/// leap days are left out. It stops at "99Y 11MO", as `countdown` stops
/// at 99 hours, so it's never longer than "23:59:59": eight characters,
/// which fit right of any theme's UPTIME label in the small font.
pub fn uptime(seconds: u64) -> heapless::String<8> {
    const HOUR: u32 = 60 * 60;
    const DAY: u32 = 24 * HOUR;
    const YEAR: u32 = 365 * DAY;
    const MONTH: u32 = YEAR / 12;
    const WEEK: u32 = 7 * DAY;
    // Short of 100 years, which also keeps it to 32 bits, whose division
    // the M4 does itself (a u64's is a library call).
    let seconds = seconds.min(u64::from(100 * YEAR - 1)) as u32;
    let mut text = heapless::String::new();
    // The largest unit it has reached, and the next below it for the
    // remainder.
    let (big, big_unit, small, small_unit) = if seconds >= YEAR {
        (YEAR, "Y", MONTH, "MO")
    } else if seconds >= MONTH {
        (MONTH, "MO", WEEK, "W")
    } else if seconds >= WEEK {
        (WEEK, "W", DAY, "D")
    } else if seconds >= DAY {
        (DAY, "D", HOUR, "H")
    } else {
        let (h, m, s) = (seconds / HOUR, seconds / 60 % 60, seconds % 60);
        write!(text, "{h:02}:{m:02}:{s:02}").fits();
        return text;
    };
    write!(
        text,
        "{}{big_unit} {}{small_unit}",
        seconds / big,
        seconds % big / small
    )
    .fits();
    text
}

/// A date as YYYY-MM-DD.
pub fn iso_date(year: u16, month: u8, day: u8) -> heapless::String<10> {
    let mut text = heapless::String::new();
    write!(text, "{year:04}-{month:02}-{day:02}").fits();
    text
}

/// Tenths (of a degree, say) as a number with one decimal: "-4.2", "21.0".
pub fn tenths(tenths: i32) -> heapless::String<12> {
    let mut text = heapless::String::new();
    let sign = if tenths < 0 { "-" } else { "" };
    let magnitude = tenths.unsigned_abs();
    write!(text, "{sign}{}.{}", magnitude / 10, magnitude % 10).fits();
    text
}

/// A percentage: "80%".
pub fn percent(percent: u32) -> heapless::String<12> {
    let mut text = heapless::String::new();
    write!(text, "{percent}%").fits();
    text
}

/// Millivolts as volts to the hundredth: "3.98V".
pub fn volts(millivolts: u32) -> heapless::String<12> {
    let mut text = heapless::String::new();
    write!(
        text,
        "{}.{:02}V",
        millivolts / 1000,
        (millivolts % 1000) / 10
    )
    .fits();
    text
}

/// A byte as two hex digits, capitals, as ASCII.
pub fn hex(byte: u8) -> [u8; 2] {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]]
}

/// A Bluetooth address as people write it: most significant byte first,
/// "C3:12:…", from the little-endian bytes the radio uses.
pub fn address(address: &[u8; 6]) -> heapless::String<17> {
    let mut text = heapless::String::new();
    for (i, &byte) in address.iter().rev().enumerate() {
        if i > 0 {
            text.push(':').fits();
        }
        for digit in hex(byte) {
            text.push(char::from(digit)).fits();
        }
    }
    text
}

// ==============================================================================
// Text
// ==============================================================================

/// `text` in `face` at (`x`, `y`), in `room` px, ending in an ellipsis if
/// it doesn't fit.
pub fn fitted(
    canvas: &mut Canvas,
    (x, y): (i32, i32),
    text: &str,
    face: font::Face,
    room: i32,
    color: Color,
) {
    let mut line = [font::Line::default()];
    if font::wrap(text, face, room, &mut line) > 0 {
        canvas.line(x, y, text, line[0], face, color);
    }
}

/// `lines` of the small font, each centred on `centre_x`, the first at
/// `y` and each `pitch` below the last.
pub fn centred_lines(
    canvas: &mut Canvas,
    (centre_x, y): (i32, i32),
    pitch: i32,
    lines: &[&str],
    color: Color,
) {
    for (i, line) in lines.iter().enumerate() {
        canvas.text_centered(centre_x, y + pitch * i as i32, line, color);
    }
}

/// How far `text`, in the small font at `scale`, overflows `room` px: what
/// a marquee scrolls (none if it's 0 or less).
pub fn overflow(text: &str, scale: i32, room: i32) -> i32 {
    font::small_text_width(text) * scale - room
}

// ==============================================================================
// Taps
// ==============================================================================

/// Which of `count` rows, each `height` px tall and stacked from `top`,
/// a tap at `y` is on, if any (none above `top`, nor below the last).
pub fn row_at(y: i32, top: i32, height: i32, count: usize) -> Option<usize> {
    if y < top || height <= 0 {
        return None;
    }
    let row = usize::try_from((y - top) / height).ok()?;
    (row < count).then_some(row)
}

// ==============================================================================
// Glyphs
// ==============================================================================

/// The 5×7 glyph for `digit` ([`font::DIGITS`]). A value too big for its
/// cell (a bug upstream) draws as 9 rather than indexing out of range: a
/// wrong digit on the screen beats a panic and a reset.
pub fn digit_glyph(digit: u64) -> &'static [u8; 7] {
    // Where debug assertions are on (the simulator and its fuzzer), that
    // bug is caught here instead: a 9 that isn't one reads as a fine
    // screen. (As was the timer editor's hours, past 99: F1.)
    debug_assert!(digit <= 9, "not a digit: {digit}");
    &font::DIGITS[digit.min(9) as usize]
}

// ==============================================================================
// Tests
// ==============================================================================
//
// Run by the simulator's `cargo test`, which builds this module for the
// host (see `sim/src/lib.rs`).

#[cfg(test)]
mod tests {
    use super::uptime;

    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    const YEAR: u64 = 365 * DAY;
    const MONTH: u64 = YEAR / 12;

    #[test]
    fn uptime_reads_in_its_two_largest_units() {
        for (seconds, shown) in [
            (0, "00:00:00"),
            (59, "00:00:59"),
            (HOUR - 1, "00:59:59"),
            (HOUR, "01:00:00"),
            (10 * HOUR + 2 * MINUTE + 13, "10:02:13"),
            (DAY - 1, "23:59:59"),
            // A day: the seconds and minutes go.
            (DAY, "1D 0H"),
            (DAY + HOUR - 1, "1D 0H"),
            (3 * DAY + 4 * HOUR + 59 * MINUTE, "3D 4H"),
            (WEEK - HOUR, "6D 23H"),
            (WEEK - 1, "6D 23H"),
            (WEEK, "1W 0D"),
            (2 * WEEK + 3 * DAY, "2W 3D"),
            // 30 days are still weeks: a month is 30 days and 10 hours.
            (30 * DAY, "4W 2D"),
            (MONTH - 1, "4W 2D"),
            (MONTH, "1MO 0W"),
            (5 * MONTH + WEEK, "5MO 1W"),
            (YEAR - DAY, "11MO 4W"),
            (YEAR - 1, "11MO 4W"),
            (YEAR, "1Y 0MO"),
            (YEAR + 2 * MONTH, "1Y 2MO"),
            // What the user saw: 6534 hours.
            (6534 * HOUR, "8MO 4W"),
            (99 * YEAR, "99Y 0MO"),
            (100 * YEAR - 1, "99Y 11MO"),
            (100 * YEAR, "99Y 11MO"),
            (u64::from(u32::MAX) + 1, "99Y 11MO"),
            (u64::MAX, "99Y 11MO"),
        ] {
            assert_eq!(uptime(seconds), shown, "{seconds} s");
        }
    }

    /// Every unit's boundary, and a sweep through a hundred years and
    /// more: each fits the row (eight characters, of glyphs the small
    /// font has), and the uptime never reads as going back.
    #[test]
    fn uptime_fits_and_only_goes_up() {
        let boundaries = [MINUTE, HOUR, DAY, WEEK, MONTH, YEAR];
        let around = boundaries
            .iter()
            .flat_map(|&unit| (1..=13).map(move |n| n * unit))
            .flat_map(|at| [at - 1, at, at + 1]);
        let sweep = (0..101 * YEAR).step_by(97 * MINUTE as usize);
        let mut seconds: Vec<u64> = around.chain(sweep).chain([u64::MAX]).collect();
        seconds.sort_unstable();
        let mut last: Option<(u8, u64, u64)> = None;
        for s in seconds {
            let shown = uptime(s);
            assert!(shown.len() <= 8, "{s} s: {shown}");
            assert!(
                shown.chars().all(|c| "0123456789:YMOWDH ".contains(c)),
                "{s} s: {shown}"
            );
            // As (unit, big, small), larger units first, which must not
            // decrease.
            let key = order(&shown);
            if let Some(last) = last {
                assert!(key >= last, "{s} s: {shown} after {last:?}");
            }
            last = Some(key);
        }
    }

    /// An uptime as text, back as something to compare: its larger unit
    /// (0 for HH:MM:SS, then 1 to 4 for days, weeks, months, years), and
    /// its two numbers (seconds alone for HH:MM:SS).
    fn order(shown: &str) -> (u8, u64, u64) {
        if let [h, m, s] = shown.split(':').collect::<Vec<_>>()[..] {
            let number = |n: &str| n.parse::<u64>().expect("digits");
            return (0, number(h) * 3600 + number(m) * 60 + number(s), 0);
        }
        let (big, small) = shown.split_once(' ').expect("two units");
        let ((big, unit), (small, _)) = (number_and_unit(big), number_and_unit(small));
        let rank = ["D", "W", "MO", "Y"]
            .iter()
            .position(|&u| u == unit)
            .expect("a known unit");
        (rank as u8 + 1, big, small)
    }

    /// "11MO" as (11, "MO").
    fn number_and_unit(part: &str) -> (u64, &str) {
        let at = part.find(|c: char| !c.is_ascii_digit()).expect("a unit");
        (part[..at].parse().expect("digits"), &part[at..])
    }
}
