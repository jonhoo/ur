//! Living Tokyo's street furniture: the pieces its screens are built
//! from, so that a sign, the rain or a star looks the same on every
//! screen (see DESIGN.md beside this file for what each is for, and
//! which colour each takes).
//!
//! Each is drawn from shapes rather than stored as a bitmap, as flash is
//! scarcer than the few cycles it takes to draw them once a second. None
//! clears the canvas or picks a place on the screen: the screens do.

use crate::canvas::{Canvas, Color, FINE};
use crate::font;
use crate::font::fine::Style;

// ==============================================================================
// Signs
// ==============================================================================

/// A sign one line of the small font tall: a tube, a clear row, the 5 rows
/// of capitals, a clear row, and the tube.
pub(super) const SIGN_H: i32 = 9;

/// A sign's lettering, in the city's fine face (see `font::fine`): its
/// capitals the small font's height, in a tube not quite two pixels
/// thick, the sign's own neon, where a [`SIGN_H`] box holds them.
pub(super) const LETTERING: Style = Style {
    cap: 12,
    stroke: 28,
    caps: true,
};
/// Where a [`title`]'s lettering starts, in the panel's pixels: its left,
/// where the small font's was, and its capitals' top, a pixel below the
/// small font's, as the face's capitals are a stroke shorter than its 5
/// rows.
pub(super) const LETTERING_AT: (i32, i32) = (3 * 4, 3 * 3 + 1);

/// A screen's title as a sign hanging in the top left corner, lit in
/// `tube`: its lettering where a title has always been (see
/// [`LETTERING_AT`]), so that what's under it keeps its place; the box
/// round it (from (2, 1), a [`SIGN_H`] tall) hangs from the top edge on
/// two short wires. Returns the x just past the box.
pub(super) fn title(canvas: &mut Canvas, text: &str, tube: Color) -> i32 {
    lettered(canvas, text, tube, tube)
}

/// A [`title`] lettered in `ink` rather than its tube's colour: a sign
/// that's off, its lettering still legible.
pub(super) fn lettered(canvas: &mut Canvas, text: &str, tube: Color, ink: Color) -> i32 {
    let end = hanging(canvas, lettering_width(text), tube);
    let (x, y) = LETTERING_AT;
    canvas.fine_type(x, y, text, LETTERING, ink, None);
    end
}

/// The width of a sign's lettering, in the canvas's pixels (as
/// [`hanging`] takes it): its ink's, a pixel or two of the panel's to
/// spare.
pub(super) fn lettering_width(text: &str) -> i32 {
    (font::Face::Fine(LETTERING).width(text) + 2) / 3
}

/// The box of a [`title`]'s sign, round `width` pixels of lettering from
/// (4, 3), for lettering the caller draws (cut to fit, say). Returns the
/// x just past the box. The box is a [`fine_sign`] on the canvas's grid,
/// and its wires the panel's pixels.
pub(super) fn hanging(canvas: &mut Canvas, width: i32, tube: Color) -> i32 {
    const X: i32 = 2;
    const Y: i32 = 1;
    let w = width + 4;
    fine_sign(canvas, 3 * X, 3 * Y, 3 * w, 3 * SIGN_H, tube);
    for x in [X + 2, X + w - 3] {
        // Down to the tube, a pixel in from the box's edge.
        canvas.fine_rect(3 * x + 1, 0, 1, 3 * Y + 1, Color::TEXT_FAINT);
    }
    X + w
}

/// A sign's box at the panel's resolution, `w` by `h` from `(x, y)`: dark
/// inside a neon tube a pixel thick, a pixel in from its edge, its
/// corners rounded, the way a bent glass tube turns; and the tube's glow
/// round it, every other pixel, so that it reads as lit rather than
/// drawn. On the canvas's grid, three times over (a canvas pixel all
/// round for the glow, the tube and the box's dark inside it), a sign
/// keeps the room for its lettering a canvas sign had. The lettering is
/// the caller's.
///
/// Glowing outwards only: a glow inside too blurred the tube into the
/// lettering, where the dark keeps it crisp. And it stays in the dim
/// look: it's still (idle mode's limit is motion and colours, and the
/// tube's are its own), and without it the tube is a hairline.
pub(super) fn fine_sign(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, tube: Color) {
    fine_sign_in(canvas, (x, y, w, h), tube, false);
}

