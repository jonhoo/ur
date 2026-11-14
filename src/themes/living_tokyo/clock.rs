//! Living Tokyo's clock face: a city at night, driven by the watch's
//! data. Ported from the InfiniTime face of the same name (see the
//! README), redrawn for the canvas's 80×80.
//!
//! - The time, with its seconds, large, across the top; the seconds in
//!   the accent colour, as on Retro's face. Under it, the date, and
//!   after that a struck-out Bluetooth rune while the phone isn't
//!   connected.
//! - The moon, in the sky on the right, is the battery: it waxes from new (empty) to
//!   full (charged), in the "good" colour while charging and the "bad"
//!   one below 20%, with its outline always there.
//! - The windows light up with the day's steps: a few always, so the city
//!   isn't dead at midnight, and every one at the step goal.
//! - A beacon on the antenna blinks, every other second, while there's a
//!   heart rate to show.
//! - A billboard on the right-hand roof flickers "MAIL" while
//!   notifications haven't been seen. That's instead of a handle on the
//!   top edge, where the time is (see `super`'s `handles`).
//! - A smaller sign on the left-hand roof shows the heart rate, while
//!   there's one to show; on the charger, where nothing is measured, it
//!   reads OFF, unlit, and the beacon is dark.
//!
//! It paints with colour roles (see `crate::palette`), in the scheme's
//! palette, the Tokyo Night ones it was drawn in (see [`super::schemes`]).
//! The windows are lit in the heart's highlight, the deep sleep's and the
//! REM sleep's colours, which are their yellow, cyan and magenta. It's
//! the night's only: Living Tokyo has no day schemes (708be17's day
//! scene, dark buildings against a light sky, went with its day scheme).
//!
//! It asks for no frames of its own: like Retro's face, it's drawn once
//! a second, and the beacon and the billboard change with the seconds (on
//! InfiniTime, the beacon blinked at the heart rate, which would take
//! frames ten times a second). In the dim look, neither moves.

use core::fmt::Write as _;

use super::street::{self, Roof};
use crate::battery::Battery;
use crate::canvas::{Canvas, Color, HEIGHT};
use crate::font;
use crate::themes::{Face, STEP_GOAL};
use crate::ui::Status;

struct Building {
    x: i32,
    width: i32,
    top: i32,
}

const fn building(x: i32, width: i32, top: i32) -> Building {
    Building { x, width, top }
}

/// Hazy buildings in the distance.
const FAR: [Building; 8] = [
    building(0, 10, 43),
    building(9, 7, 37),
    building(17, 9, 45),
    building(28, 6, 35),
    building(36, 11, 41),
    building(50, 7, 33),
    building(59, 9, 43),
    building(69, 11, 39),
];

/// The buildings in front, which have windows.
const NEAR: [Building; 6] = [
    building(0, 13, 56),
    building(14, 10, 50),
    building(25, 14, 59),
    building(41, 9, 46),
    building(51, 12, 53),
    building(65, 15, 49),
];

/// Clear of the time, the date (and the rune after it), and the moon:
/// its shadow is painted in the sky's colour, so a star beside it would
/// come and go with the battery.
const STARS: [(i32, i32); 16] = [
    (3, 26),
    (10, 31),
    (18, 25),
    (24, 33),
    (32, 28),
    (39, 24),
    (42, 35),
    (49, 27),
    (53, 31),
    (57, 22),
    (60, 34),
    (77, 35),
    (58, 18),
    (1, 21),
    (6, 36),
    (35, 32),
];

/// The moon's top left corner and diameter.
const MOON_X: i32 = 63;
const MOON_Y: i32 = 19;
const MOON_D: i32 = 12;

/// The beacon (a 3×3 plus) tops an antenna on the fourth near building.
const ANTENNA_X: i32 = 45;
const BEACON_Y: i32 = 37;

/// Signs: a box a line of text high, with a pixel's margin round the
/// text, on two legs down to a roof.
const SIGN_H: i32 = 7;

