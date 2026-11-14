//! Living Tokyo's screens of the watch itself: storage mode, switching off,
//! pairing, and an update arriving.
//!
//! Each is a small, still scene round what it has to say, which keeps the
//! light: a shop pulling its shutter down (storage mode), the city's
//! windows going dark (switching off), a ticket machine's display (the
//! passkey), and a train pulling into a platform (an update, as far in
//! as it's come). They show seldom and say one thing each, so each has
//! one thing in it that changes, if any, and is otherwise empty night.
//! In the dim look, the scene keeps its shapes, unlit.

use super::{BACKGROUND, draw_digit, street};
use crate::canvas::{Canvas, Color};
use crate::system_off::Reason;
use crate::themes::helpers::centred_lines;

/// The line pitch of these screens' messages: Info's rows' (see
/// `settings`), kept here, so that either file can be replaced alone.
const LINE: i32 = 8;

/// Lines of a message, centred, from `y` down.
pub(super) fn render_lines(canvas: &mut Canvas, y: i32, lines: &[&str], color: Color) {
    centred_lines(canvas, (40, y), LINE, lines, color);
}

// ==============================================================================
// Storage mode
// ==============================================================================

/// The shop's front: its eave, and the shutter's sides and how far down
/// it's come.
const SHOP_X: i32 = 10;
const SHOP_W: i32 = 60;
const EAVE: i32 = 42;
const SHUTTER_FOOT: i32 = 61;
/// The pavement, under the light from the shop.
const PAVEMENT: i32 = 67;

/// Asks whether to switch to storage mode (see [`crate::system_off`]): a
/// shop closing up for the night, its shutter half down, the light still
/// on under it, and what to do painted on the shutter.
pub(super) fn render_storage_prompt(canvas: &mut Canvas, dim: bool) {
    canvas.clear(BACKGROUND);
    street::title(canvas, "STORAGE MODE", Color::ACCENT);
    let explanation = ["OFF UNTIL THE", "BUTTON OR THE", "CHARGER WAKES IT"];
    render_lines(canvas, 15, &explanation, Color::TEXT);

    // The eave over the shopfront, and its posts down either side (in the
    // dim look, in grey).
    let structure = if dim { Color::TRACK } else { Color::TEXT_FAINT };
    canvas.fill_rect(SHOP_X - 3, EAVE, SHOP_W + 6, 1, structure);
    for x in [SHOP_X - 1, SHOP_X + SHOP_W] {
        canvas.fill_rect(x, EAVE + 1, 1, PAVEMENT - EAVE - 1, structure);
    }
    // The shutter, down past halfway: its slats (a line between the
    // hints' rows) and its foot, a bar, the light from inside still on
    // under it (in the dim look, out).
    let shutter = if dim { BACKGROUND } else { Color::TRACK };
    canvas.fill_rect(SHOP_X, EAVE + 1, SHOP_W, SHUTTER_FOOT - EAVE - 1, shutter);
    for y in [EAVE + 2, EAVE + 10] {
        canvas.fill_rect(SHOP_X, y, SHOP_W, 1, Color::SHADOW);
    }
    canvas.fill_rect(SHOP_X, SHUTTER_FOOT - 1, SHOP_W, 2, structure);
    if !dim {
        canvas.fill_rect(
            SHOP_X,
            SHUTTER_FOOT + 1,
            SHOP_W,
            PAVEMENT - SHUTTER_FOOT - 1,
            Color::HEART_SHINE,
        );
        // Its light, spilling onto the pavement and thinning out.
        for row in 0..3 {
            let y = PAVEMENT + 1 + 2 * row;
            for x in (SHOP_X + row..SHOP_X + SHOP_W - row).step_by(2 + row as usize) {
                canvas.pixel(x, y, Color::HEART_SHINE);
            }
        }
    }
    render_lines(
        canvas,
        EAVE + 4,
        &["TAP: OFF NOW", "ELSE: CANCEL"],
        Color::TEXT_DIM,
    );
    canvas.fill_rect(0, PAVEMENT, 80, 1, structure);
}

