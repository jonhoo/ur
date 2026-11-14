//! Living Tokyo's glance: what a look at the wrist shows, the time alone
//! (or with the running timers' time left), the next turn while the phone
//! navigates, or on the charger, how long it has to go; and, just after a
//! media gesture, what it did.
//!
//! All but the charger's are always in the dim look (see
//! [`Glance::dim`]): idle mode's eight colours, still. So they're drawn
//! in its terms: the information solid, in white; the city round it in
//! outlines, in the hints' red; and no tones (`Canvas::fine_tone`'s
//! checkerboards flicker under idle mode's slow refresh: see
//! `Palette::columns`). Their big figures are the city's lettering (see
//! DESIGN.md's Type), as they change by the minute at most; only the
//! timers' time left, which ticks, stays in the pixel fonts.

use super::street::{self, Roof, Windows};
use super::{BACKGROUND, action_icon};
use crate::alarm;
use crate::canvas::{Canvas, Color, FINE};
use crate::font::{self, fine::Style};
use crate::route::Turn;
use crate::themes::helpers::{countdown, hhmm, percent as percent_text, to_full as to_full_text};
use crate::themes::{Glance, STEP_GOAL, ToFull};
use crate::ui::Status;

/// A glance: the time alone, or with the running timers; the next turn
/// while the phone navigates; or the charge on the charger. Only the
/// time's glance shows the timers: the route's has no room to spare, and
/// the charger's is about the charge.
pub(super) fn render_glance(canvas: &mut Canvas, glance: &Glance) {
    match (glance.route, glance.charge) {
        (Some(step), _) => render_route_glance(canvas, glance.status, step),
        (None, Some(to_full)) => {
            render_charge_glance(canvas, glance.status, to_full, glance.dim);
        }
        (None, None) if glance.timers.iter().any(Option::is_some) => {
            render_timers_glance(canvas, glance.status, &glance.timers);
        }
        (None, None) => render_time_glance(canvas, glance.status),
    }
    // Just after a media gesture, what it did, in the top right corner,
    // which every layout leaves free.
    if let Some(action) = glance.media {
        let icon = action_icon(action);
        canvas.sprite(65, 3, font::MEDIA_HINT_W, icon, 2, Color::TEXT);
    }
}

/// `text` in the city's lettering, centred on the panel's middle column,
/// its capitals' top at the panel's row `y`; smoothed at its edges in
/// `edge`, if given (never in the dim look).
fn centred(
    canvas: &mut Canvas,
    y: i32,
    text: &str,
    style: Style,
    color: Color,
    edge: Option<Color>,
) {
    let width = font::Face::Fine(style).width(text);
    canvas.fine_type((FINE - width) / 2, y, text, style, color, edge);
}

/// The time a glance shows small, over what it's for (a turn, the
/// charge): its capitals the height of the small font's at 2× before.
const SMALL_TIME: Style = Style {
    cap: 18,
    stroke: 40,
    caps: true,
};

/// The time, small, at the top of the route's and the charger's glances,
/// in the labels' colour: a glance is still for the time, but there it's
/// not what the look is for.
fn small_time(canvas: &mut Canvas, status: &Status) {
    if let Some(t) = &status.local_time {
        let time = hhmm(i64::from(t.hour) * 60 + i64::from(t.minute));
        centred(canvas, 10, &time, SMALL_TIME, Color::TEXT_DIM, None);
    }
}

// ==============================================================================
// The time
// ==============================================================================

/// The time glance's figures: as tall as the 4× pixel digits were (28
/// canvas rows), less a little for the city under them, in a pen of 8
/// pixels, a little lighter than their 12-pixel strokes, as a stroke
/// face of this size reads bolder than pixel art.
const BIG: Style = Style {
    cap: 66,
    stroke: 128,
    caps: true,
};