/// The billboard, on the last near building's roof.
const BILLBOARD_X: i32 = 63;
const BILLBOARD_Y: i32 = 41;
const BILLBOARD_W: i32 = 16;

/// The heart rate's sign, on the first near building's roof: as wide as
/// the widest rate, "188" (11 px), its text centred.
const PULSE_X: i32 = 0;
const PULSE_Y: i32 = 48;
const PULSE_W: i32 = 13;

/// Lit windows, out of 256: always this many, so that the city isn't dead
/// at midnight, up to all of them at the goal, in steps of
/// [`LIT_STEP`] so that the windows don't change with every step.
const MIN_LIT: u32 = 8;
const MAX_LIT: u32 = 256;
const LIT_STEP: u32 = 8;

/// The time: HH:MM:SS in the 5×7 digits at 2×, each pair's digits 2 px
/// apart and the colons' dots 1 px from them, 74 px in all, which leaves
/// 3 px either side. Each digit's and colon's x.
const TIME_DIGITS: [i32; 6] = [3, 15, 29, 41, 55, 67];
const TIME_COLONS: [i32; 2] = [26, 52];
const TIME_X: i32 = TIME_DIGITS[0];
const TIME_Y: i32 = 2;
const TIME_SCALE: i32 = 2;
const DATE_Y: i32 = 18;

const WEEKDAYS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];
const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// The roles the city is drawn in.
struct Palette {
    near: Color,
    unlit: Color,
    /// A lit window's colour, picked by a hash of its index.
    lit: [Color; 8],
    /// The billboard's box, and its lettering lit.
    sign: Color,
    sign_lit: Color,
}

/// Mostly the heart's highlight (Tokyo Night's yellow), with a few windows
/// in the deep sleep's and REM's colours (its cyan and magenta).
const NIGHT: Palette = Palette {
    near: Color::SHADOW,
    unlit: Color::TRACK,
    lit: [
        Color::HEART_SHINE,
        Color::HEART_SHINE,
        Color::HEART_SHINE,
        Color::HEART_SHINE,
        Color::HEART_SHINE,
        Color::HEART_SHINE,
        Color::DEEP,
        Color::REM,
    ],
    sign: Color::SHADOW,
    sign_lit: Color::REM,
};

/// Draws the face, from a clear canvas up.
pub(super) fn render_clock(canvas: &mut Canvas, face: &Face) {
    let status = face.status;
    // The night scene, in every scheme and the dim look (white on black).
    let palette = &NIGHT;
    canvas.clear(Color::BACKGROUND);
    render_sky(canvas, status.battery);
    render_city(canvas, palette, lit_level(status.steps));
    // The seconds shown, as the usual face's blinking colon has them; in
    // the dim look, none: the beacon and the billboard stay as they are
    // between blinks.
    let second = match &status.local_time {
        Some(t) => u64::from(t.second),
        None => status.uptime_secs,
    };
    let second = (!face.dim).then_some(second);
    // On the charger, nothing is measured (see `heart_schedule`'s "The
    // charger"), and a reading from before it is no one's pulse.
    let charger = status.battery.power_present;
    let heart_rate = status.heart_rate.filter(|_| !charger);
    render_beacon(canvas, heart_rate, second);
    if face.unseen {
        render_billboard(canvas, palette, second);
    }
    let mut text = heapless::String::<3>::new();
    let color = match heart_rate {
        Some(rate) => {
            let _ = write!(text, "{rate}");
            Color::HEART
        }
        // An unlit tube, as the billboard's flickering out.
        None if charger => {
            let _ = text.push_str("OFF");
            Color::TEXT_FAINT
        }
        None => Color::HEART,
    };
    if !text.is_empty() {
        let x = PULSE_X + (PULSE_W - font::small_text_width(&text)) / 2;
        render_sign(canvas, palette, PULSE_X, PULSE_Y, PULSE_W, NEAR[0].top);
        canvas.text(x, PULSE_Y + 1, &text, color);
    }
    render_time(canvas, status, face.dim);
}

