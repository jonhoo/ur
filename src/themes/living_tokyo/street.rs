//! Living Tokyo's street furniture: the pieces its screens are built
//! from, so that a sign, the rain or a star looks the same on every
//! screen (see DESIGN.md beside this file for what each is for, and
//! which colour each takes).
//!
//! Each is drawn from shapes rather than stored as a bitmap, as flash is
//! scarcer than the few cycles it takes to draw them once a second. None
//! clears the canvas or picks a place on the screen: the screens do.

use crate::canvas::{Canvas, Color, WIDTH};
use crate::font;

// ==============================================================================
// Signs
// ==============================================================================

/// A sign one line of the small font tall: a tube, a clear row, the 5 rows
/// of capitals, a clear row, and the tube.
pub(super) const SIGN_H: i32 = 9;

/// A sign's box: dark (the near buildings' colour, black in the dim look)
/// inside a neon tube a pixel thick, in `tube`. What it says is the
/// caller's to draw, in the tube's colour for a sign's own lettering, or
/// in the text's for what it shows (a name, a time).
pub(super) fn sign(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, tube: Color) {
    canvas.fill_rect(x, y, w, h, Color::SHADOW);
    canvas.fill_rect(x, y, w, 1, tube);
    canvas.fill_rect(x, y + h - 1, w, 1, tube);
    canvas.fill_rect(x, y, 1, h, tube);
    canvas.fill_rect(x + w - 1, y, 1, h, tube);
}

/// A screen's title as a sign hanging in the top left corner, lit in
/// `tube`: its lettering where a title has always been (4, 3), so that
/// what's under it keeps its place; the box round it (from (2, 1), a
/// [`SIGN_H`] tall) hangs from the top edge on two short wires. Returns
/// the x just past the box.
pub(super) fn title(canvas: &mut Canvas, text: &str, tube: Color) -> i32 {
    let end = hanging(canvas, font::small_text_width(text), tube);
    canvas.text(4, 3, text, tube);
    end
}

/// The box of a [`title`]'s sign, round `width` pixels of lettering from
/// (4, 3), for lettering the caller draws (cut to fit, say). Returns the
/// x just past the box.
pub(super) fn hanging(canvas: &mut Canvas, width: i32, tube: Color) -> i32 {
    const X: i32 = 2;
    const Y: i32 = 1;
    let w = width + 4;
    sign(canvas, X, Y, w, SIGN_H, tube);
    for x in [X + 2, X + w - 3] {
        canvas.fill_rect(x, 0, 1, Y, Color::TEXT_FAINT);
    }
    X + w
}

// ==============================================================================
// Lights
// ==============================================================================

/// Whether `(x, y)` is in the disc of diameter `d` whose bounding box
/// starts at the origin, measured from pixel centres.
pub(super) fn in_disc(x: i32, y: i32, d: i32) -> bool {
    let (dx, dy) = (2 * x + 1 - d, 2 * y + 1 - d);
    dx * dx + dy * dy <= d * d
}

/// A disc `d` across, from `(left, top)`: the moon, a lamp's lens.
pub(super) fn disc(canvas: &mut Canvas, left: i32, top: i32, d: i32, color: Color) {
    for y in 0..d {
        for x in 0..d {
            if in_disc(x, y, d) {
                canvas.pixel(left + x, top + y, color);
            }
        }
    }
}

/// The glow round a light `d` across at `(left, top)`: every other pixel
/// of a ring `spread` pixels wide round it, in its colour, the way a lamp
/// hazes in the rain. Drawn before the light, and before what stands in
/// front of it.
pub(super) fn glow(canvas: &mut Canvas, left: i32, top: i32, d: i32, spread: i32, color: Color) {
    let outer = d + 2 * spread;
    for y in 0..outer {
        for x in 0..outer {
            let lamp = in_disc(x - spread, y - spread, d);
            if (x + y) % 2 == 0 && in_disc(x, y, outer) && !lamp {
                canvas.pixel(left - spread + x, top - spread + y, color);
            }
        }
    }
}

/// Stars: single pixels, every third a little brighter.
pub(super) fn stars(canvas: &mut Canvas, points: &[(i32, i32)]) {
    for (i, &(x, y)) in points.iter().enumerate() {
        let color = if i % 3 == 0 {
            Color::TEXT_DIM
        } else {
            Color::TEXT_FAINT
        };
        canvas.pixel(x, y, color);
    }
}

// ==============================================================================
// Rain and the wet street
// ==============================================================================

/// How many streaks of rain fill the screen's width.
const DROPS: u32 = 22;