/// The glance: the hours above the minutes, as big as fits with room
/// round them, and at the foot, the far city, its windows lit by the
/// day's steps.
///
/// The hours and minutes are the city's lettering rather than the pixel
/// digits: they change once a minute, so their rows, redrawn finely
/// each second the glance shows (see `Canvas::refine`), go to the panel
/// once. By proximity, the gap between the hours and the minutes is
/// twice the gap between two digits, so they read as two numbers rather
/// than a grid of four digits. The block has 14 rows above it, and at
/// least as many between it and the city's tallest mast below.
///
/// Under it, the city a glance has room for (see [`foot`]). Still, as
/// the dim look is.
fn render_time_glance(canvas: &mut Canvas, status: &Status) {
    canvas.clear(BACKGROUND);
    let Some(t) = &status.local_time else {
        return;
    };
    foot(canvas, status.steps.unwrap_or(0));
    const TOP: i32 = 14;
    const STEP: i32 = BIG.height() + 2 * BIG.gap();
    let time = hhmm(i64::from(t.hour) * 60 + i64::from(t.minute));
    // The hours, then the minutes, of "HH:MM".
    for (y, from) in [(TOP, 0), (TOP + STEP, 3)] {
        let digits = font::slice(&time, from, from + 2);
        centred(canvas, y, digits, BIG, Color::TEXT, None);
    }
}

/// The far city along the time glance's foot, left to right: each
/// building's width and height, in the panel's pixels, and what's on
/// its roof. Three apart, they span the screen from edge to edge; low,
/// so there are only so many windows to light (about 250).
const FOOT: [(i32, i32, Roof); 9] = [
    (22, 20, Roof::Flat),
    (16, 30, Roof::Antenna),
    (28, 16, Roof::Flat),
    (19, 26, Roof::Flat),
    (31, 21, Roof::Flat),
    (16, 33, Roof::Antenna),
    (25, 18, Roof::Flat),
    (22, 27, Roof::Flat),
    (34, 22, Roof::Flat),
];

/// The far city as the time glance has room for it: a row of low
/// buildings along its foot, outlined in the hints' red, their windows
/// lit with `steps` as the clock face's are with the day's: a share of
/// them as far as the goal, all of them there. Only the lit ones show,
/// in the windows' warm light (the dim look's white): a few lights at
/// the foot of the dark after a quiet morning, the blocks lit up after a
/// long walk.
fn foot(canvas: &mut Canvas, steps: u32) {
    let lit = steps.min(STEP_GOAL) * 256 / STEP_GOAL;
    let light = |index: u32| street::lit_window(index, lit).then_some(Color::HEART_SHINE);
    let mut x = 2;
    let mut first = 0;
    for &(w, h, roof) in &FOOT {
        let top = FINE - h;
        // Its wall the night's, so only its outline and lights show.
        first = street::fine_building(
            canvas,
            (x, top, w, FINE),
            BACKGROUND,
            roof,
            Some(Windows {
                first,
                lit: &light,
                glass: None,
            }),
        );
        canvas.fine_rect(x, top, 1, h, Color::TEXT_FAINT);
        canvas.fine_rect(x + w - 1, top, 1, h, Color::TEXT_FAINT);
        x += w + 3;
    }
}

// ==============================================================================
// The timers
// ==============================================================================

/// The timers glance's time: a little smaller than the 3× pixel digits'
/// row was (180 pixels wide to its 213, 55 tall to its 63), as the
/// stroke face's open shapes read bigger than pixel art's blocks.
const ROW_TIME: Style = Style {
    cap: 48,
    stroke: 104,
    caps: true,
};

