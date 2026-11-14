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
// The time of night
// ==============================================================================

/// Whether the screen is being drawn in the dim look ([`super::GLANCE`]):
/// a glance, a notification that came with the screen off, the quiet
/// night. There, the city holds still and calm (see DESIGN.md): no rain,
/// no glow, nothing that moves with the seconds.
pub(super) fn dim(canvas: &Canvas) -> bool {
    core::ptr::eq(canvas.palette(), &super::GLANCE)
}

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

/// The sky's light now, for a screen that isn't handed the time: the
/// dead of night until the phone has set the clock.
pub(super) fn daylight_now() -> i32 {
    crate::clock::now().map_or(0, |t| daylight(t.hour))
}

/// The glow of a sky lightening above the horizon, `w` wide from `x`, the
/// row above `y` and up, for `level` of [`daylight`]: dithered rows, the
/// windows' warm yellow nearest the horizon and the neon's violet above
/// it, sparser the higher they are, two more rows a level. Drawn before
/// the buildings, or whatever stands on the horizon.
pub(super) fn horizon(canvas: &mut Canvas, x: i32, w: i32, y: i32, level: i32) {
    for row in 0..2 * level {
        let color = if row < level {
            Color::HEART_SHINE
        } else {
            Color::REM
        };
        // An ordered dither: every other pixel in the two rows on the
        // horizon (a checkerboard), every third in the two above, and so
        // on up.
        let period = 2 + row / 2;
        for dx in 0..w {
            if (dx + row) % period == 0 {
                canvas.pixel(x + dx, y - 1 - row, color);
            }
        }
    }
}