/// A [`fine_sign`], or in the dim look (`dim`), its glow as solid as its
/// tube, a tube two pixels thick: the glow's checkerboard flickers in
/// idle mode (see `Palette::columns`), and a tube a pixel thick alone is
/// a hairline.
pub(super) fn fine_sign_in(
    canvas: &mut Canvas,
    (x, y, w, h): (i32, i32, i32, i32),
    tube: Color,
    dim: bool,
) {
    // The box, its corners cut round the tube's.
    canvas.fine_rect(x + 2, y, w - 4, h, Color::SHADOW);
    canvas.fine_rect(x, y + 2, w, h - 4, Color::SHADOW);
    canvas.fine_rect(x + 1, y + 1, w - 2, h - 2, Color::SHADOW);
    // A ring `inset` pixels in, its corners rounded: its sides stop
    // short of them, and a pixel each turns them, diagonally.
    let ring = |canvas: &mut Canvas, inset: i32, level: u8| {
        let (left, right) = (x + inset, x + w - 1 - inset);
        let (top, bottom) = (y + inset, y + h - 1 - inset);
        let (across, down) = (w - 6, h - 6);
        canvas.fine_tone(x + 3, top, across, 1, level, tube);
        canvas.fine_tone(x + 3, bottom, across, 1, level, tube);
        canvas.fine_tone(left, y + 3, 1, down, level, tube);
        canvas.fine_tone(right, y + 3, 1, down, level, tube);
        // The corners: the glow's at (1, 1), the tube's at (2, 2).
        let c = inset + 1;
        for (cx, cy) in [(x + c, y + c), (x + w - 1 - c, y + c)] {
            canvas.fine_tone(cx, cy, 1, 1, level, tube);
            canvas.fine_tone(cx, y + h - 1 - (cy - y), 1, 1, level, tube);
        }
    };
    ring(canvas, 0, if dim { 4 } else { 2 });
    ring(canvas, 1, 4);
}

// ==============================================================================
// Lights
// ==============================================================================

/// Craters on a [`fine_moon`] `d` across: each one's middle and radius, in
/// 64ths of its diameter, from its box's top left. The seas, really: the
/// big grey patches the eye sees on a full moon.
const CRATERS: [(i32, i32, i32); 3] = [(22, 24, 9), (40, 38, 7), (36, 17, 4)];

/// The moon at the panel's resolution, `d` across from `(x, y)`, `lit`
/// percent of it lit in `color` (waxing: lit from the right, the
/// terminator a half-ellipse across it, as the real one's is), with its
/// craters greyed on the lit part, and the rest of it dark (the near
/// buildings' colour, a little lighter than the sky, as earthshine
/// leaves a new moon), ringed in a faint tone of `color` so that it's
/// there even at 0%.
///
/// Drawn a row at a time: the dark run, then the lit one over it, then
/// the craters' runs, so it costs a few rectangles a row.
pub(super) fn fine_moon(canvas: &mut Canvas, x: i32, y: i32, d: i32, lit: i32, color: Color) {
    moon(canvas, (x, y, d), lit, color, false);
}

/// A [`fine_moon`], or in the dim look (`dim`), as the dim look draws
/// the city: its dark side outlined in the hints' red, a pixel thick,
/// the lit share solid, and no seas, as tones there would be
/// checkerboards, which flicker in idle mode (see `Palette::columns`).
pub(super) fn moon(
    canvas: &mut Canvas,
    (x, y, d): (i32, i32, i32),
    lit: i32,
    color: Color,
    dim: bool,
) {
    let lit = lit.clamp(0, 100);
    // For a row half `half` wide (from the middle), the terminator is at
    // `k * half` across the middle, `k` from 1 (new) to -1 (full): its
    // lit share is then (1 - k) / 2 of the disc, a row and so the whole.
    // Doubled coordinates, from pixel centres, keep it in integers.
    let c = 2 * lit - 100;
    for py in drawn(canvas, y, d) {
        let span = ellipse_span(x, y, d, d, py);
        if span.is_empty() {
            continue;
        }
        let dy = 2 * (py - y) + 1 - d;
        let half2 = d * d - dy * dy;
        // Lit from the first column whose middle is past the terminator:
        // 100·dx > -c·√half2, compared squared (dx doubled, as dy).
        let past = |px: i32| {
            let a = 100 * (2 * (px - x) + 1 - d);
            let b2 = c * c * half2;
            match (a >= 0, c >= 0) {
                (true, true) => true,
                (false, true) => a * a < b2,
                (true, false) => a * a > b2,
                (false, false) => false,
            }
        };
        let from = span.clone().find(|&px| past(px)).unwrap_or(span.end);
        let width = span.len() as i32;
        canvas.fine_rect(span.start, py, width, 1, Color::SHADOW);
        if dim {
            // The outline: in to where the next row out starts, so that
            // it's unbroken where the rows narrow fast; the top and
            // bottom rows whole.
            let outward = if 2 * (py - y) < d { -1 } else { 1 };
            let outer = ellipse_span(x, y, d, d, py + outward);
            let ring = if outer.is_empty() {
                width
            } else {
                (outer.start - span.start).max(1)
            };
            canvas.fine_rect(span.start, py, ring, 1, Color::TEXT_FAINT);
            canvas.fine_rect(span.end - ring, py, ring, 1, Color::TEXT_FAINT);
            canvas.fine_rect(from, py, span.end - from, 1, color);
            continue;
        }
        canvas.fine_tone(span.start, py, 1, 1, 2, color);
        canvas.fine_tone(span.end - 1, py, 1, 1, 2, color);
        canvas.fine_rect(from, py, span.end - from, 1, color);
        for (cx, cy, r) in CRATERS {
            let (cx, cy, r) = (x + cx * d / 64, y + cy * d / 64, (r * d / 64).max(1));
            let crater = ellipse_span(cx - r, cy - r, 2 * r, 2 * r, py);
            let (start, end) = (crater.start.max(from), crater.end);
            if start < end {
                canvas.fine_tone(start, py, end - start, 1, 2, Color::TEXT_DIM);
            }
        }
    }
    if dim {
        return;
    }
    // The ring's top and bottom, which the rows' ends leave open.
    let cap = ellipse_span(x, y, d, d, y);
    canvas.fine_tone(cap.start, y, cap.len() as i32, 1, 2, color);
    canvas.fine_tone(cap.start, y + d - 1, cap.len() as i32, 1, 2, color);
}