/// The glance while a timer runs: the time in one row, and under it a row
/// for each running timer, an hourglass and the time left in the medium
/// face, as the Alarms screen writes it ("42 MIN", then "7:45" under 10
/// minutes, "2H05" beyond the hour).
///
/// The time gives up a size rather than share the screen with the
/// stacked hours and minutes, which leave only the 17 canvas pixels
/// either side of them, room for 1× text that crowds the edges. In a row,
/// it's still the biggest thing on the screen, in the city's lettering
/// (it changes once a minute). It starts at the panel's row 45, under
/// the band where a media gesture's icon shows with a gap: a block
/// centred on the screen would run into that icon with two timers.
///
/// The rows are the medium face (a notification's name's: 7 px capitals,
/// bold, with 2 px stems) beside the 5×7 hourglass at 1×, the same
/// height, on the canvas's grid: they tick, and a fine row that changes
/// goes to the panel whole each second (see `Canvas::refine`), where
/// the canvas sends a changed figure's few pixels. They're spaced in even
/// bands: as much above the rows as under them, and less between them,
/// so they read as one list under the time.
///
/// The timers go soonest first, which is what the wearer looks for, and
/// their order only changes when one is started again. They aren't
/// labelled TIMER 1 and TIMER 2: they're set to different lengths for
/// different things, and the wearer who set them knows the tea from the
/// laundry by how long is left. The rows share a left edge, the widest
/// centred, so the hourglasses stand in a column, as a list.
///
/// Under 10 minutes, the seconds tick: the UI draws a frame every second
/// while the screen is on, a glance included, and pushes only the rows
/// that changed, so that's a few rows a second for the glance's few
/// seconds, and no extra wake-ups. A timer whose time is up shows "0:00"
/// for the moment (under a second) before it rings, which takes the
/// screen; one ringing is no longer running, so it isn't here.
fn render_timers_glance(
    canvas: &mut Canvas,
    status: &Status,
    timers: &[Option<i64>; alarm::TIMERS],
) {
    // Two rows fit under the time; a third timer would need a new layout.
    const _: () = assert!(alarm::TIMERS == 2, "a row a timer, two at most");
    const ICON_W: i32 = font::HOURGLASS_W as i32;
    const GAP: i32 = 3;
    canvas.clear(BACKGROUND);
    if let Some(t) = &status.local_time {
        let time = hhmm(i64::from(t.hour) * 60 + i64::from(t.minute));
        centred(canvas, 45, &time, ROW_TIME, Color::TEXT, None);
    }

    // Soonest first (stopped timers, `None`, last).
    let mut running = *timers;
    running.sort_unstable_by_key(|left| (left.is_none(), *left));
    let texts = running.map(|left| left.map(countdown));
    // The room under the time is the canvas's y 37..79 (43 px), and a row
    // is 7 px. One row centred in it, 18 px above and below; two with
    // 11 px above and below, and 7 px (a row's height) between them.
    let rows: &[i32] = match texts.iter().flatten().count() {
        1 => &[55],
        _ => &[48, 62],
    };
    let widest = texts
        .iter()
        .flatten()
        .map(|text| font::Face::Medium.width(text))
        .max()
        .unwrap_or(0);
    let x = 40 - (ICON_W + GAP + widest) / 2;
    // The widest, "10 MIN" (or a 16-hour timer's "16H00"), leaves room
    // either side.
    debug_assert!(x >= 4, "a timer's row runs off the glance");
    for (text, &y) in texts.iter().flatten().zip(rows) {
        canvas.sprite(x, y, font::HOURGLASS_W, &font::HOURGLASS, 1, Color::ACCENT);
        canvas.text_medium(x + ICON_W + GAP, y, text, Color::TEXT);
    }
}

// ==============================================================================
// The route
// ==============================================================================

/// The distance to the turn: as the phone writes it ("150 m", "1,2
/// km"), units in lowercase.
const DISTANCE: Style = Style {
    cap: 26,
    stroke: 64,
    caps: false,
};

/// The street plate's lettering: a sign's.
const PLATE: Style = street::LETTERING;