/// Rain between rows `top` and `bottom`: short streaks, four pixels
/// long and leaning with the wind, in `color`: the far buildings'
/// ([`Color::TRACK`]) where it's texture behind text, the hints'
/// ([`Color::TEXT_FAINT`]) where it's weather to be seen. Drawn first,
/// so whatever's in front covers it. `tick` moves it on: a screen drawn
/// every second passes the seconds, and the rain falls; one that isn't,
/// a constant.
pub(super) fn rain(canvas: &mut Canvas, top: i32, bottom: i32, tick: u64, color: Color) {
    let height = (bottom - top).max(1) as u32;
    // Falling 11 rows a second, which isn't a multiple of the spacing, so
    // the drops don't seem to stand still.
    let fall = (tick % u64::from(height)) as u32 * 11;
    for i in 0..DROPS {
        // Odd multipliers, coprime to the width, spread the drops
        // across every column before any repeats.
        let x = ((i * 37 + 5) % WIDTH as u32) as i32;
        let y = top + ((i * 29 + 3 + fall) % height) as i32;
        for (dx, dy) in [(0, 0), (0, 1), (-1, 2), (-1, 3)] {
            if y + dy < bottom {
                canvas.pixel(x + dx, y + dy, color);
            }
        }
    }
}

/// A light's reflection in the wet street below it, `w` wide from `x`,
/// from row `y` down: broken dashes in its colour, sparser each row
/// further from it, as ripples break it up.
pub(super) fn reflection(canvas: &mut Canvas, x: i32, w: i32, y: i32, rows: i32, color: Color) {
    for row in 0..rows {
        // Two rows apart; each row's dashes shorter, and shifted, so
        // they don't line up into columns.
        let dash = (4 - row).max(1);
        let period = dash + 2 + row;
        let shift = row * 3;
        let mut dx = shift % period;
        while dx < w {
            let len = dash.min(w - dx);
            canvas.fill_rect(x + dx, y + 2 * row, len, 1, color);
            dx += period;
        }
    }
}

/// A railway line seen from the side, `w` wide from `x` at row `y`: the
/// rail, and the sleepers' ends under it, every third pixel.
pub(super) fn rails(canvas: &mut Canvas, x: i32, w: i32, y: i32) {
    canvas.fill_rect(x, y, w, 1, Color::TEXT_FAINT);
    for dx in (1..w).step_by(3) {
        canvas.pixel(x + dx, y + 1, Color::TRACK);
    }
}

// ==============================================================================
// Lanterns and steam
// ==============================================================================

/// A red paper lantern (chōchin), 5 wide and 7 tall from `(x, y)`, on a
/// short cord from the row above: dark caps top and bottom, and a body
/// that glows (with a haze round it, `glow`'s) when `lit`, or hangs dark.
/// The lit body's middle column is the paper's warm light through it.
pub(super) fn lantern(canvas: &mut Canvas, x: i32, y: i32, lit: bool) {
    if lit {
        glow(canvas, x, y + 1, 5, 2, Color::HEART);
    }
    canvas.pixel(x + 2, y - 1, Color::TEXT_FAINT);
    canvas.fill_rect(x + 1, y, 3, 1, Color::TEXT_FAINT);
    let body = if lit { Color::HEART } else { Color::TRACK };
    canvas.fill_rect(x, y + 1, 5, 5, body);
    if lit {
        canvas.fill_rect(x + 2, y + 2, 1, 3, Color::HEART_SHINE);
    }
    canvas.fill_rect(x + 1, y + 6, 3, 1, Color::TEXT_FAINT);
}

/// How a wisp of steam sways, a column a row, over a cycle.
const SWAY: [i32; 6] = [0, 1, 1, 0, -1, -1];

/// Steam rising `height` rows from `(x, y)` (its source, at the bottom):
/// two wisps, swaying and broken, fading as they rise. `tick`
/// moves it up a row a second; a constant holds it still.
pub(super) fn steam(canvas: &mut Canvas, x: i32, y: i32, height: i32, tick: u64) {
    let tick = (tick % SWAY.len() as u64) as i32;
    for (wisp, offset) in [(0, 0), (3, 3)] {
        for row in 0..height {
            // Every other row, and fewer higher up: wisps, not a column.
            let phase = (row - tick + offset).rem_euclid(SWAY.len() as i32);
            if phase % 2 == 1 || (row > height / 2 && phase % 3 == 0) {
                continue;
            }
            let dx = SWAY[(phase as usize) % SWAY.len()];
            let color = if row < height / 2 {
                Color::TEXT_DIM
            } else {
                Color::TEXT_FAINT
            };
            canvas.pixel(x + wisp + dx, y - row, color);
        }
    }
}