fn render_sky(canvas: &mut Canvas, battery: &Battery) {
    street::fine_stars(canvas, &STARS, None);
    moon(canvas, MOON_X, MOON_Y, MOON_D, battery);
}

/// The moon as the battery, `d` across, from `(left, top)` (the canvas's),
/// drawn at the panel's resolution (see `street::fine_moon`): waxing from
/// new (empty) to full (charged), in the "good" colour while charging and
/// the "bad" one below 20%, with its outline always there. The clock
/// face's, and the glance's on the charger, bigger.
pub(super) fn moon(canvas: &mut Canvas, left: i32, top: i32, d: i32, battery: &Battery) {
    let percent = battery.percent() as i32;
    let color = if battery.charging {
        Color::GOOD
    } else if percent < 20 {
        Color::BAD
    } else {
        Color::TEXT
    };
    street::fine_moon(canvas, 3 * left, 3 * top, 3 * d, percent, color);
}

/// The city, its windows lit by the day's `steps`, as the clock face
/// has it, for the full charge's screen beneath its sky.
pub(super) fn city(canvas: &mut Canvas, steps: Option<u32>) {
    render_city(canvas, &NIGHT, lit_level(steps));
}

/// How many windows of 256 are lit for the day's `steps`.
fn lit_level(steps: Option<u32>) -> u32 {
    let steps = steps.unwrap_or(0).min(STEP_GOAL);
    // A goal that was just reached still has to light up every window.
    if steps == STEP_GOAL {
        return MAX_LIT;
    }
    let level = MIN_LIT + steps * (MAX_LIT - MIN_LIT) / STEP_GOAL;
    level - level % LIT_STEP
}

/// What stands on the near buildings' roofs: a mast and two water
/// tanks, clear of the beacon's antenna and the signs.
const NEAR_ROOFS: [Roof; 6] = [
    Roof::Flat,
    Roof::Tank,
    Roof::Antenna,
    Roof::Flat,
    Roof::Tank,
    Roof::Flat,
];

/// The city: the far buildings dark slate, windowless in the haze; the
/// near ones at the panel's resolution (see `street::fine_building`),
/// with their windows, `level` of every 256 lit (each in a colour picked
/// by a hash of its index), the rest dark glass.
///
/// The far buildings stay on the canvas's grid, as plain blocks lose
/// nothing there, and it keeps the beacon's rows coarse: drawn finely,
/// they'd be sent whole every time it blinks, a second at a time (see
/// `Canvas::refine`), where now only its few pixels go.
fn render_city(canvas: &mut Canvas, palette: &Palette, level: u32) {
    let height = HEIGHT as i32;
    for building in &FAR {
        let Building { x, width, top } = *building;
        canvas.fill_rect(x, top, width, height - top, Color::TRACK);
    }

    // The beacon's antenna, from under the beacon to its building's roof.
    let roof = NEAR[3].top;
    canvas.fill_rect(
        ANTENNA_X,
        BEACON_Y + 3,
        1,
        roof - BEACON_Y - 3,
        Color::TEXT_FAINT,
    );

    let lit = |index: u32| {
        street::lit_window(index, level).then(|| palette.lit[(index * 37 % 8) as usize])
    };
    let mut index: u32 = 0;
    for (building, &roof) in NEAR.iter().zip(&NEAR_ROOFS) {
        let Building { x, width, top } = *building;
        let windows = street::Windows {
            first: index,
            lit: &lit,
            glass: Some(palette.unlit),
        };
        let shape = (3 * x, 3 * top, 3 * width, 3 * height);
        index = street::fine_building(canvas, shape, palette.near, roof, Some(windows));
    }
}

/// Lit on even seconds while there's a heart rate, unless nothing blinks
/// (`second` is `None`).
fn render_beacon(canvas: &mut Canvas, heart_rate: Option<u8>, second: Option<u64>) {
    let lit = heart_rate.is_some() && second.is_some_and(|second| second % 2 == 0);
    let color = if lit { Color::HEART } else { Color::TEXT_FAINT };
    canvas.fill_rect(ANTENNA_X - 1, BEACON_Y + 1, 3, 1, color);
    canvas.fill_rect(ANTENNA_X, BEACON_Y, 1, 3, color);
}