/// The glance while the phone navigates: the next turn takes the middle of
/// the screen, as an arrow with the distance under it, then the street on
/// a street plate, as a corner's in Tokyo has it (two lines at most). The
/// time stays, smaller, at the top: a glance is still for the time too,
/// but on the way somewhere, the turn is what a look at the wrist is for.
///
/// From the top, in the panel's pixels: the time (to 30), the arrow (16
/// cells of 6, from 44), the distance (from 148), then the plate in the
/// room from 186 to 238, 48 rows for two lines (and 4 more over a name
/// with accents, which reach above its capitals); a one-line plate, a
/// sign's height, centred in that room. All but the plate's tube is
/// white or red, the dim look's own, and none of it a tone.
fn render_route_glance(canvas: &mut Canvas, status: &Status, step: &crate::route::Step) {
    canvas.clear(BACKGROUND);
    const ARROW_Y: i32 = 44;
    const DISTANCE_Y: i32 = ARROW_Y + 16 * CELL + 8;
    const PLATE_Y: i32 = 186;
    const PLATE_H: i32 = 52;
    small_time(canvas, status);

    let (arrow, mirror) = match step.turn {
        Turn::Straight => (&font::TURN_STRAIGHT, false),
        Turn::Right => (&font::TURN_RIGHT, false),
        Turn::Left => (&font::TURN_RIGHT, true),
        Turn::SlightRight | Turn::KeepRight => (&font::TURN_SLIGHT_RIGHT, false),
        Turn::SlightLeft | Turn::KeepLeft => (&font::TURN_SLIGHT_RIGHT, true),
        Turn::SharpRight => (&font::TURN_SHARP_RIGHT, false),
        Turn::SharpLeft => (&font::TURN_SHARP_RIGHT, true),
        // Gadgetbridge doesn't know which side the traffic keeps to, so a
        // U-turn at a roundabout is drawn as a plain one, going round left.
        Turn::UturnLeft | Turn::RoundaboutUturn => (&font::TURN_UTURN_LEFT, false),
        Turn::UturnRight => (&font::TURN_UTURN_LEFT, true),
        Turn::RoundaboutRight => (&font::TURN_ROUNDABOUT_RIGHT, false),
        Turn::RoundaboutLeft => (&font::TURN_ROUNDABOUT_RIGHT, true),
        Turn::RoundaboutStraight => (&font::TURN_ROUNDABOUT_STRAIGHT, false),
        Turn::Finish => (&font::TURN_FINISH, false),
        Turn::OffRoute => (&font::TURN_OFF_ROUTE, false),
        Turn::Unknown => (&font::TURN_UNKNOWN, false),
    };
    smooth_arrow(canvas, (FINE - 16 * CELL) / 2, ARROW_Y, arrow, mirror);

    // "1,234 km" fits big; the smaller size is for safety.
    let room = FINE - 12;
    let style = if font::Face::Fine(DISTANCE).width(&step.distance) <= room {
        DISTANCE
    } else {
        SMALL_TIME
    };
    centred(canvas, DISTANCE_Y, &step.distance, style, Color::TEXT, None);

    // The street on its plate: a sign's box round its lines (none for a
    // turn on no named street), its tube in the hints' colour as street
    // furniture is, the name in the text's. Laid out only in the passes
    // that draw it (the plate's rows), as balancing it takes a few.
    let street = step.street.trim();
    let rows = canvas.fine_rows();
    if street.is_empty() || rows.end <= PLATE_Y || rows.start >= PLATE_Y + PLATE_H {
        return;
    }
    // Inside the tube, a clear column and a little more either side.
    const MARGIN: i32 = 9;
    let mut lines = [font::Line::default(); 2];
    let face = font::Face::Fine(PLATE);
    let count = font::wrap(street, face, FINE - 6 - 2 * MARGIN, &mut lines);
    let lines = &lines[..count];
    let widest = lines.iter().map(|line| line.width(street, face)).max();
    let plate_w = widest.unwrap_or(0) + 2 * MARGIN;
    // A sign's height a line, and a line's pitch (the small font's 7
    // canvas rows) more for the second; and room for accents over the
    // capitals (Latin-1's letters past the ASCII), clear of the tube.
    let accents = if street.chars().any(|c| c >= '\u{c0}') {
        4
    } else {
        0
    };
    let plate_h = 3 * street::SIGN_H + 21 * (count as i32 - 1) + accents;
    let (plate_x, plate_y) = ((FINE - plate_w) / 2, PLATE_Y + (PLATE_H - plate_h) / 2);
    // Always dim (a glance): its glow solid, untoned.
    let plate = (plate_x, plate_y, plate_w, plate_h);
    street::fine_sign_in(canvas, plate, Color::TEXT_FAINT, true);
    for (i, &line) in lines.iter().enumerate() {
        let x = (FINE - line.width(street, face)) / 2;
        let y = plate_y + street::LETTERING_AT.1 - 3 + accents + 21 * i as i32;
        canvas.line(x, y, street, line, face, Color::TEXT);
    }
}

/// A turn arrow's cell, in the panel's pixels: its 16×16 art 96 across,
/// as the canvas's 2× had it.
const CELL: i32 = 6;