// ==============================================================================
// Switching off
// ==============================================================================

/// What shows as the watch switches itself off: why, on its sign, and
/// what wakes it, over the city along the bottom with its lights all but
/// out, as they go when the watch does. Drawn once, still.
pub(super) fn render_off(canvas: &mut Canvas, reason: Reason, dim: bool) {
    canvas.clear(BACKGROUND);
    let (title, tube, wake): (_, _, &[&str]) = match reason {
        Reason::BatteryEmpty => ("BATTERY EMPTY", Color::BAD, &["CHARGE TO", "WAKE IT"]),
        Reason::Storage => (
            "STORAGE MODE",
            Color::ACCENT,
            &["BUTTON OR", "CHARGER", "WAKES IT"],
        ),
    };
    street::title(canvas, title, tube);
    render_lines(canvas, 18, &["SWITCHING OFF"], Color::TEXT_DIM);
    render_lines(canvas, 32, wake, Color::TEXT);
    // The city, its windows nearly all dark: the last few of the hour's
    // (an eighth of them) still lit. Not in the dim look, whose far
    // buildings would be half-toned.
    if !dim {
        let hour = street::local_now().map_or(0, street::hour_of);
        street::fine_skyline(canvas, 0, 240, 240, 3 * 16, street::city_lights(hour) / 8);
    }
}

// ==============================================================================
// Pairing
// ==============================================================================

/// The ticket machine's display, and the passkey's six digits on it: 2×
/// (10 px), 2 px apart, 70 px across.
const DISPLAY_TOP: i32 = 24;
const DIGITS_Y: i32 = 30;

/// The six-digit passkey, on a station ticket machine's display, big
/// enough to read at a glance while typing it on the phone; under it,
/// the machine's fare buttons and its ticket slot. Still.
pub(super) fn render_pairing(canvas: &mut Canvas, passkey: u32, dim: bool) {
    canvas.clear(BACKGROUND);
    street::title(canvas, "PAIRING", Color::ACCENT);
    canvas.text(4, 14, "ENTER ON PHONE:", Color::TEXT_DIM);

    // The machine's front, darker than the night, down to the bottom
    // edge; the display set in it, framed (in the dim look, the frame
    // alone).
    if !dim {
        canvas.fill_rect(1, DISPLAY_TOP - 2, 78, 80 - DISPLAY_TOP + 2, Color::SHADOW);
        canvas.fill_rect(1, DISPLAY_TOP - 2, 78, 1, Color::TRACK);
    }
    let frame = Color::TRACK;
    canvas.fill_rect(2, DISPLAY_TOP, 76, 1, frame);
    canvas.fill_rect(2, DISPLAY_TOP + 21, 76, 1, frame);
    canvas.fill_rect(2, DISPLAY_TOP, 1, 22, frame);
    canvas.fill_rect(77, DISPLAY_TOP, 1, 22, frame);
    if !dim {
        canvas.fill_rect(3, DISPLAY_TOP + 1, 74, 20, BACKGROUND);
    }

    let mut digits = [0u64; 6];
    let mut rest = u64::from(passkey);
    for digit in digits.iter_mut().rev() {
        *digit = rest % 10;
        rest /= 10;
    }
    for (i, &digit) in digits.iter().enumerate() {
        draw_digit(canvas, 5 + 12 * i as i32, DIGITS_Y, digit, 2, Color::TEXT);
    }

    // Under it, a row of fare buttons, and the slot the ticket comes out
    // of.
    if !dim {
        for i in 0..5 {
            let x = 5 + 14 * i;
            canvas.fill_rect(x, 54, 12, 6, Color::TRACK);
            canvas.fill_rect(x, 54, 12, 1, Color::TEXT_FAINT);
        }
        canvas.fill_rect(24, 68, 32, 3, BACKGROUND);
        canvas.fill_rect(24, 67, 32, 1, Color::TEXT_FAINT);
    }
}

// ==============================================================================
// Firmware update
// ==============================================================================

