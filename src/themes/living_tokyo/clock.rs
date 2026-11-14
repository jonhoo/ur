//! Living Tokyo's clock face: a city at night, driven by the watch's
//! data. Ported from the InfiniTime face of the same name (see the
//! README), redrawn at the panel's resolution, bar the time.
//!
//! - The time, with its seconds, large, across the top; the seconds in
//!   the accent colour, as on Retro's face. Under it, the date, and
//!   after that a struck-out Bluetooth rune while the phone isn't
//!   connected.
//! - The moon, in the sky on the right, is the battery: it waxes from
//!   new (empty) to full (charged), in the "good" colour while charging
//!   and the "bad" one below 20%, with its outline always there.
//! - The near towers' windows light up with the day's steps: a few
//!   always, so the city isn't dead at midnight, and every one at the
//!   step goal.
//! - A beacon on the tall tower's mast blinks, every other second, while
//!   there's a heart rate to show.
//! - A billboard on the right-hand roof, MAIL in magenta neon, is up
//!   while notifications haven't been seen, its tube failing for a
//!   second now and then. That's instead of a handle on the top edge,
//!   where the time is (see `super`'s `handles`).
//! - A red sign on the left-hand roof shows the heart rate, while
//!   there's one to show; on the charger, where nothing is measured, it
//!   reads OFF, unlit, and the beacon is dark.
//! - The hour: the far city behind the towers has its windows lit as
//!   its people are up (`street::city_lights`), the dawn's glow rises
//!   behind it from 04:00, and the stars go out towards the morning.
//! - The night's weather (`street::rainy`): a clear one has its stars;
//!   a rainy one none, and the rain falling past the roofs, where the
//!   city's light catches it.
//!
//! It paints with colour roles (see `crate::palette`), in the scheme's
//! palette, the Tokyo Night ones it was drawn in (see [`super::schemes`]).
//! It's the night's only: Living Tokyo has no day schemes (708be17's day
//! scene, dark buildings against a light sky, went with its day scheme).
//!
//! It asks for no frames of its own: like Retro's face, it's drawn once
//! a second (on InfiniTime, the beacon blinked at the heart rate, which
//! would take frames ten times a second). What changes every second,
//! the time's digits and the beacon, stays on the canvas's grid, in rows
//! nothing is drawn finely in: a fine row that changes is drawn again
//! and sent whole (see `Canvas::refine`), where the canvas sends a
//! changed digit as a narrow span. So the city's fine rows only change
//! with the minute (the rain), the steps, the battery, a new heart rate
//! and the billboard's rare flicker.
//!
//! The dim look holds still: the stars, the moon, the towers outlined
//! with their lit windows, and the signs; no far city, glow or rain.

use core::fmt::Write as _;

use super::street;
use crate::battery::Battery;
use crate::canvas::{Canvas, Color, FINE};
use crate::font;
use crate::themes::{Face, STEP_GOAL};
use crate::ui::Status;

// ==============================================================================
// The scene's layout
// ==============================================================================

/// What crowns a near tower: each a different silhouette against the
/// far city, so the towers read as buildings rather than a wall.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Crown {
    /// A flat roof behind its parapet.
    Flat,
    /// Its top floors set back, a tier narrower, as an office tower's.
    Setback,
    /// A plant room, and on it the beacon's mast.
    Mast,
    /// A water tank on its legs, as on Tokyo's older blocks.
    Tank,
}

/// A near tower, in the panel's pixels: its left, its width, its main
/// roof's row (its crown stands on that), its crown, and its windows:
/// each `(w, h)`, and how far apart they are across and down.
struct Tower {
    x: i32,
    w: i32,
    top: i32,
    crown: Crown,
    window: (i32, i32),
    pitch: (i32, i32),
}