/// A turn's arrow (its art, as `Canvas::sprite16` takes it) at the
/// panel's resolution, in the text's white, from `(x, y)`: its cells
/// [`CELL`] pixels a side, and its stair-stepped edges smoothed into
/// slopes, so that an arrowhead is a point and a bend a curve, as a road
/// sign's are, rather than a pixel-art staircase.
///
/// The smoothing is Scale2x's, at a cell's size: each cell's corners are
/// looked at in turn, and where both its neighbours towards a corner
/// differ from the cell (both empty beside a cell of the arrow; both of
/// it beside an empty one), the corner's triangle, half a cell along each
/// side, takes their side's colour: cut from the arrow at an outer
/// corner, filled into it at an inner one. Along a staircase of single
/// steps, the triangles meet into a straight 45° edge; a lone cell
/// sticking out (a tip) becomes a point. No new art: the arrows are the
/// 2× glance's, so each still reads as before, sharpened.
fn smooth_arrow(canvas: &mut Canvas, x: i32, y: i32, art: &[u16; 16], mirror: bool) {
    const HALF: i32 = CELL / 2;
    let set = |cx: i32, cy: i32| -> bool {
        if !(0..16).contains(&cx) || !(0..16).contains(&cy) {
            return false;
        }
        let column = if mirror { 15 - cx } else { cx };
        art[cy as usize] >> (15 - column) & 1 != 0
    };
    let rows = canvas.fine_rows();
    for cy in 0..16 {
        let top = y + cy * CELL;
        if top + CELL <= rows.start || top >= rows.end {
            continue;
        }
        for cx in 0..16 {
            let left = x + cx * CELL;
            let on = set(cx, cy);
            if on {
                canvas.fine_rect(left, top, CELL, CELL, Color::TEXT);
            }
            for (dx, dy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
                if set(cx + dx, cy) == on || set(cx, cy + dy) == on {
                    continue;
                }
                let ink = if on { BACKGROUND } else { Color::TEXT };
                // The triangle's rows from the corner inwards, each a
                // pixel shorter, flush with the corner's side.
                for i in 0..HALF {
                    let length = HALF - i;
                    let row = if dy < 0 { top + i } else { top + CELL - 1 - i };
                    let from = if dx < 0 { left } else { left + CELL - length };
                    canvas.fine_rect(from, row, length, 1, ink);
                }
            }
        }
    }
}

// ==============================================================================
// The charge
// ==============================================================================

/// A few stars round the moon on the charger's glance, clear of it and
/// of the text, on the canvas's grid.
const STARS: [(i32, i32); 7] = [
    (8, 14),
    (16, 30),
    (5, 41),
    (66, 13),
    (71, 26),
    (62, 39),
    (75, 47),
];

/// The charge glance's percentage, the label over the time to full, and
/// that time.
const PERCENT: Style = Style {
    cap: 16,
    stroke: 36,
    caps: true,
};
const TO_FULL: Style = Style {
    cap: 40,
    stroke: 96,
    caps: true,
};

/// The glance on the charger: the clock face's moon, the battery, big in
/// a patch of night sky, waxing as it charges, with its percentage under
/// it, and how long the charge has to go, big, below. The time stays,
/// smaller, at the top, as on the route's glance.
///
/// From the top, in the panel's pixels: the time (to 30), the moon (90
/// across, from 36) between stars, its percentage, the label and the
/// time to full, all of it the city's lettering, which changes by the
/// minute at most. Woken on the charger, in the scheme's colours; or, as
/// a glance there, `dim`: in idle mode's eight colours, the text unshaded.
fn render_charge_glance(canvas: &mut Canvas, status: &Status, to_full: ToFull, dim: bool) {
    canvas.clear(BACKGROUND);
    let edge = (!dim).then_some(Color::TEXT_FAINT);
    small_time(canvas, status);
    street::fine_stars(canvas, &STARS, None);
    // The moon's colour says it's charging; in the dim look, where
    // that's white as the rest are (bar a low battery's red, which isn't
    // on the charger), it's outlined instead.
    if dim {
        let lit = status.battery.percent() as i32;
        street::moon(canvas, (75, 36, 90), lit, Color::TEXT, true);
    } else {
        super::clock::moon(canvas, 25, 12, 30, status.battery);
    }
    let percent = percent_text(status.battery.percent());
    centred(canvas, 136, &percent, PERCENT, Color::TEXT, edge);
    let (label, value) = to_full_text(to_full);
    centred(canvas, 166, label, street::LETTERING, Color::TEXT_DIM, None);
    centred(canvas, 188, &value, TO_FULL, Color::TEXT, edge);
}

