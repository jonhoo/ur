//! Living Tokyo's glance: what a look at the wrist shows, the time alone
//! (or with the running timers' time left), the next turn while the phone
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

/// The glance while a timer runs: the time in one row, HH:MM at 3×, and
/// under it a row for each running timer, an hourglass and the time left
/// at 2×, as the Alarms screen writes it ("42 MIN", then "7:45" under 10
/// minutes, "2H05" beyond the hour).
///
/// The time gives up a size (4× to 3×) rather than share the screen at
/// 4×: the stacked hours and minutes take 65 of the 80 rows, which leaves
/// only the 17 px either side of them, room for 1× text that crowds the
/// edges ("7:45" ends on the last column). At 3×, in a row as the clock
/// face has it (with the clock face's square white colon), it's still the
/// biggest thing on the screen. It starts at y = 15, under the band where
/// a media gesture's icon shows (y 3..12) with a gap: a block centred on
/// the screen would run into that icon with two timers.
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
    const ICON_SCALE: i32 = 2;
    const TEXT_SCALE: i32 = 2;
    const ICON_W: i32 = font::HOURGLASS_W as i32 * ICON_SCALE;
    const GAP: i32 = 4;
    canvas.clear(BACKGROUND);
    if let Some(t) = &status.local_time {
        // 15 px digits 2 px apart, and a 3 px column for the colon, its
        // dots on glyph rows 2 and 4: 71 px, 4 px either side.
        let (hours, minutes) = (u64::from(t.hour), u64::from(t.minute));
        let digits = [hours / 10, hours % 10, minutes / 10, minutes % 10];
        for (digit, x) in digits.into_iter().zip([4, 21, 43, 60]) {
            draw_digit(canvas, x, TIME_Y, digit, TIME_SCALE, Color::TEXT);
        }
        for row in [2, 4] {
            let y = TIME_Y + row * TIME_SCALE;
            canvas.fill_rect(38, y, TIME_SCALE, TIME_SCALE, Color::TEXT);
        }
    }

    // Soonest first (stopped timers, `None`, last).
    let mut running = *timers;
    running.sort_unstable_by_key(|left| (left.is_none(), *left));
    let texts = running.map(|left| left.map(countdown));
    // One row centred in the room under the time and its shadow (y
    // 37..79); two a little closer to each other than to the time, with
    // the glance's 7–8 px margin under the last.
    let rows: &[i32] = match texts.iter().flatten().count() {
        1 => &[51],
        _ => &[42, 59],
    };
    let widest = texts
        .iter()
        .flatten()
        .map(|text| font::small_text_width(text) * TEXT_SCALE)
        .max()
        .unwrap_or(0);
    let x = 40 - (ICON_W + GAP + widest) / 2;
    // The widest, "10 MIN" (or a 16-hour timer's "16H00"), leaves room
    // either side.
    debug_assert!(x >= 4, "a timer's row runs off the glance");
    for (text, &y) in texts.iter().flatten().zip(rows) {
        canvas.sprite(
            x,
            y,
            font::HOURGLASS_W,
            &font::HOURGLASS,
            ICON_SCALE,
            Color::ACCENT,
        );
        // The text's capitals (10 px) centred on the hourglass (14 px).
        canvas.text_scaled(x + ICON_W + GAP, y + 2, text, Color::TEXT, TEXT_SCALE);
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

/// A few stars round the moon on the charger's glance, clear of it and
/// of the text.
const STARS: [(i32, i32); 7] = [
    (9, 19),
    (16, 31),
    (6, 38),
    (62, 18),
    (70, 27),
    (57, 36),
    (74, 40),
];

/// The glance on the charger: the clock face's moon, the battery, big in
/// a patch of night sky, waxing as it charges, with its percentage under
/// it, and how long the charge has to go, big, below. The time stays,
/// smaller, at the top, as on the route's glance.
///
/// From the top: the time (2×, 10 px), the moon (19 px across) between
/// stars, its percentage (1×), the label (1×) and the time to full (3×).
fn render_charge_glance(canvas: &mut Canvas, status: &Status, to_full: ToFull) {
    canvas.clear(BACKGROUND);
    const TIME_Y: i32 = 3;
    const MOON_D: i32 = 19;
    const MOON_Y: i32 = 17;
    const PERCENT_Y: i32 = MOON_Y + MOON_D + 3;
    const LABEL_Y: i32 = PERCENT_Y + 9;
    const VALUE_Y: i32 = LABEL_Y + 8;
    let centred = |canvas: &mut Canvas, y: i32, text: &str, scale: i32, color: Color| {
        let width = font::small_text_width(text) * scale;
        canvas.text_scaled(40 - width / 2, y, text, color, scale);
    };
    if let Some(t) = &status.local_time {
        let time = hhmm(i64::from(t.hour) * 60 + i64::from(t.minute));
        centred(canvas, TIME_Y, &time, 2, Color::TEXT_DIM);
    }
    for (i, &(x, y)) in STARS.iter().enumerate() {
        let color = if i % 3 == 0 {
            Color::TEXT_DIM
        } else {
            Color::TEXT_FAINT
        };
        canvas.pixel(x, y, color);
    }
    super::clock::moon(canvas, 40 - MOON_D / 2, MOON_Y, MOON_D, status.battery);
    centred(
        canvas,
        PERCENT_Y,
        &percent_text(status.battery.percent()),
        1,
        Color::TEXT,
    );
    let (label, value) = to_full_text(to_full);
    centred(canvas, LABEL_Y, label, 1, Color::TEXT_DIM);
    centred(canvas, VALUE_Y, &value, 3, Color::TEXT);
}

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

/// The lightning's path down the sky, from the top edge to the tallest
/// far building's roof (`clock`'s `FAR`, at x 28–33, its top at 35): a
/// stroke between each point and the next; and a fork off it, from its
/// first kink.
const LIGHTNING: [(i32, i32); 6] = [(11, 0), (17, 10), (13, 12), (22, 23), (18, 25), (29, 35)];
const FORK: [(i32, i32); 3] = [(17, 10), (10, 16), (8, 21)];

/// Draws a stroke through `points`, each pixel of it a `size`-sided
/// square from it down and right.
fn stroke(canvas: &mut Canvas, points: &[(i32, i32)], size: i32, color: Color) {
    for pair in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        let steps = (x1 - x0).abs().max((y1 - y0).abs());
        for i in 0..=steps {
            let x = x0 + (x1 - x0) * i / steps;
            let y = y0 + (y1 - y0) * i / steps;
            canvas.fill_rect(x, y, size, size, color);
        }
    }
}