// ==============================================================================
// The far city
// ==============================================================================

/// Far buildings along a horizon, left to right: each one's width and
/// height. They repeat across a screen wider than they are.
const SKYLINE: [(i32, i32); 12] = [
    (6, 9),
    (4, 14),
    (7, 7),
    (5, 12),
    (8, 10),
    (3, 16),
    (6, 8),
    (7, 13),
    (4, 6),
    (6, 11),
    (5, 15),
    (7, 9),
];

/// The far city along the horizon `y` (its ground), `w` wide from `x`,
/// at most `tallest` rows high (the table's heights scaled to it): its
/// buildings in the far buildings' slate, their windows a pixel each,
/// every other column and every third row, `lit` of 256 of them lit in
/// the windows' warm yellow (see [`city_lights`]).
pub(super) fn skyline(canvas: &mut Canvas, x: i32, w: i32, y: i32, tallest: i32, lit: u32) {
    let mut left = x;
    let mut window: u32 = 0;
    let mut i = 0;
    while left < x + w {
        let (width, height) = SKYLINE[i % SKYLINE.len()];
        let width = width.min(x + w - left);
        let height = height * tallest / 16;
        canvas.fill_rect(left, y - height, width, height, Color::TRACK);
        let mut wy = y - height + 2;
        while wy < y - 1 {
            let mut wx = left + 1;
            while wx < left + width - 1 {
                // As the clock face's windows: a permutation of their
                // indices, so `lit` lights its share wherever they are.
                if (window * 167 + 13) % 256 < lit {
                    canvas.pixel(wx, wy, Color::HEART_SHINE);
                }
                window += 1;
                wx += 2;
            }
            wy += 3;
        }
        left += width + 1;
        i += 1;
    }
}

/// How many of the city's windows, out of 256, are lit at `hour`
/// (local): most in the evening, fewer as the night goes on, hardly any
/// in the small hours, a few early risers before dawn, and few by day,
/// when the lights are off.
pub(super) fn city_lights(hour: u8) -> u32 {
    match hour {
        18..=21 => 150,
        22 | 23 => 100,
        0 => 60,
        1..=3 => 24,
        4 => 32,
        5 | 6 => 64,
        _ => 16,
    }
}

// ==============================================================================
// The time of night, and the weather
// ==============================================================================

/// How light the sky is at `hour` (local), from 0, the dead of night, to
/// [`DAYLIGHT`], day: up an hour at a time from 4 to 7, and down again
/// from 17 to 20. What a screen that keeps the hour shows the dawn (or
/// the dusk) by: a glow on the horizon ([`horizon`]), and fewer stars.
pub(super) fn daylight(hour: u8) -> i32 {
    match hour {
        4 | 19 => 1,
        5 | 18 => 2,
        6 | 17 => 3,
        7..=16 => DAYLIGHT,
        _ => 0,
    }
}

/// The most [`daylight`] there is.
pub(super) const DAYLIGHT: i32 = 4;

/// The local time now, as local seconds since 1970, if the phone has set
/// the clock: for a screen that isn't handed the time.
pub(super) fn local_now() -> Option<i64> {
    let t = crate::clock::now()?;
    Some(
        t.days_since_epoch() * 86_400
            + i64::from(t.hour) * 3600
            + i64::from(t.minute) * 60
            + i64::from(t.second),
    )
}

/// The hour of local second `second`.
pub(super) fn hour_of(second: i64) -> u8 {
    (second.div_euclid(3600).rem_euclid(24)) as u8
}

/// How far up the sky the dawn's glow reaches, by level of [`daylight`].
const GLOW_ROWS: [i32; DAYLIGHT as usize + 1] = [0, 4, 9, 15, 22];

/// The glow of a sky lightening above the horizon, `w` wide from `x`, the
/// row above `y` and up, for `level` of [`daylight`], so that 02:00,
/// 05:00 and 06:30 are three skies at a glance: nothing in the dead of
/// night; then a band that grows a level at a time (4, 9, 15 and 22
/// rows), the windows' warm yellow nearest the horizon (solid from the
/// third level, dithered before), the neon's violet above it, and a
/// sparse violet haze at its top. Drawn before the buildings, or
/// whatever stands on the horizon, which stand dark against it.
pub(super) fn horizon(canvas: &mut Canvas, x: i32, w: i32, y: i32, level: i32) {
    let rows = GLOW_ROWS[level.clamp(0, DAYLIGHT) as usize];
    for row in 0..rows {
        // Which quarter of the band, from the horizon up.
        let (color, period) = match row * 4 / rows {
            0 if level >= 3 => (Color::HEART_SHINE, 1),
            0 | 1 => (Color::HEART_SHINE, 2),
            2 => (Color::REM, 2),
            _ => (Color::REM, 3),
        };
        for dx in 0..w {
            if (dx + row) % period == 0 {
                canvas.pixel(x + dx, y - 1 - row, color);
            }
        }
    }
}