/// The line the train runs on, and the platform's edge in front of it.
const RAILS: i32 = 66;
const PLATFORM: i32 = 70;
/// The train: how tall, and how long a carriage.
const TRAIN_H: i32 = 12;
const CARRIAGE: i32 = 30;

/// How far an update has come, in big figures, and a train pulling into
/// the platform under them, as far in as the update is: its nose at the
/// left edge at the start, at the platform's end when it's done.
pub(super) fn render_update(canvas: &mut Canvas, percent: u8, dim: bool) {
    canvas.clear(BACKGROUND);
    street::title(canvas, "UPDATING", Color::ACCENT);
    canvas.text(4, 14, "KEEP CLOSE", Color::TEXT_DIM);

    // The percentage, in the clock's digits at 3×, centred, the % after
    // it.
    let mut figures = [0u64; 3];
    let mut count = 0;
    let mut rest = u64::from(percent.min(100));
    loop {
        figures[count] = rest % 10;
        count += 1;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    let width = 18 * count as i32 - 3 + 6;
    let mut x = 40 - width / 2;
    for &figure in figures[..count].iter().rev() {
        draw_digit(canvas, x, 25, figure, 3, Color::TEXT);
        x += 18;
    }
    canvas.text(x, 41, "%", Color::TEXT_DIM);

    // The rails, and the platform's edge, its yellow tactile strip along
    // it (in the dim look, the edge alone, in grey).
    if dim {
        canvas.fill_rect(0, PLATFORM + 1, 80, 1, Color::TRACK);
    } else {
        street::rails(canvas, 0, 80, RAILS);
        for x in (0..80).step_by(2) {
            canvas.pixel(x, PLATFORM, Color::HEART_SHINE);
        }
        canvas.fill_rect(0, PLATFORM + 1, 80, 1, Color::TEXT_FAINT);
    }

    // The train, its nose as far across as the update has come.
    let nose = 4 + i32::from(percent.min(100)) * 72 / 100;
    render_train(canvas, nose, dim);
}

/// The train, coming in from the left, its nose's last column at `nose`:
/// silver carriages with a green line along them, lit windows, a
/// rounded nose and its headlight. In the dim look, its roof and its
/// nose, in grey, unlit.
fn render_train(canvas: &mut Canvas, nose: i32, dim: bool) {
    let top = RAILS - TRAIN_H;
    if dim {
        canvas.fill_rect(0, top, nose - 1, 1, Color::TRACK);
        canvas.pixel(nose - 1, top + 1, Color::TRACK);
        canvas.fill_rect(nose, top + 2, 1, TRAIN_H - 2, Color::TRACK);
        return;
    }
    let (body, line, window) = (Color::TEXT_DIM, Color::GOOD, Color::HEART_SHINE);
    // The body, the nose's corner rounded off above.
    canvas.fill_rect(0, top, nose + 1, TRAIN_H, body);
    canvas.fill_rect(nose - 1, top, 2, 1, BACKGROUND);
    canvas.pixel(nose, top + 1, BACKGROUND);
    // The line along it, and the windows over it: each carriage's, in
    // from its ends, and the cab's.
    canvas.fill_rect(0, top + 8, nose + 1, 1, line);
    let mut end = nose;
    while end > 0 {
        let start = end - CARRIAGE + 1;
        let cab = end == nose;
        // (Those of a carriage still off the screen, not.)
        let mut x = start + 3;
        while x + 4 <= end - if cab { 6 } else { 3 } {
            if x >= 0 {
                canvas.fill_rect(x, top + 3, 4, 3, window);
            }
            x += 6;
        }
        if cab {
            canvas.fill_rect(end - 4, top + 3, 3, 3, window);
            canvas.pixel(nose, top + 9, Color::TEXT);
        }
        // The gap between carriages.
        if start > 0 {
            canvas.fill_rect(start - 1, top, 1, TRAIN_H, BACKGROUND);
        }
        end = start - 2;
    }
    // Its wheels, on the rails.
    for x in (2..nose - 2).step_by(10) {
        canvas.fill_rect(x, RAILS - 1, 3, 1, Color::SHADOW);
    }
}