// ==============================================================================
// The full charge
// ==============================================================================

/// Stars round the full charge's moon and bolt, clear of both.
const CHARGED_STARS: [(i32, i32); 8] = [
    (4, 6),
    (30, 4),
    (40, 13),
    (34, 24),
    (46, 30),
    (5, 29),
    (75, 33),
    (60, 34),
];

/// The lightning's path down the sky, in the panel's pixels, from the top
/// edge to the far city's tallest roof (`clock`'s far city, shifted by
/// its `FAR_SHIFT` to stand here, its top at row 104): a stroke between
/// each point and the next; and a fork off it, from its first kink.
const LIGHTNING: [(i32, i32); 8] = [
    (34, 0),
    (46, 18),
    (52, 31),
    (41, 37),
    (67, 70),
    (55, 75),
    (78, 96),
    (89, 104),
];
const FORK: [(i32, i32); 4] = [(52, 31), (37, 46), (31, 58), (24, 64)];

/// A bolt along `points`, each a row or more below the last: a run of
/// `width` pixels a row, centred on the stroke (and as long as a shallow
/// stroke moves across in a row), lighting `level` of every four.
fn bolt(canvas: &mut Canvas, points: &[(i32, i32)], width: i32, level: u8, color: Color) {
    for pair in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        let at = |y: i32| x0 + (x1 - x0) * (y - y0) / (y1 - y0);
        for y in y0..y1 {
            let (a, b) = (at(y), at(y + 1));
            let w = (a - b).abs() + width;
            canvas.fine_tone(a.min(b) - width / 2, y, w, 1, level, color);
        }
    }
}

/// FULL's lettering: as tall as the 3× small font's was, in a neon's
/// tube.
const FULL: Style = Style {
    cap: 40,
    stroke: 88,
    caps: true,
};

/// The full charge's screen (see `Theme::charged`): the city at night,
/// as the clock face has it, under a full moon (the face's moon is the
/// battery) with lightning striking a roof, and FULL in neon, big, on a
/// sign in front of the city. Not the time: it's up for a few seconds,
/// for a look from across the room.
///
/// The lightning is the panel's: a jagged bolt of the windows' yellow
/// with a white-hot core, and a thinner fork, and a haze where it
/// strikes; the sign a [`street::fine_sign`], lettered in the city's
/// lettering in its own violet.
pub(super) fn render_charged(canvas: &mut Canvas, status: &Status) {
    const MOON_D: i32 = 63;
    const SIGN_Y: i32 = 166;
    const SIGN_H: i32 = 62;
    canvas.clear(BACKGROUND);
    street::fine_stars(canvas, &CHARGED_STARS, None);
    street::fine_moon(canvas, 156, 15, MOON_D, 100, Color::TEXT);
    super::clock::city(canvas, status.steps);
    // A haze round it, its yellow, its white-hot core; the fork fainter.
    bolt(canvas, &LIGHTNING, 13, 1, Color::HEART_SHINE);
    bolt(canvas, &FORK, 7, 1, Color::HEART_SHINE);
    bolt(canvas, &LIGHTNING, 6, 4, Color::HEART_SHINE);
    bolt(canvas, &FORK, 3, 4, Color::HEART_SHINE);
    bolt(canvas, &LIGHTNING, 2, 4, Color::TEXT);
    // The sign, round its lettering, in front of the near buildings.
    let word = "FULL";
    let width = font::Face::Fine(FULL).width(word);
    let sign_w = width + 36;
    let left = (FINE - sign_w) / 2;
    street::fine_sign(canvas, left, SIGN_Y, sign_w, SIGN_H, Color::REM);
    let top = SIGN_Y + (SIGN_H - FULL.height()) / 2;
    canvas.fine_type(left + 18, top, word, FULL, Color::REM, None);
}