/// The full charge's screen (see `Theme::charged`): the city at night,
/// as the clock face has it, under a full moon (the face's moon is the
/// battery) with lightning striking a roof, and FULL in neon, big, on a
/// sign in front of the city. Not the time: it's up for a few seconds,
/// for a look from across the room.
pub(super) fn render_charged(canvas: &mut Canvas, status: &Status) {
    const MOON_D: i32 = 21;
    const MOON_X: i32 = 52;
    const MOON_Y: i32 = 5;
    const SCALE: i32 = 3;
    const SIGN_Y: i32 = 56;
    const SIGN_H: i32 = 5 * SCALE + 6;
    canvas.clear(BACKGROUND);
    for (i, &(x, y)) in CHARGED_STARS.iter().enumerate() {
        let color = if i % 3 == 0 {
            Color::TEXT_DIM
        } else {
            Color::TEXT_FAINT
        };
        canvas.pixel(x, y, color);
    }
    super::clock::disc(canvas, MOON_X, MOON_Y, MOON_D, Color::TEXT);
    super::clock::city(canvas, status.steps);
    // The lightning: a white-hot core in a glow of the windows' yellow,
    // three pixels across, so it reads at a glance; its fork thinner.
    stroke(canvas, &LIGHTNING, 3, Color::HEART_SHINE);
    stroke(canvas, &FORK, 2, Color::HEART_SHINE);
    let core: [(i32, i32); 6] = LIGHTNING.map(|(x, y)| (x + 1, y + 1));
    stroke(canvas, &core, 1, Color::TEXT);
    // The sign: its box in the near buildings' colour, as the clock
    // face's are, with a lit tube's FULL.
    let word = "FULL";
    let width = font::small_text_width(word) * SCALE;
    let left = 40 - width / 2;
    canvas.fill_rect(left - 3, SIGN_Y, width + 6, SIGN_H, Color::SHADOW);
    canvas.text_scaled(left, SIGN_Y + 3, word, Color::REM, SCALE);
}
