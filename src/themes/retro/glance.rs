//! Retro's glance: what a look at the wrist shows, the time alone (or with
//! the running timers' time left), the next turn while the phone
//! navigates, or on the charger, how long it has to go; and, just after a
//! media gesture, what it did.

use super::{BACKGROUND, action_icon, draw_digit};
use crate::alarm;
use crate::canvas::{Canvas, Color};
use crate::font;
use crate::route::Turn;
use crate::themes::helpers::{countdown, hhmm, percent as percent_text, to_full as to_full_text};
use crate::themes::{Glance, ToFull};
use crate::ui::Status;

/// A glance: the time alone, or with the running timers; the next turn
/// while the phone navigates; or the charge on the charger. Only the
/// time's glance shows the timers: the route's has no room to spare, and
/// the charger's is about the charge.
pub(super) fn render_glance(canvas: &mut Canvas, glance: &Glance) {
    match (glance.route, glance.charge) {
        (Some(step), _) => render_route_glance(canvas, glance.status, step),
        (None, Some(to_full)) => render_charge_glance(canvas, glance.status, to_full),
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

/// The glance: the hours above the minutes, as big as fits with room
/// around them, and nothing else.
///
/// The sizes follow from three rules. The digits scale by whole numbers
/// (they're pixel art). The block keeps a margin of about a tenth of the
/// screen (8 px), as one big focal element needs room to read as placed
/// rather than spilling over. And by proximity, the gap between the
/// hours' digits (one glyph cell) is half the gap between hours and
/// minutes (two cells), so they read as two numbers rather than a grid of
/// four digits. 5× (25×35 digits) can't keep the margin; 4× (20×28) can:
/// a 45×65 block, with 7–8 px above and below.
fn render_time_glance(canvas: &mut Canvas, status: &Status) {
    canvas.clear(BACKGROUND);
    let Some(t) = &status.local_time else {
        return;
    };
    const SCALE: i32 = 4;
    const WIDTH: i32 = font::DIGIT_W as i32 * SCALE;
    const HEIGHT: i32 = 7 * SCALE;
    const GAP_X: i32 = SCALE;
    const GAP_Y: i32 = 2 * SCALE;
    // Centred, counting the 1 px shadow.
    const LEFT: i32 = (80 - (2 * WIDTH + GAP_X + 1)) / 2;
    const TOP: i32 = (80 - (2 * HEIGHT + GAP_Y + 1)) / 2;
    for (row, value) in [t.hour, t.minute].into_iter().enumerate() {
        let y = TOP + row as i32 * (HEIGHT + GAP_Y);
        let value = u64::from(value);
        draw_digit(canvas, LEFT, y, value / 10, SCALE, Color::TEXT);
        draw_digit(
            canvas,
            LEFT + WIDTH + GAP_X,
            y,
            value % 10,
            SCALE,
            Color::TEXT,
        );
    }
}

/// The glance while a timer runs: the time as the clock face has it, in
/// one row at 3×, and under it a row for each running timer, an hourglass
/// and the time left in the medium face, as the clock face and the Alarms
/// screen write it ("42 MIN", then "7:45" under 10 minutes, "2H05" beyond
/// the hour).
///
/// The time gives up a size (4× to 3×) rather than share the screen at
/// 4×: the stacked hours and minutes take 65 of the 80 rows, which leaves
/// only the 17 px either side of them, room for 1× text that crowds the
/// edges ("7:45" ends on the last column). At 3×, it's still the biggest
/// thing on the screen, and it's the clock face's own time, near the
/// clock face's place, so a glance with a timer reads as the watch's face
/// with the timers brought forward. It starts at y = 15, two rows below
/// the clock face's, under the band where a media gesture's icon shows
/// (y 3..12) with a gap: a block centred on the screen would run into
/// that icon with two timers.
///
/// The colon is steady, in the accent, unlike the clock face's blinking
/// one: nothing on a glance moves but what has to (see below).
///
/// The rows are the medium face (a notification's name's: 7 px capitals,
/// bold, with 2 px stems) beside the 5×7 hourglass at 1×, the same
/// height: a size between the small font at 1× (5 px, hairline) and at
/// 2× (10 px), which crowded the room under the time. Smaller, they leave
/// that room as space around them, in even bands: as much above the rows
/// as under them, and less between them, so they read as one list under
/// the time.
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
    const TIME_Y: i32 = 15;
    const TIME_SCALE: i32 = 3;
    const ICON_W: i32 = font::HOURGLASS_W as i32;
    const GAP: i32 = 3;
    canvas.clear(BACKGROUND);
    if let Some(t) = &status.local_time {
        // As the clock face draws it: 15 px digits 2 px apart, and a 3 px
        // column for the colon, its dots on glyph rows 2 and 4.
        let (hours, minutes) = (u64::from(t.hour), u64::from(t.minute));
        let digits = [hours / 10, hours % 10, minutes / 10, minutes % 10];
        for (digit, x) in digits.into_iter().zip([4, 21, 43, 60]) {
            draw_digit(canvas, x, TIME_Y, digit, TIME_SCALE, Color::TEXT);
        }
        for row in [2, 4] {
            let y = TIME_Y + row * TIME_SCALE;
            canvas.fill_rect(39, y + 1, 3, 3, Color::SHADOW);
            canvas.fill_rect(38, y, 3, 3, Color::ACCENT);
        }
    }

    // Soonest first (stopped timers, `None`, last).
    let mut running = *timers;
    running.sort_unstable_by_key(|left| (left.is_none(), *left));
    let texts = running.map(|left| left.map(countdown));
    // The room under the time and its shadow is y 37..79 (43 px), and a
    // row is 7 px. One row centred in it, 18 px above and below; two with
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

/// The glance while the phone navigates: the next turn takes the middle of
/// the screen, as an arrow with the distance under it, then the street
/// (two lines of the small font at most, as Gadgetbridge gives it). The
/// time stays, smaller, at the top: a glance is still for the time too,
/// but on the way somewhere, the turn is what a look at the wrist is for.
///
/// From the top: the time (2×, 10 px), the arrow (16×16 at 2×), the
/// distance (2×), and the street (1×), with 4 px between them; it just
/// fits, with the street's second line ending a pixel above the bottom.
fn render_route_glance(canvas: &mut Canvas, status: &Status, step: &crate::route::Step) {
    canvas.clear(BACKGROUND);
    const TIME_Y: i32 = 3;
    const ARROW_Y: i32 = 17;
    const DISTANCE_Y: i32 = ARROW_Y + 32 + 4;
    const STREET_Y: i32 = DISTANCE_Y + 10 + 4;
    let centred = |canvas: &mut Canvas, y: i32, text: &str, scale: i32, color: Color| {
        let width = font::small_text_width(text) * scale;
        canvas.text_scaled(40 - width / 2, y, text, color, scale);
    };
    if let Some(t) = &status.local_time {
        let time = hhmm(i64::from(t.hour) * 60 + i64::from(t.minute));
        centred(canvas, TIME_Y, &time, 2, Color::TEXT_DIM);
    }

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
    let arrow_x = 40 - font::TURN_W as i32;
    canvas.sprite16(arrow_x, ARROW_Y, arrow, mirror, 2, Color::TEXT);

    // "1,234 km" fits at 2× (62 px); the 1× fallback is for safety.
    let scale = if font::small_text_width(&step.distance) * 2 <= 76 {
        2
    } else {
        1
    };
    centred(canvas, DISTANCE_Y, &step.distance, scale, Color::TEXT);

    let mut lines = [font::Line::default(); 2];
    let street = step.street.trim();
    match font::fill(street, font::Face::Small, 76, &mut lines) {
        count @ 0..=2 => {
            for (i, &line) in lines[..count].iter().enumerate() {
                let width = font::small_text_width(line.of(street))
                    + font::small_text_width(line.tail.as_str())
                    + if line.tail == font::Tail::None { 0 } else { 1 };
                let y = STREET_Y + i as i32 * 7;
                canvas.line(
                    40 - width / 2,
                    y,
                    street,
                    line,
                    font::Face::Small,
                    Color::TEXT_DIM,
                );
            }
        }
        // Too long even for two lines (one very long word): its start.
        _ => canvas.text_cut(2, STREET_Y, &step.street, Color::TEXT_DIM),
    }
}

/// The glance on the charger: how long it has to go, big, under the
/// battery as the clock face draws it, three times the size, with its
/// percentage beside it. The time stays, smaller, at the top, as on the
/// route's glance: a look at a watch on its charger is for when it's
/// ready, and the time.
///
/// From the top: the time (2×, 10 px), the battery (3×, 15 px) and its
/// percentage (2×) side by side, the label (1×) and the time to full (3×),
/// with the gaps between them about even, and as much room at the bottom
/// as at the top.
fn render_charge_glance(canvas: &mut Canvas, status: &Status, to_full: ToFull) {
    canvas.clear(BACKGROUND);
    const TIME_Y: i32 = 3;
    const BATTERY_Y: i32 = 20;
    const LABEL_Y: i32 = 45;
    const VALUE_Y: i32 = 53;
    let centred = |canvas: &mut Canvas, y: i32, text: &str, scale: i32, color: Color| {
        let width = font::small_text_width(text) * scale;
        canvas.text_scaled(40 - width / 2, y, text, color, scale);
    };
    if let Some(t) = &status.local_time {
        let time = hhmm(i64::from(t.hour) * 60 + i64::from(t.minute));
        centred(canvas, TIME_Y, &time, 2, Color::TEXT_DIM);
    }

    // The clock face's battery (10×5, a 1×3 nub, 8×3 inside), at 3×. Not
    // its charging animation (the fill creeping up to full): the bolt
    // says it's charging, and in a few seconds' look, a still fill says
    // how far it's got.
    const SCALE: i32 = 3;
    let battery = status.battery;
    let percent = battery.percent();
    let text = percent_text(percent);
    let text_w = font::small_text_width(&text) * 2;
    let left = (80 - (11 * SCALE + 4 + text_w)) / 2;
    canvas.fill_rect(left, BATTERY_Y, 10 * SCALE, 5 * SCALE, Color::TEXT);
    canvas.fill_rect(
        left + 10 * SCALE,
        BATTERY_Y + SCALE,
        SCALE,
        3 * SCALE,
        Color::TEXT,
    );
    canvas.fill_rect(
        left + SCALE,
        BATTERY_Y + SCALE,
        8 * SCALE,
        3 * SCALE,
        BACKGROUND,
    );
    let fill = (percent * 8).div_ceil(100) as i32;
    canvas.fill_rect(
        left + SCALE,
        BATTERY_Y + SCALE,
        fill * SCALE,
        3 * SCALE,
        Color::GOOD,
    );
    // The bolt, as on the clock face, over the battery's middle while
    // charging.
    if battery.charging {
        let bolt_x = left + 5 * SCALE - font::BOLT_W as i32;
        canvas.sprite(
            bolt_x,
            BATTERY_Y + 3,
            font::BOLT_W,
            &font::BOLT,
            2,
            Color::HEART,
        );
    }
    canvas.text_scaled(
        left + 11 * SCALE + 4,
        BATTERY_Y + (5 * SCALE - 10) / 2,
        &text,
        Color::TEXT,
        2,
    );

    let (label, value) = to_full_text(to_full);
    centred(canvas, LABEL_Y, label, 1, Color::TEXT_DIM);
    centred(canvas, VALUE_Y, &value, 3, Color::TEXT);
}

/// The full charge's lightning bolt (see [`render_charged`]), 16×16, a
/// row of `#`s and `.`s each, made bits by [`bolt_rows`].
const BOLT: [&str; 16] = [
    ".........######.",
    "........######..",
    ".......######...",
    "......######....",
    ".....######.....",
    "....######......",
    "...######.......",
    "..############..",
    ".############...",
    ".......#####....",
    "......#####.....",
    ".....####.......",
    "....####........",
    "...###..........",
    "..##............",
    ".#..............",
];

/// [`BOLT`] as bits, for [`Canvas::sprite16`], at compile time.
const fn bolt_rows() -> [u16; 16] {
    let mut out = [0; 16];
    let mut y = 0;
    while y < 16 {
        let row = BOLT[y].as_bytes();
        let mut x = 0;
        while x < 16 {
            if row[x] == b'#' {
                out[y] |= 1 << (15 - x);
            }
            x += 1;
        }
        y += 1;
    }
    out
}

/// The full charge's screen (see `Theme::charged`): the clock face's
/// battery bolt, as big as the glance's digits, with the big digits'
/// drop shadow, and FULL under it, as big as fits. Not the time: it's up
/// for a few seconds, for a look from across the room, which the one
/// shape is for.
///
/// From the top: the bolt (16×16 at 3×, 48 px) and the word (3×, 15 px),
/// 6 px apart, the block centred.
pub(super) fn render_charged(canvas: &mut Canvas, _status: &Status) {
    const SCALE: i32 = 3;
    const ROWS: [u16; 16] = bolt_rows();
    const BOLT_Y: i32 = (80 - (16 * SCALE + 6 + 5 * SCALE)) / 2;
    const WORD_Y: i32 = BOLT_Y + 16 * SCALE + 6;
    canvas.clear(BACKGROUND);
    let x = 40 - 8 * SCALE;
    canvas.sprite16(x + 1, BOLT_Y + 1, &ROWS, false, SCALE, Color::SHADOW);
    canvas.sprite16(x, BOLT_Y, &ROWS, false, SCALE, Color::HEART);
    let word = "FULL";
    let width = font::small_text_width(word) * SCALE;
    canvas.text_scaled(40 - width / 2, WORD_Y, word, Color::TEXT, SCALE);
}