const fn tower(x: i32, w: i32, top: i32, crown: Crown, window: [i32; 4]) -> Tower {
    let [ww, wh, across, down] = window;
    Tower {
        x,
        w,
        top,
        crown,
        window: (ww, wh),
        pitch: (across, down),
    }
}

/// The near towers, left to right, shoulder to shoulder, their walls
/// in two shades by turns so that each stands apart from the next, and
/// each glazed its own way: a low block of small square windows under
/// the heart rate's sign, an office tower's bands of glass, the tall
/// one's slits under the beacon, an old block's sash windows under its
/// tank, and the billboard's, wider.
const TOWERS: [Tower; 5] = [
    tower(0, 46, 172, Crown::Flat, [4, 4, 8, 9]),
    tower(46, 46, 136, Crown::Setback, [6, 3, 8, 7]),
    tower(92, 34, 118, Crown::Mast, [3, 6, 6, 10]),
    tower(126, 54, 148, Crown::Tank, [4, 5, 9, 10]),
    tower(180, 60, 160, Crown::Flat, [5, 4, 10, 9]),
];

/// The beacon: a plus of canvas pixels round this column and row, atop
/// the tall tower's mast (its middle). Nothing else is drawn finely in
/// its three rows, which are sky (the moon's above them, the dawn's glow
/// and the far city's roofs below), so its blinking stays a few pixels'
/// change on the canvas rather than a fine band's (see the module's
/// docs).
const BEACON_X: i32 = 36;
const BEACON_ROW: i32 = 30;
/// The beacon's rows, in the panel's.
const BEACON_ROWS: core::ops::Range<i32> = 3 * (BEACON_ROW - 1)..3 * (BEACON_ROW + 2);

/// The far city's ground (hidden behind the towers) and its tallest
/// building's height: its roofs (a tank's top included) all below the
/// beacon's rows, so they leave them coarse.
const FAR_GROUND: i32 = 200;
const FAR_TALLEST: i32 = 96;
/// Where the far city starts, off the left edge: so that its tallest
/// stands beside the beacon's tower, not behind its mast, and the full
/// charge's lightning (see `glance::render_charged`) strikes it.
const FAR_SHIFT: i32 = -20;
/// The dawn's glow's horizon, just under the towers' lower roofs: its
/// highest that the face shows (the third level's 45 rows: from 07:00
/// it's day, which the night's city doesn't show either) stays under the
/// beacon's rows too.
const HORIZON: i32 = 150;

/// The rain, in the panel's rows: from under the date (it passes behind
/// the moon) to below the lowest roof, behind the towers; and how many
/// streaks fall in it.
const RAIN: (i32, i32) = (69, 170);
const RAIN_DROPS: u64 = 32;

/// The stars, on the canvas's grid: clear of the time, the date (and
/// the rune after it), the moon (whose shadow is painted over the sky,
/// so a star there would come and go with the battery), the beacon's
/// rows and the far roofs. Fewer show towards the morning, from the end.
const STARS: [(i32, i32); 12] = [
    (60, 17),
    (5, 25),
    (41, 27),
    (79, 20),
    (17, 28),
    (28, 24),
    (56, 26),
    (12, 33),
    (50, 32),
    (78, 27),
    (24, 33),
    (66, 33),
];

/// The moon's top left corner and diameter, on the canvas's grid.
const MOON_X: i32 = 63;
const MOON_Y: i32 = 17;
const MOON_D: i32 = 12;

/// The signs on the roofs, in the panel's pixels: each box's left, top,
/// width and height. The heart rate's on the first tower, as wide as its
/// widest reading ("188") with room either side; the billboard on the
/// last, under the moon. Each in a band of its own (eight canvas rows:
/// see `Canvas::refine`), so that a change sends only that band's fine
/// rows.
const PULSE: (i32, i32, i32, i32) = (4, 145, 38, 22);
const BILLBOARD: (i32, i32, i32, i32) = (186, 121, 50, 22);