/// The evening a local second belongs to, as days since 1970: a night
/// runs from noon to noon, so 02:00 on the 10th is the 9th's evening, and
/// a night's weather doesn't change at midnight.
pub(super) fn evening(second: i64) -> i64 {
    (second - 12 * 3600).div_euclid(86_400)
}

/// Whether it rains on the evening `evening` (see [`evening`]): about one
/// night in three, picked from the date (not the weather, which the watch
/// doesn't know), the same for every screen that night, and the same on
/// every watch. A hash (murmur3's finaliser) rather than the date itself,
/// so the rainy nights don't fall in a pattern.
pub(super) fn rainy(evening: i64) -> bool {
    let mut hash = evening as u32;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x7feb_352d);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x846c_a68b);
    hash ^= hash >> 16;
    hash.is_multiple_of(3)
}

// ==============================================================================
// Station boards: the alarms, the phone's alarms, the editor, and Info
// ==============================================================================

/// A station board, `w` by `h` from `(x, y)`, hanging from the top edge
/// (a platform's roof) on a rod at each of the columns `rods`: a dark
/// panel (the near buildings' colour, black in the dim look) in a frame
/// of the far buildings' slate a pixel thick (a half-tone there). Its
/// rows are the caller's, lit as an LED board's: what it announces in
/// the windows' warm yellow ([`Color::HEART_SHINE`]), what's off as a
/// dark row (in the hints' colour), and a time to read from along the
/// platform in [`led_digit`]s. Rods behind a title's sign are covered by
/// it, as if it hung in front of them.
pub(super) fn board(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, rods: [i32; 2]) {
    for rod in rods {
        canvas.fill_rect(rod, 0, 1, y, Color::TEXT_FAINT);
    }
    canvas.fill_rect(x, y, w, h, Color::TRACK);
    canvas.fill_rect(x + 1, y + 1, w - 2, h - 2, Color::SHADOW);
}

/// How far apart a big [`led_digit`]'s dots are; each is a pixel less
/// across, the gap between them the board's dark.
pub(super) const LED_PITCH: i32 = 3;
/// A big [`led_digit`]'s height: the 5×7 digits' seven rows of dots.
pub(super) const LED_H: i32 = 7 * LED_PITCH - 1;

/// One of the 5×7 digits, big, from `(x, y)`, as an LED board shows it:
/// a dot (2×2) a pixel of the glyph, lit in `color`, and, given an
/// `unlit` colour, the rest of its grid's dots dark in it, as a board's
/// LEDs show when they're off (none in the dim look, where a half-tone's
/// dots would only be noise).
pub(super) fn led_digit(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    digit: u64,
    color: Color,
    unlit: Option<Color>,
) {
    let glyph = crate::themes::helpers::digit_glyph(digit);
    for (row, &bits) in glyph.iter().enumerate() {
        for column in 0..5 {
            let on = bits & (1 << (4 - column)) != 0;
            let Some(color) = (if on { Some(color) } else { unlit }) else {
                continue;
            };
            let (dx, dy) = (column * LED_PITCH, row as i32 * LED_PITCH);
            canvas.fill_rect(x + dx, y + dy, LED_PITCH - 1, LED_PITCH - 1, color);
        }
    }
}

/// The colon between two pairs of [`led_digit`]s, from column `x`: a dot
/// level with each of their third and fifth rows of dots.
pub(super) fn led_colon(canvas: &mut Canvas, x: i32, y: i32, color: Color) {
    for row in [2, 4] {
        canvas.fill_rect(x, y + row * LED_PITCH, LED_PITCH - 1, LED_PITCH - 1, color);
    }
}

// ==============================================================================
// For the street-level scenes: music, a call, the air, finding the watch
// ==============================================================================

/// The night a screen that isn't handed the time is drawn in: the hour
/// (for the sky, the far city and the shops' hours), whether it rains,
/// and the seconds, for what moves. With no time yet, a clear evening,
/// standing still.
#[derive(Clone, Copy)]
pub(super) struct Night {
    pub hour: u8,
    pub rain: bool,
    pub tick: u64,
}

impl Night {
    pub(super) fn now() -> Self {
        let now = local_now();
        Night {
            hour: now.map_or(21, hour_of),
            rain: now.is_some_and(|second| rainy(evening(second))),
            tick: now.map_or(0, |second| second as u64),
        }
    }
}