/// Stars at the panel's resolution: each a single pixel at the middle of
/// its canvas pixel in `points` (the screens' tables are on the canvas's
/// grid), every third brighter; and given a `tick`, one twinkling every
/// third second, a different one each time (as the screens had them), as
/// a small cross of the text's white, three pixels each way.
///
/// A star a canvas pixel across was a square, and a twinkle a brighter
/// square; a point of light and a glint read as stars.
pub(super) fn fine_stars(canvas: &mut Canvas, points: &[(i32, i32)], tick: Option<u64>) {
    for (i, &(x, y)) in points.iter().enumerate() {
        let color = if i % 3 == 0 {
            Color::TEXT
        } else {
            Color::TEXT_DIM
        };
        canvas.fine_pixel(3 * x + 1, 3 * y + 1, color);
    }
    let Some(tick) = tick.filter(|tick| tick.is_multiple_of(3)) else {
        return;
    };
    if let Some(&(x, y)) = points.get((tick / 3) as usize % points.len().max(1)) {
        let (x, y) = (3 * x + 1, 3 * y + 1);
        canvas.fine_rect(x - 1, y, 3, 1, Color::TEXT);
        canvas.fine_rect(x, y - 1, 1, 3, Color::TEXT);
    }
}

// ==============================================================================
// Rain and the wet street
// ==============================================================================

/// How many streaks of [`fine_rain`] fill the screen's width.
const FINE_DROPS: u32 = 28;