/// Lit windows, out of 256: always this many, so that the city isn't
/// dead at midnight, up to all of them at the goal, in steps of
/// [`LIT_STEP`] so that the windows don't change with every step.
const MIN_LIT: u32 = 16;
const MAX_LIT: u32 = 256;
const LIT_STEP: u32 = 8;

/// A near tower's first row of windows is this far under its roof, and
/// its windows are kept this far from its sides.
const WINDOWS_FROM: i32 = 7;
const MARGIN: i32 = 5;

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

// ==============================================================================
// The face
// ==============================================================================

/// The hour, and the night, the face is drawn for.
struct Night {
    /// How many of the far city's windows are lit (see
    /// `street::city_lights`).
    lights: u32,
    /// How far the dawn's glow has risen (see `street::daylight`).
    daylight: i32,
    /// The rain's positions, which move on with the minute, if it rains.
    rain: Option<u64>,
}

/// An evening with no time set, or the full charge's: clear, its
/// windows lit as at 21:00, no glow.
const EVENING: Night = Night {
    lights: 150,
    daylight: 0,
    rain: None,
};

/// Draws the face, from a clear canvas up.
pub(super) fn render_clock(canvas: &mut Canvas, face: &Face) {
    let status = face.status;
    let dim = face.dim;
    canvas.clear(Color::BACKGROUND);
    let night = match (&status.local_time, status.local_second) {
        (Some(t), Some(second)) => Night {
            lights: street::city_lights(t.hour),
            daylight: street::daylight(t.hour),
            rain: street::rainy(street::evening(second)).then_some(second as u64 / 60),
        },
        _ => EVENING,
    };
    // The sky: the dawn's glow and the stars still out on a clear night;
    // the city in front (and the rain over it, on a rainy one); then the
    // moon, in front of the rain.
    if !dim {
        street::fine_horizon(canvas, 0, FINE, HORIZON, night.daylight.min(3));
    }
    if night.rain.is_none() {
        let shown =
            STARS.len() * (street::DAYLIGHT - night.daylight) as usize / street::DAYLIGHT as usize;
        street::fine_stars(canvas, &STARS[..shown], None);
    }
    render_city(canvas, &night, lit_level(status.steps), dim);
    moon(canvas, MOON_X, MOON_Y, MOON_D, status.battery);

    // The seconds, local (or since boot, as the usual face's blinking
    // colon has them); in the dim look, none: the beacon and the
    // billboard stay as they are between blinks.
    let tick = status
        .local_second
        .map_or(status.uptime_secs, |second| second as u64);
    let tick = (!dim).then_some(tick);
    // On the charger, nothing is measured (see `heart_schedule`'s "The
    // charger"), and a reading from before it is no one's pulse.
    let charger = status.battery.power_present;
    let heart_rate = status.heart_rate.filter(|_| !charger);
    let lit = heart_rate.is_some() && tick.is_some_and(|tick| tick % 2 == 0);
    let beacon = if lit { Color::HEART } else { Color::TEXT_FAINT };
    canvas.fill_rect(BEACON_X - 1, BEACON_ROW, 3, 1, beacon);
    canvas.fill_rect(BEACON_X, BEACON_ROW - 1, 1, 3, beacon);

    if face.unseen {
        // Lit, but out for a second about once a minute, like a failing
        // tube: rarely, as each flicker sends the billboard's band of
        // fine rows twice. Lit throughout if nothing blinks.
        let lit = tick.is_none_or(|tick| (tick * 167 + 13) % 64 != 0);
        render_sign(canvas, BILLBOARD, TOWERS[4].top, "MAIL", Color::REM, lit);
    }
    let mut text = heapless::String::<3>::new();
    match heart_rate {
        Some(rate) => {
            let _ = write!(text, "{rate}");
        }
        None if charger => {
            let _ = text.push_str("OFF");
        }
        None => {}
    }
    if !text.is_empty() {
        render_sign(canvas, PULSE, TOWERS[0].top, &text, Color::HEART, !charger);
    }
    render_time(canvas, status, dim);
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
/// has it of an evening, for the full charge's screen beneath its sky.
pub(super) fn city(canvas: &mut Canvas, steps: Option<u32>) {
    render_city(canvas, &EVENING, lit_level(steps), false);
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

// ==============================================================================
// The city
// ==============================================================================

/// The city: the far one in slate, its small windows lit by the hour;
/// on a rainy night the rain over it; and in front, the near towers,
/// `level` of every 256 of their windows lit (see [`render_tower`]). In
/// the dim look, only the towers, their outlines and their lit windows.
fn render_city(canvas: &mut Canvas, night: &Night, level: u32, dim: bool) {
    if !dim {
        // Shifted so that no far roof stands right behind the mast.
        let (x, w) = (FAR_SHIFT, FINE - FAR_SHIFT);
        street::fine_skyline(canvas, x, w, FAR_GROUND, FAR_TALLEST, night.lights);
        if let Some(minute) = night.rain {
            render_rain(canvas, minute);
        }
    }
    let mut index: u32 = 0;
    for (i, tower) in TOWERS.iter().enumerate() {
        index = render_tower(canvas, tower, i % 2 == 1, index, level, dim);
    }
}

/// The rain, as `street::fine_rain` draws it (thin streaks, nine pixels
/// long, leaning with the wind), but placed afresh each `minute` rather
/// than falling a step a second, and with a gap where it crosses the
/// beacon's rows. Rain that fell every second would change every fine
/// row it crosses every second, and so send them all whole (see
/// `Canvas::refine`); a minute's still, like a photograph's, reads as
/// rain just as well at a glance, and costs its rows once a minute, with
/// the time's minute.
fn render_rain(canvas: &mut Canvas, minute: u64) {
    let (top, bottom) = RAIN;
    let beacon = BEACON_ROWS;
    for i in 0..RAIN_DROPS {
        // Odd multipliers spread the streaks over every column and row
        // before any repeats.
        let x = ((i * 113 + minute * 41 + 17) % FINE as u64) as i32;
        let y = top + ((i * 89 + minute * 33 + 7) % (bottom - top) as u64) as i32;
        for step in 0..3 {
            let sy = y + 3 * step;
            if sy + 3 <= beacon.start || sy >= beacon.end {
                let h = 3.min(bottom - sy);
                canvas.fine_rect(x - step, sy, 1, h.max(0), Color::TEXT_FAINT);
            }
        }
    }
}

/// A near tower: its wall (in the near buildings' shade, a little
/// lighter if `light`, so neighbours stand apart; outlined in the dim
/// look, where both are black), its crown, and its windows, numbered
/// from `first` (the city's, tower after tower), `level` of every 256
/// lit, in the windows' warm yellow. Returns the number after its last
/// window.
///
/// The unlit windows aren't drawn: a lit room reads at a glance, and a
/// grid of dark glass too was what made the towers a texture.
fn render_tower(
    canvas: &mut Canvas,
    tower: &Tower,
    light: bool,
    first: u32,
    level: u32,
    dim: bool,
) -> u32 {
    let Tower {
        x,
        w,
        top,
        crown,
        window: (ww, wh),
        pitch: (across, down),
    } = *tower;
    // A block of wall from `(x, y)` down to the ground, `w` wide, with a
    // parapet's edge along its top.
    let slab = |canvas: &mut Canvas, x: i32, y: i32, w: i32| {
        let h = FINE - y;
        canvas.fine_rect(x, y, w, h, Color::SHADOW);
        if dim {
            canvas.fine_rect(x, y, 1, h, Color::TEXT_FAINT);
            canvas.fine_rect(x + w - 1, y, 1, h, Color::TEXT_FAINT);
        } else if light {
            canvas.fine_tone(x, y, w, h, 1, Color::TRACK);
        }
        canvas.fine_rect(x, y, w, 1, Color::TEXT_FAINT);
    };
    match crown {
        Crown::Flat => {}
        // The upper tier, a fifth in from each side, two floors high.
        Crown::Setback => slab(canvas, x + w / 5, top - 16, w - 2 * (w / 5)),
        // The plant room, half the tower's width; on it the mast, up to
        // the beacon, braced twice.
        Crown::Mast => {
            slab(canvas, x + w / 4, top - 8, w / 2);
            let (mast, from) = (3 * BEACON_X + 1, BEACON_ROWS.end);
            canvas.fine_rect(mast, from, 1, top - 8 - from, Color::TEXT_FAINT);
            canvas.fine_rect(mast - 1, from + 4, 3, 1, Color::TEXT_FAINT);
            canvas.fine_rect(mast - 2, from + 9, 5, 1, Color::TEXT_FAINT);
        }
        // The tank on its legs, a third of the way along.
        Crown::Tank => {
            let at = x + w / 3;
            canvas.fine_rect(at, top - 11, 10, 7, Color::SHADOW);
            canvas.fine_rect(at, top - 11, 10, 1, Color::TEXT_FAINT);
            canvas.fine_rect(at + 1, top - 4, 1, 4, Color::TEXT_FAINT);
            canvas.fine_rect(at + 8, top - 4, 1, 4, Color::TEXT_FAINT);
        }
    }
    slab(canvas, x, top, w);

    // The windows: as many columns as fit inside the margins, centred;
    // rows from under the roof to the ground. Only the rows being drawn
    // (see `Canvas::refine`) are walked.
    let columns = (w - 2 * MARGIN + across - ww) / across;
    let left = x + (w - (columns * across - (across - ww))) / 2;
    let from = top + WINDOWS_FROM;
    let rows = (FINE - from + down - wh) / down;
    let drawn = canvas.fine_rows();
    for row in 0..rows {
        let wy = from + row * down;
        if wy >= drawn.end || wy + wh <= drawn.start {
            continue;
        }
        for column in 0..columns {
            let index = first + (row * columns + column) as u32;
            if street::lit_window(index, level) {
                // One in four with its blind half down: those that'd be
                // lit at a quarter, of the windows half the city on, so
                // they're scattered as the lit ones are, but others.
                let blinds = street::lit_window(index + 128, 64);
                let blind = if blinds { wh / 2 } else { 0 };
                let wx = left + column * across;
                canvas.fine_rect(wx, wy + blind, ww, wh - blind, Color::HEART_SHINE);
            }
        }
    }
    first + (rows * columns) as u32
}

/// A sign on a roof (see `street::fine_sign`), its box `(x, y, w, h)` in
/// the panel's pixels, on two legs down to the roof at row `roof`, `text`
/// lettered in the middle in the city's signs' face (`street::LETTERING`):
/// lit, in `tube`'s neon; or out, its tube a dark glass and its letters
/// faint.
fn render_sign(
    canvas: &mut Canvas,
    (x, y, w, h): (i32, i32, i32, i32),
    roof: i32,
    text: &str,
    tube: Color,
    lit: bool,
) {
    for leg in [x + 6, x + w - 8] {
        canvas.fine_rect(leg, y + h, 2, roof - y - h, Color::TEXT_FAINT);
    }
    let (tube, ink) = if lit {
        (tube, tube)
    } else {
        (Color::TRACK, Color::TEXT_FAINT)
    };
    street::fine_sign(canvas, x, y, w, h, tube);
    let width = font::Face::Fine(street::LETTERING).width(text);
    let (tx, ty) = (
        x + (w - width) / 2,
        y + (h - street::LETTERING.height()) / 2,
    );
    canvas.fine_type(tx, ty, text, street::LETTERING, ink, None);
}

// ==============================================================================
// The time
// ==============================================================================

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