/// Mostly lit, out for a second whenever a hash of the time comes up
/// low, like a failing tube (about one second in five); lit throughout
/// if nothing blinks (`second` is `None`).
fn render_billboard(canvas: &mut Canvas, palette: &Palette, second: Option<u64>) {
    let lit = second.is_none_or(|second| (second * 167 + 13) % 256 >= 48);
    render_sign(
        canvas,
        palette,
        BILLBOARD_X,
        BILLBOARD_Y,
        BILLBOARD_W,
        NEAR[5].top,
    );
    let color = if lit {
        palette.sign_lit
    } else {
        Color::TEXT_FAINT
    };
    canvas.text(BILLBOARD_X + 1, BILLBOARD_Y + 1, "MAIL", color);
}

/// A sign's box, `width` wide, and its legs down to the `roof`.
fn render_sign(canvas: &mut Canvas, palette: &Palette, x: i32, y: i32, width: i32, roof: i32) {
    canvas.fill_rect(x, y, width, SIGN_H, palette.sign);
    let legs = y + SIGN_H;
    for x in [x + 3, x + width - 4] {
        canvas.fill_rect(x, legs, 1, roof - legs, Color::TEXT_FAINT);
    }
}

/// The time and the date, and the rune while the phone isn't connected.
fn render_time(canvas: &mut Canvas, status: &Status, dim: bool) {
    let (hours, minutes, seconds) = match &status.local_time {
        Some(t) => (u64::from(t.hour), u64::from(t.minute), t.second),
        // Time since boot, as the usual face shows it.
        None => {
            let secs = status.uptime_secs;
            ((secs / 3600) % 24, (secs / 60) % 60, (secs % 60) as u8)
        }
    };
    let seconds = u64::from(seconds);
    let digits = [
        hours / 10,
        hours % 10,
        minutes / 10,
        minutes % 10,
        seconds / 10,
        seconds % 10,
    ];
    for (i, (&digit, x)) in digits.iter().zip(TIME_DIGITS).enumerate() {
        let color = if i < 4 { Color::TEXT } else { Color::ACCENT };
        let glyph = &font::DIGITS[digit.min(9) as usize];
        canvas.sprite(x, TIME_Y, font::DIGIT_W, glyph, TIME_SCALE, color);
    }
    // The colons' dots line up with the digits' rows 2 and 4.
    for x in TIME_COLONS {
        for row in [2, 4] {
            let y = TIME_Y + row * TIME_SCALE;
            canvas.fill_rect(x, y, TIME_SCALE, TIME_SCALE, Color::TEXT);
        }
    }

    let end = match &status.local_time {
        Some(t) => {
            let weekday = (t.days_since_epoch() + 3).rem_euclid(7) as usize;
            let month = MONTHS[usize::from(t.month.clamp(1, 12)) - 1];
            let mut date = heapless::String::<10>::new();
            // "THU 01 JAN" is exactly ten.
            let _ = write!(date, "{} {:02} {month}", WEEKDAYS[weekday], t.day);
            canvas.text(TIME_X, DATE_Y, &date, Color::TEXT_DIM)
        }
        None => canvas.text(TIME_X, DATE_Y, "NO TIME", Color::TEXT_FAINT),
    };

    // As on the usual face: the rune in the faintest ink (a grey in the
    // dim look, whose faint ink is red), struck through in the low
    // battery's colour.
    if !status.connected {
        let x = end + 3;
        let faded = if dim { Color::GREY } else { Color::TEXT_FAINT };
        canvas.sprite(
            x + 1,
            DATE_Y - 1,
            font::BLUETOOTH_W,
            &font::BLUETOOTH,
            1,
            faded,
        );
        for i in 0..7 {
            canvas.pixel(x + i, DATE_Y - 1 + i, Color::BAD);
        }
    }
}