/// Rain at the panel's resolution, between its rows `top` and `bottom`:
/// thin streaks, a pixel wide and nine long, leaning a pixel every three
/// rows with the wind, in `color`: the far buildings' ([`Color::TRACK`])
/// where it's texture behind text, the hints' ([`Color::TEXT_FAINT`])
/// where it's weather to be seen. Drawn first, so whatever's in front
/// covers it. `tick` moves it on, 33 rows a second, which isn't a
/// multiple of the drops' spacing, so they don't seem to stand still: a
/// screen drawn every second passes the seconds, and the rain falls; one
/// that isn't, a constant.
///
/// The canvas's streaks, a canvas pixel wide, were stripes; a hairline
/// reads as rain, and leaves the sky round it dark.
pub(super) fn fine_rain(canvas: &mut Canvas, top: i32, bottom: i32, tick: u64, color: Color) {
    let height = (bottom - top).max(1) as u32;
    let fall = (tick % u64::from(height)) as u32 * 33;
    for i in 0..FINE_DROPS {
        // Odd multipliers spread the drops over every column and row
        // before any repeats.
        let x = ((i * 113 + 17) % FINE as u32) as i32;
        let y = top + ((i * 89 + 7 + fall) % height) as i32;
        for step in 0..3 {
            let (sy, sh) = (y + 3 * step, 3.min(bottom - y - 3 * step));
            if sh > 0 {
                canvas.fine_rect(x - step, sy, 1, sh, color);
            }
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

/// What stands on a [`fine_building`]'s roof: a tank in its wall's
/// colour, so that a near one stands dark against the far ones behind
/// it; a mast in the structure's colour.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Roof {
    Flat,
    /// A water tank on its legs, as on Tokyo's older blocks.
    Tank,
    /// A mast, and its red light (unlit in the dim look).
    Antenna,
}

/// A [`fine_building`]'s windows: from which index they're numbered (the
/// city's, building after building), the colour of each one lit (by its
/// index; `None` for one dark), and, if the dark ones show, the glass's
/// colour, a tone over the whole face (a pixel in four).
///
/// The glass is a tone rather than a dark window each: a window is a
/// rectangle drawn, and the clock face's city has thousands, each drawn
/// again for its band (see `Canvas::refine`); a tone is one a building.
pub(super) struct Windows<'a> {
    pub first: u32,
    pub lit: &'a dyn Fn(u32) -> Option<Color>,
    pub glass: Option<Color>,
}

/// A building at the panel's resolution, `w` wide from `x`, from its roof
/// at `top` down to the panel's row `bottom`: its wall in `wall`, a
/// parapet's edge along its roof, `roof` on that, and its `windows`, if
/// any, of 2×2 pixels, 3 apart across and 4 down (the floors' slabs
/// between). Returns the index after its last window, for the next
/// building's.
///
/// A window a pixel high read as a slot, and one a canvas pixel across as
/// a light; at the panel's resolution, a building is its windows.
pub(super) fn fine_building(
    canvas: &mut Canvas,
    (x, top, w, bottom): (i32, i32, i32, i32),
    wall: Color,
    roof: Roof,
    windows: Option<Windows>,
) -> u32 {
    canvas.fine_rect(x, top, w, bottom - top, wall);
    canvas.fine_rect(x, top, w, 1, Color::TEXT_FAINT);
    // What's on the roof, off its middle, standing on it.
    let at = x + w / 3;
    match roof {
        Roof::Flat => {}
        Roof::Tank => {
            canvas.fine_rect(at, top - 8, 7, 5, wall);
            canvas.fine_rect(at, top - 8, 7, 1, Color::TEXT_FAINT);
            canvas.fine_rect(at + 1, top - 3, 1, 3, wall);
            canvas.fine_rect(at + 5, top - 3, 1, 3, wall);
        }
        Roof::Antenna => {
            canvas.fine_rect(at, top - 12, 1, 12, Color::TEXT_FAINT);
            canvas.fine_rect(at - 2, top - 7, 5, 1, Color::TEXT_FAINT);
            canvas.fine_pixel(at, top - 13, Color::HEART);
        }
    }
    let Some(Windows { first, lit, glass }) = windows else {
        return 0;
    };
    // The windows: as many columns as fit in a margin of 2 a side,
    // centred; rows from 3 under the roof to 2 above the ground.
    let columns = ((w - 4 + 1) / 3).max(0);
    let left = x + (w - (3 * columns - 1)) / 2;
    let rows = ((bottom - top - 5 + 2) / 4).max(0);
    if let Some(glass) = glass.filter(|_| rows > 0 && columns > 0) {
        let (gw, gh) = (3 * columns - 1, 4 * rows - 2);
        canvas.fine_tone(left, top + 3, gw, gh, 1, glass);
    }
    // Only the rows being drawn: the rest are walked past.
    let drawn = canvas.fine_rows();
    let from = ((drawn.start - top - 3) / 4).clamp(0, rows);
    let to = ((drawn.end - top - 3 + 3) / 4).clamp(0, rows);
    for row in from..to {
        let wy = top + 3 + 4 * row;
        for column in 0..columns {
            let index = first + (row * columns + column) as u32;
            if let Some(color) = lit(index) {
                canvas.fine_rect(left + 3 * column, wy, 2, 2, color);
            }
        }
    }
    first + (rows * columns) as u32
}

/// Whether window `index` is one of the `lit` of every 256 (see
/// [`fine_building`]): its index's low byte through a permutation of
/// 0..256, so every share lights exactly its part of them, wherever they
/// are. Multiplying by an odd number and adding permutes them, as does
/// folding the high bits into the low (`x ^ x >> n`); the canvas's
/// windows needed only the first, but a building's rows of the panel's
/// many windows lined its lit ones up into diagonals, which the rest
/// break up.
pub(super) fn lit_window(index: u32, lit: u32) -> bool {
    let mut x = (index as u8).wrapping_mul(167).wrapping_add(13);
    x ^= x >> 4;
    x = x.wrapping_mul(43);
    x ^= x >> 3;
    u32::from(x) < lit
}

/// The far city at the panel's resolution, along its row `y` (the
/// ground), `w` wide from `x`, at most `tallest` rows high (the table's
/// heights scaled to it): [`fine_building`]s in the far buildings'
/// slate, a few with a water tank or a mast, `lit` of 256 of their
/// windows lit in the windows' warm yellow (see [`city_lights`]), the
/// rest dark.
pub(super) fn fine_skyline(canvas: &mut Canvas, x: i32, w: i32, y: i32, tallest: i32, lit: u32) {
    let mut left = x;
    let mut window: u32 = 0;
    let mut i = 0;
    let light = |index: u32| lit_window(index, lit).then_some(Color::HEART_SHINE);
    while left < x + w {
        let (width, height) = SKYLINE[i % SKYLINE.len()];
        let width = (3 * width).min(x + w - left);
        let height = height * tallest / 16;
        // A tank on every fourth from the second, a mast on every fifth:
        // a few, at no rhythm the eye picks up.
        let roof = match i {
            _ if i % 4 == 1 => Roof::Tank,
            _ if i % 5 == 3 => Roof::Antenna,
            _ => Roof::Flat,
        };
        window = fine_building(
            canvas,
            (left, y - height, width, y),
            Color::TRACK,
            roof,
            Some(Windows {
                first: window,
                lit: &light,
                glass: None,
            }),
        );
        left += width + 2;
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
/// the dusk) by: a glow on the horizon ([`fine_horizon`]), and fewer stars.
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

/// The glow of a sky lightening above the horizon at the panel's
/// resolution, `w` wide from `x`, the rows above `y`, for `level` of
/// [`daylight`], so that 02:00, 05:00 and 06:30 are three skies at a
/// glance: nothing in the dead of night; then a band that grows a level
/// at a time (12, 27, 45 and 66 rows), in six steps of tone from the
/// horizon up: the windows' warm yellow (solid at the horizon from the
/// third level, three in four before), thinning, then the neon's violet,
/// thinning to nothing. Drawn before the buildings, or whatever stands
/// on the horizon, which stand dark against it.
///
/// The canvas's was four dithers of its pixels, which stood in bands; a
/// tone of the panel's reads as a shade, so the glow fades.
pub(super) fn fine_horizon(canvas: &mut Canvas, x: i32, w: i32, y: i32, level: i32) {
    let rows = 3 * GLOW_ROWS[level.clamp(0, DAYLIGHT) as usize];
    let bright = if level >= 3 { 4 } else { 3 };
    let steps = [
        (Color::HEART_SHINE, bright),
        (Color::HEART_SHINE, 3),
        (Color::HEART_SHINE, 2),
        (Color::REM, 2),
        (Color::REM, 1),
        (Color::REM, 1),
    ];
    for (i, (color, level)) in steps.into_iter().enumerate() {
        // Each step's share of the band, from the horizon up.
        let (low, high) = (rows * i as i32 / 6, rows * (i as i32 + 1) / 6);
        canvas.fine_tone(x, y - high, w, high - low, level, color);
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
///
/// The rods are the panel's pixels, two wide, as thin beside a title's
/// sign as its wires; the frame stays on the canvas's grid, as its sides
/// would otherwise make every row of the board a fine one, each drawn
/// again and sent whole whenever a running timer on it ticks (see
/// `Canvas::refine`).
pub(super) fn board(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, rods: [i32; 2]) {
    for rod in rods {
        canvas.fine_rect(3 * rod, 0, 2, 3 * y, Color::TEXT_FAINT);
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
/// dots would only be noise). `fine`, each dot is LEDs of 2×2 of the
/// panel's pixels (see [`led`]); otherwise a canvas pixel's square each,
/// for digits that change every second (a running timer's), whose rows
/// are cheaper sent coarse (see `Canvas::refine`).
pub(super) fn led_digit(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    digit: u64,
    color: Color,
    unlit: Option<Color>,
    fine: bool,
) {
    let glyph = crate::themes::helpers::digit_glyph(digit);
    for (row, &bits) in glyph.iter().enumerate() {
        for column in 0..5 {
            let on = bits & (1 << (4 - column)) != 0;
            let Some(color) = (if on { Some(color) } else { unlit }) else {
                continue;
            };
            let (dx, dy) = (column * LED_PITCH, row as i32 * LED_PITCH);
            led(canvas, x + dx, y + dy, color, fine);
        }
    }
}

/// The colon between two pairs of [`led_digit`]s, from column `x`: a dot
/// level with each of their third and fifth rows of dots.
pub(super) fn led_colon(canvas: &mut Canvas, x: i32, y: i32, color: Color, fine: bool) {
    for row in [2, 4] {
        led(canvas, x, y + row * LED_PITCH, color, fine);
    }
}

/// A big [`led_digit`]'s dot, from the canvas's `(x, y)`, its square
/// ([`LED_PITCH`] less a pixel a side): `fine`, as a board's LEDs at the
/// panel's resolution, 3×3 of them, each 2×2 of its pixels, 3 apart, the
/// next dot's 3 on, so that a digit's strokes are an even matrix of
/// LEDs, as a departure board's are; or the square.
fn led(canvas: &mut Canvas, x: i32, y: i32, color: Color, fine: bool) {
    let side = LED_PITCH - 1;
    if !fine {
        canvas.fill_rect(x, y, side, side, color);
        return;
    }
    for dy in [0, 3, 6] {
        for dx in [0, 3, 6] {
            canvas.fine_rect(3 * x + dx, 3 * y + dy, 2, 2, color);
        }
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

// ==============================================================================
// At the panel's resolution
// ==============================================================================
//
// Pieces drawn in the panel's own pixels, three to a canvas pixel each
// way (see `Canvas::fine_rect`): their coordinates are the panel's,
// 0..240. A row drawn finely is drawn again on its own as it's sent, so
// what's drawn a pixel at a time walks only the rows being drawn
// (`Canvas::fine_rows`).

/// Of the rows from `y`, `h` of them, those being drawn.
fn drawn(canvas: &Canvas, y: i32, h: i32) -> core::ops::Range<i32> {
    let rows = canvas.fine_rows();
    y.max(rows.start)..(y + h).min(rows.end)
}

/// The columns of row `py` inside the ellipse in the box `w` by `h` from
/// `(x, y)`, measured from pixel centres; empty off it.
fn ellipse_span(x: i32, y: i32, w: i32, h: i32, py: i32) -> core::ops::Range<i32> {
    let dy = 2 * (py - y) + 1 - h;
    let inside = |dx: i32| {
        let ex = 2 * dx + 1 - w;
        ex * ex * h * h + dy * dy * w * w <= w * w * h * h
    };
    match (0..w).find(|&dx| inside(dx)) {
        Some(first) => x + first..x + w - first,
        None => x..x,
    }
}

/// A filled ellipse in the box `w` by `h` from `(x, y)`.
pub(super) fn fine_ellipse(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, color: Color) {
    for py in drawn(canvas, y, h) {
        let span = ellipse_span(x, y, w, h, py);
        canvas.fine_rect(span.start, py, span.len() as i32, 1, color);
    }
}

/// A line a pixel thick from `from` to `to`, both drawn.
pub(super) fn fine_line(canvas: &mut Canvas, from: (i32, i32), to: (i32, i32), color: Color) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let steps = dx.abs().max(dy.abs()).max(1);
    for i in 0..=steps {
        let x = from.0 + (dx * i + dx.signum() * steps / 2) / steps;
        let y = from.1 + (dy * i + dy.signum() * steps / 2) / steps;
        canvas.fine_pixel(x, y, color);
    }
}

/// A red paper lantern (chōchin) centred on column `x`, 12 wide and 18
/// tall from `top`, on a cord from the 4 rows above: black caps, a body
/// ribbed with its bamboo hoops, lit from inside (a haze round it, the
/// flame's light down its middle) when `lit`, or hanging dark.
pub(super) fn fine_lantern(canvas: &mut Canvas, x: i32, top: i32, lit: bool) {
    const W: i32 = 12;
    const H: i32 = 14;
    let (left, body) = (x - W / 2, top + 2);
    if lit {
        fine_glow(canvas, left - 6, body - 5, W + 12, H + 10, Color::HEART);
    }
    canvas.fine_rect(x, top - 4, 1, 4, Color::TEXT_FAINT);
    fine_ellipse(
        canvas,
        left,
        body,
        W,
        H,
        if lit { Color::HEART } else { Color::TRACK },
    );
    if lit {
        canvas.fine_rect(x - 1, body + 3, 2, H - 6, Color::HEART_SHINE);
    }
    // The hoops, dark, across it.
    for rib in [4, 7, 10] {
        let span = ellipse_span(left, body, W, H, body + rib);
        let w = span.len() as i32;
        canvas.fine_rect(span.start, body + rib, w, 1, Color::SHADOW);
    }
    canvas.fine_rect(x - 3, top, 7, 2, Color::TEXT_FAINT);
    canvas.fine_rect(x - 3, body + H, 7, 2, Color::TEXT_FAINT);
}

/// A haze of `color` round a light, in the ellipse in the box `w` by `h`
/// from `(x, y)`: a pixel in four lit, thinning to one in eight at its
/// edge. Drawn before the light, which covers its middle.
pub(super) fn fine_glow(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, color: Color) {
    for py in drawn(canvas, y, h) {
        let span = ellipse_span(x, y, w, h, py);
        // Its outer quarter, each side, is sparser.
        let edge = span.len() as i32 / 4;
        for px in span.clone() {
            let outer = px < span.start + edge || px >= span.end - edge;
            let period = if outer { 4 } else { 2 };
            if px.rem_euclid(period) == 0 && py.rem_euclid(2) == (px / period).rem_euclid(2) {
                canvas.fine_pixel(px, py, color);
            }
        }
    }
}

/// How a wisp of steam sways, a column a row, over a cycle of 16 rows.
const FINE_SWAY: [i32; 16] = [0, 0, 1, 1, 1, 1, 0, 0, 0, 0, -1, -1, -1, -1, 0, 0];

/// Steam rising `height` rows from `(x, y)` (the bowl's rim), in `wisps`
/// wisps side by side, swaying wider as they rise and broken up towards
/// their tops: the steam of something hot. `tick` lifts it 4 rows a
/// second.
pub(super) fn fine_steam(canvas: &mut Canvas, x: i32, y: i32, height: i32, wisps: i32, tick: u64) {
    let cycle = FINE_SWAY.len() as i32;
    let lift = (tick % FINE_SWAY.len() as u64) as i32 * 4;
    for wisp in 0..wisps {
        let base = x + 5 * wisp - 5 * (wisps - 1) / 2;
        for row in drawn(canvas, y - height, height) {
            let up = y - row;
            let phase = (up + lift + wisp * 6).rem_euclid(cycle);
            // Solid low down; every other row above a third of the way,
            // and every third above two thirds, so it thins out.
            let gap = match 3 * up / height {
                0 => false,
                1 => phase % 2 == 1,
                _ => phase % 3 != 0,
            };
            if gap {
                continue;
            }
            let color = if 3 * up < 2 * height {
                Color::TEXT
            } else {
                Color::TEXT_DIM
            };
            let sway = FINE_SWAY[phase as usize] * (1 + 2 * up / height);
            // Two pixels wide low down, where it's thickest.
            let w = if 3 * up < height { 2 } else { 1 };
            canvas.fine_rect(base + sway, row, w, 1, color);
        }
    }
}

/// A light's reflection on a wet street, as [`reflection`] has it, `w`
/// wide from `x`, from row `y` down `rows` rows: dashes in its colour,
/// shorter and sparser away from it.
pub(super) fn fine_reflection(
    canvas: &mut Canvas,
    x: i32,
    w: i32,
    y: i32,
    rows: i32,
    color: Color,
) {
    for row in 0..rows {
        let dash = (10 - 2 * row).max(2);
        let period = dash + 4 + 2 * row;
        let mut dx = (row * 7) % period;
        while dx < w {
            canvas.fine_rect(x + dx, y + 2 * row, dash.min(w - dx), 1, color);
            dx += period;
        }
    }
}

/// The slit between a [`fine_noren`]'s panels.
pub(super) const NOREN_SLIT: i32 = 2;

/// A shop's noren at the panel's resolution, hung from its rail at row
/// `rail` and `h` rows long, from column `x`: `panels` panels `panel`
/// wide, [`NOREN_SLIT`] apart, in `tube` (the app's neon, say), each with
/// the cloth's fold under the rail and its hem shaded; and where there's
/// room (`h` of 20 or more), the shop's crest dyed small on the middle
/// one, a bowl with chopsticks over it, in a shade of the cloth. In the
/// dim look (`dim`), the panels' outlines in `tube`, from the rail
/// itself, and no crest.
///
/// No lettering: the shop's name in letters across the cloth (RAMEN)
/// drew the eye from the notification's sender; the crest is a quiet
/// cue, and the counter, the bowl and its steam say the rest.
#[allow(clippy::too_many_arguments)]
pub(super) fn fine_noren(
    canvas: &mut Canvas,
    x: i32,
    rail: i32,
    h: i32,
    panel: i32,
    panels: i32,
    tube: Color,
    dim: bool,
) {
    for i in 0..panels {
        let x = x + i * (panel + NOREN_SLIT);
        if dim {
            canvas.fine_rect(x, rail, panel, 1, tube);
            canvas.fine_rect(x, rail + h - 1, panel, 1, tube);
            canvas.fine_rect(x, rail, 1, h, tube);
            canvas.fine_rect(x + panel - 1, rail, 1, h, tube);
            continue;
        }
        canvas.fine_rect(x, rail + 1, panel, h, tube);
        canvas.fine_tone(x, rail + 1, panel, 2, 2, Color::SHADOW);
        canvas.fine_tone(x, rail + h - 1, panel, 2, 2, Color::SHADOW);
        if i == panels / 2 && h >= 20 {
            // The crest, in a checker of the dark over the cloth: the
            // bowl, wide at its rim, and two chopsticks leaning over it.
            let (mx, top) = (x + panel / 2, rail + h / 2 + 2);
            for (dy, half) in [(0, 7), (1, 7), (2, 6), (3, 5), (4, 3)] {
                canvas.fine_tone(mx - half, top + dy, 2 * half, 1, 2, Color::SHADOW);
            }
            for (from, to) in [(-3, 3), (0, 6)] {
                for step in 0..7 {
                    let (cx, cy) = (mx + from + step * (to - from) / 6, top - 1 - step);
                    canvas.fine_tone(cx, cy, 2, 1, 2, Color::SHADOW);
                }
            }
        }
    }
}

// ==============================================================================
// For the alarms' boards, the crossing and finding the watch (alarms.rs)
// ==============================================================================

/// `text` in the small font as a station board's LEDs, from the canvas's
/// `(x, y)` (as `Canvas::text` takes them): each of the font's pixels an
/// LED of 2×2 of the panel's, 3 apart, as [`led_digit`]'s are, so that a
/// board's rows read as its dot matrix rather than as the canvas's
/// blocks. Not `fine`, the canvas's text, for a row that changes every
/// second (a running timer's), whose rows are cheaper sent coarse (see
/// `Canvas::refine`). Returns the x just past it, as `Canvas::text` does.
pub(super) fn led_text(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    fine: bool,
) -> i32 {
    if !fine {
        return canvas.text(x, y, text, color);
    }
    let top = y - font::ACCENT_ROWS;
    let rows = canvas.fine_rows();
    let drawn = 3 * top < rows.end && 3 * (y + 5) > rows.start;
    let mut cursor = x;
    for c in text.chars() {
        let glyph = font::glyph(c);
        let w = glyph.width as i32;
        for (dy, &bits) in glyph.rows.iter().enumerate().filter(|_| drawn) {
            for dx in (0..w).filter(|dx| bits >> (w - 1 - dx) & 1 != 0) {
                let (px, py) = (3 * (cursor + dx), 3 * (top + dy as i32));
                canvas.fine_rect(px, py, 2, 2, color);
            }
        }
        cursor += w + 1;
    }
    cursor
}

/// An ellipse's outline in the box `w` by `h` from `(x, y)`, `thick`
/// pixels thick (across; a pixel or so down at its top and bottom), lit
/// `level` of four (see `Canvas::fine_tone`): a ripple on water, fainter
/// as it spreads.
pub(super) fn fine_ring(
    canvas: &mut Canvas,
    (x, y, w, h): (i32, i32, i32, i32),
    thick: i32,
    level: u8,
    color: Color,
) {
    for py in drawn(canvas, y, h) {
        let outer = ellipse_span(x, y, w, h, py);
        let inner = ellipse_span(x + thick, y + 1, w - 2 * thick, h - 2, py);
        if inner.is_empty() {
            canvas.fine_tone(outer.start, py, outer.len() as i32, 1, level, color);
        } else {
            canvas.fine_tone(outer.start, py, inner.start - outer.start, 1, level, color);
            canvas.fine_tone(inner.end, py, outer.end - inner.end, 1, level, color);
        }
    }
}

// ==============================================================================
// For the notification's stall and the air's room
// ==============================================================================

/// A ramen bowl at the panel's resolution standing on a counter at row
/// `counter`, centred on column `x`: black, its red band, wide at the rim
/// and narrow at the foot, the noodles' heap catching the light over its
/// rim, and chopsticks across it. Its steam is [`fine_steam`]'s, from 10
/// rows over the counter.
pub(super) fn fine_bowl(canvas: &mut Canvas, x: i32, counter: i32) {
    let rim = counter - 8;
    for dy in [-3, -2] {
        fine_line(
            canvas,
            (x - 13, rim + dy),
            (x + 12, rim + dy + 4),
            Color::TEXT_DIM,
        );
    }
    for (dy, half) in [
        (0, 10),
        (1, 10),
        (2, 9),
        (3, 9),
        (4, 8),
        (5, 7),
        (6, 6),
        (7, 4),
    ] {
        let color = if dy == 2 { Color::HEART } else { Color::SHADOW };
        canvas.fine_rect(x - half, rim + dy, 2 * half, 1, color);
    }
    canvas.fine_rect(x - 9, rim - 1, 18, 1, Color::TEXT_DIM);
}
