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
//!
//! The scenes are drawn at the panel's resolution (see
//! `Canvas::fine_rect`), in its pixels; what they say stays in the
//! canvas's fonts, but for the passkey, an LED display's.

use super::{BACKGROUND, draw_digit, street};
use crate::canvas::{Canvas, Color};
use crate::system_off::Reason;
use crate::themes::helpers::{centred_lines, digit_glyph};

/// The line pitch of these screens' messages: Info's rows' (see
/// `settings`), kept here, so that either file can be replaced alone.
const LINE: i32 = 8;

/// Lines of a message, centred, from `y` down.
pub(super) fn render_lines(canvas: &mut Canvas, y: i32, lines: &[&str], color: Color) {
    centred_lines(canvas, (40, y), LINE, lines, color);
}

/// The sides of a rectangle `w` by `h` from `(x, y)`, a pixel of the
/// panel thick, in `color`.
fn fine_outline(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, color: Color) {
    for (x, y, w, h) in [
        (x, y, w, 1),
        (x, y + h - 1, w, 1),
        (x, y, 1, h),
        (x + w - 1, y, 1, h),
    ] {
        canvas.fine_rect(x, y, w, h, color);
    }
}

// ==============================================================================
// Storage mode
// ==============================================================================

/// The shop's front, in the canvas's pixels: its sides, its eave, the
/// shutter's foot (how far down it's come), and the pavement.
const SHOP_X: i32 = 10;
const SHOP_W: i32 = 60;
const EAVE: i32 = 42;
const SHUTTER_FOOT: i32 = 61;
const PAVEMENT: i32 = 67;

/// Asks whether to switch to storage mode (see [`crate::system_off`]): a
/// shop closing up for the night, its shutter half down and what to do
/// painted on it, the light still on under it and the shopkeeper's feet
/// in the light. Still. In the dim look, the shutter's outline, the
/// light out.
///
/// The shutter is ribbed steel, its slats a groove apart, rolled down
/// from a box under the eave, between two posts, a handle in its foot.
pub(super) fn render_storage_prompt(canvas: &mut Canvas, dim: bool) {
    canvas.clear(BACKGROUND);
    street::title(canvas, "STORAGE MODE", Color::ACCENT);
    let explanation = ["OFF UNTIL THE", "BUTTON OR THE", "CHARGER WAKES IT"];
    render_lines(canvas, 15, &explanation, Color::TEXT);

    let (left, right) = (3 * SHOP_X, 3 * (SHOP_X + SHOP_W));
    let (eave, foot, pavement) = (3 * EAVE, 3 * SHUTTER_FOOT, 3 * PAVEMENT);
    let width = right - left;
    // In the dim look, the structure's lines grey, and nothing lit.
    let structure = if dim { Color::TRACK } else { Color::TEXT_FAINT };
    if dim {
        fine_outline(canvas, left, eave, width, foot - eave, structure);
    } else {
        // The light inside, and the shopkeeper's feet in it, by the
        // counter's dark foot.
        canvas.fine_rect(left, foot, width, pavement - foot, Color::HEART_SHINE);
        canvas.fine_rect(left, pavement - 4, width, 4, Color::TRACK);
        for x in [146, 157] {
            canvas.fine_rect(x, foot, 5, pavement - foot - 4, Color::SHADOW);
            canvas.fine_rect(x - 3, pavement - 5, 9, 3, Color::SHADOW);
        }
        // The shutter: steel slats, a groove in the dark and a ridge
        // catching the light every four rows.
        canvas.fine_rect(left, eave, width, foot - eave, Color::TRACK);
        for y in (eave + 3..foot - 3).step_by(4) {
            canvas.fine_rect(left, y, width, 1, Color::SHADOW);
            canvas.fine_tone(left, y + 1, width, 1, 2, Color::TEXT_FAINT);
        }
        // Its light, spilling onto the pavement and thinning out.
        for row in 0..6 {
            let spread = 4 * row;
            let level = if row < 2 { 2 } else { 1 };
            let w = width + 2 * spread;
            canvas.fine_tone(
                left - spread,
                pavement + 1 + row,
                w,
                1,
                level,
                Color::HEART_SHINE,
            );
        }
    }
    // The eave, its edge catching the light, and the box the shutter rolls
    // into under it; the posts either side; the shutter's foot, a bar with
    // a handle.
    canvas.fine_rect(left - 9, eave - 9, width + 18, 6, Color::TRACK);
    canvas.fine_rect(left - 9, eave - 9, width + 18, 1, structure);
    canvas.fine_rect(left, eave - 3, width, 3, structure);
    for x in [left - 4, right] {
        canvas.fine_rect(x, eave - 3, 4, pavement - eave + 3, Color::TRACK);
        canvas.fine_rect(x, eave - 3, 1, pavement - eave + 3, structure);
    }
    canvas.fine_rect(left, foot - 3, width, 3, structure);
    canvas.fine_rect(110, foot - 2, 20, 1, Color::SHADOW);
    render_lines(
        canvas,
        EAVE + 4,
        &["TAP: OFF NOW", "ELSE: CANCEL"],
        Color::TEXT_DIM,
    );
    canvas.fine_rect(0, pavement, 240, 1, structure);
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
    // (an eighth of them) still lit; behind it, the night's rain, or a few
    // stars over it. Not in the dim look, whose far buildings would be
    // half-toned.
    if !dim {
        let night = street::Night::now();
        if night.rain {
            street::fine_rain(canvas, 168, 240, night.tick, Color::TRACK);
        } else {
            street::fine_stars(canvas, &OFF_STARS, None);
        }
        let lit = street::city_lights(night.hour) / 8;
        street::fine_skyline(canvas, 0, 240, 240, 3 * 16, lit);
    }
}

/// The stars over the city as the watch switches off, on a clear night.
const OFF_STARS: [(i32, i32); 5] = [(7, 58), (22, 62), (47, 57), (63, 61), (74, 56)];

// ==============================================================================
// Pairing
// ==============================================================================

/// The ticket machine's display, in the panel's pixels: its top, and its
/// height.
const DISPLAY_TOP: i32 = 72;
const DISPLAY_H: i32 = 66;
/// The passkey's LEDs: how far apart, and how big each is.
const LED_PITCH: i32 = 6;
const LED: i32 = 5;

/// The six-digit passkey, on a station ticket machine's dark display as
/// its LEDs show a fare, in the windows' warm yellow, the unlit LEDs
/// faint round them; big enough to read at a glance while typing it on
/// the phone. Under it, the machine's fare buttons, the coin slot and the
/// ticket slot. Still. In the dim look, the display and the digits.
pub(super) fn render_pairing(canvas: &mut Canvas, passkey: u32, dim: bool) {
    canvas.clear(BACKGROUND);
    street::title(canvas, "PAIRING", Color::ACCENT);
    canvas.text(4, 14, "ENTER ON PHONE:", Color::TEXT_DIM);

    // The machine's front, darker than the night, down to the bottom
    // edge, its top catching the light.
    if !dim {
        let top = DISPLAY_TOP - 7;
        canvas.fine_rect(3, top, 234, 240 - top, Color::SHADOW);
        canvas.fine_rect(3, top, 234, 1, Color::TEXT_FAINT);
    }
    // The display, sunk in a bezel (in the dim look, grey).
    canvas.fine_rect(6, DISPLAY_TOP, 228, DISPLAY_H, Color::TRACK);
    canvas.fine_rect(9, DISPLAY_TOP + 3, 222, DISPLAY_H - 6, BACKGROUND);

    // The digits, five LEDs across and seven down, a column of them
    // apart: each LED a dot with its corners rounded.
    let unlit = (!dim).then_some(Color::TRACK);
    let mut rest = passkey;
    for i in (0..6).rev() {
        let glyph = digit_glyph(u64::from(rest % 10));
        rest /= 10;
        let x = 15 + 6 * LED_PITCH * i;
        for (row, &bits) in glyph.iter().enumerate() {
            for column in 0..5 {
                let on = bits & (1 << (4 - column)) != 0;
                let Some(color) = (if on { Some(Color::HEART_SHINE) } else { unlit }) else {
                    continue;
                };
                let (dx, dy) = (
                    x + column * LED_PITCH,
                    DISPLAY_TOP + 12 + row as i32 * LED_PITCH,
                );
                let level = if on { 4 } else { 2 };
                canvas.fine_tone(dx, dy + 1, LED, LED - 2, level, color);
                canvas.fine_tone(dx + 1, dy, LED - 2, LED, level, color);
            }
        }
    }
    if dim {
        return;
    }

    // Under it, a row of fare buttons, each with its fare's dash lit; the
    // coin slot, a slit lit round it; and the slot the ticket comes out
    // of, a ticket in it.
    for i in 0..5 {
        let x = 15 + 42 * i;
        canvas.fine_rect(x, 157, 36, 18, Color::TRACK);
        canvas.fine_rect(x, 157, 36, 1, Color::TEXT_FAINT);
        canvas.fine_rect(x + 11, 165, 14, 2, Color::HEART_SHINE);
    }
    canvas.fine_rect(190, 190, 9, 22, Color::GOOD);
    canvas.fine_rect(193, 192, 3, 18, BACKGROUND);
    canvas.fine_rect(72, 201, 96, 9, BACKGROUND);
    canvas.fine_rect(72, 200, 96, 1, Color::TEXT_FAINT);
    canvas.fine_rect(102, 203, 36, 12, Color::TEXT_DIM);
    canvas.fine_rect(102, 211, 36, 2, Color::TEXT_FAINT);
}

// ==============================================================================
// Firmware update
// ==============================================================================

/// The line the train runs on, and the platform's edge in front of it,
/// in the panel's pixels.
const RAILS: i32 = 198;
const PLATFORM: i32 = 210;
/// The train: how tall, its roof's row, and how long a carriage.
const TRAIN_H: i32 = 34;
const TRAIN_TOP: i32 = RAILS - TRAIN_H - 2;
const CARRIAGE: i32 = 90;
/// The overhead wire the train draws its power from.
const WIRE: i32 = 150;

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

    // The train, its nose as far across as the update has come.
    let nose = 14 + i32::from(percent.min(100)) * 216 / 100;
    if dim {
        // The platform's edge, and the train's roof and nose, in grey.
        canvas.fine_rect(0, PLATFORM + 3, 240, 2, Color::TRACK);
        canvas.fine_rect(0, TRAIN_TOP, nose - 6, 2, Color::TRACK);
        street::fine_line(
            canvas,
            (nose - 6, TRAIN_TOP),
            (nose, TRAIN_TOP + 10),
            Color::TRACK,
        );
        canvas.fine_rect(nose, TRAIN_TOP + 10, 2, TRAIN_H - 10, Color::TRACK);
        return;
    }

    // The overhead wire, and the rails under the train: their heads
    // catching the light, the sleepers between.
    canvas.fine_rect(0, WIRE, 240, 1, Color::TEXT_FAINT);
    for x in (4..240).step_by(9) {
        canvas.fine_rect(x, RAILS + 2, 6, 2, Color::TRACK);
    }
    canvas.fine_rect(0, RAILS, 240, 2, Color::TEXT_FAINT);
    render_train(canvas, nose);

    // The platform, in front: its white edge, the yellow strip of
    // studded tiles along it, and its floor.
    canvas.fine_rect(0, PLATFORM, 240, 2, Color::TEXT_DIM);
    canvas.fine_rect(0, PLATFORM + 2, 240, 30, Color::TRACK);
    canvas.fine_rect(0, PLATFORM + 5, 240, 8, Color::HEART_SHINE);
    for x in (1..240).step_by(4) {
        for y in [PLATFORM + 6, PLATFORM + 10] {
            canvas.fine_rect(x + (y & 2), y, 2, 2, Color::SHADOW);
        }
    }
}

/// Where a carriage's windows are, from its rear end: each one's left
/// and width; its doors, between them, with a window each.
const WINDOWS: [(i32, i32); 3] = [(4, 8), (30, 30), (78, 8)];
const DOORS: [i32; 2] = [15, 63];

/// The train, coming in from the left, its nose's last column at `nose`:
/// silver carriages with a green band along them under lit windows, their
/// doors, the air conditioners on their roofs and their bogies under
/// them, the second carriage's pantograph up to the wire; and the cab,
/// its nose rounded, its windscreen dark, its headlight on.
fn render_train(canvas: &mut Canvas, nose: i32) {
    let top = TRAIN_TOP;
    let (body, band, window) = (Color::TEXT_DIM, Color::GOOD, Color::HEART_SHINE);
    let mut end = nose;
    let mut carriage = 0;
    while end > 0 {
        let start = end - CARRIAGE + 3;
        let cab = carriage == 0;
        // The body, a skirt of dark under it, and its roof; the cab's
        // nose sloping back from the windscreen, rounded at its top.
        let length = end - start + 1 - if cab { 10 } else { 0 };
        canvas.fine_rect(start, top, length, TRAIN_H - 4, body);
        canvas.fine_rect(start, top + TRAIN_H - 4, end - start + 1, 4, Color::TRACK);
        canvas.fine_rect(start, top, length, 2, Color::TEXT_FAINT);
        if cab {
            for row in 0..TRAIN_H - 4 {
                let w = 10 - (12 - row).max(0) * 10 / 12;
                canvas.fine_rect(end - 9, top + row, w, 1, body);
            }
            canvas.fine_rect(end - 14, top + 8, 10, 9, Color::SHADOW);
            canvas.fine_rect(end - 5, top + 22, 4, 3, Color::TEXT);
            street::fine_glow(canvas, end - 2, top + 16, 16, 16, Color::HEART_SHINE);
        }
        // The band, the windows over it (the cab's short of its nose), and
        // the doors.
        canvas.fine_rect(
            start,
            top + 20,
            end - start - if cab { 4 } else { 0 },
            3,
            band,
        );
        for (x, w) in WINDOWS {
            if !(cab && x > 60) {
                canvas.fine_rect(start + x, top + 6, w, 10, window);
            }
        }
        for x in DOORS {
            let x = start + x;
            canvas.fine_rect(x, top + 4, 11, TRAIN_H - 8, Color::TEXT_FAINT);
            canvas.fine_rect(x + 1, top + 5, 9, TRAIN_H - 10, body);
            canvas.fine_rect(x + 3, top + 7, 5, 8, window);
        }
        // On its roof, two air conditioners; under it, two bogies.
        for x in [12, 58] {
            canvas.fine_rect(start + x, top - 3, 18, 3, Color::TEXT_FAINT);
            canvas.fine_rect(start + x, TRAIN_TOP + TRAIN_H - 2, 18, 3, Color::SHADOW);
        }
        // The second carriage's pantograph, a diamond up to the wire.
        if carriage == 1 {
            let (x, mid) = (start + 45, (top + WIRE) / 2);
            for (from, to) in [
                ((x, top - 1), (x - 8, mid)),
                ((x - 8, mid), (x - 1, WIRE + 1)),
            ] {
                street::fine_line(canvas, from, to, Color::TEXT_FAINT);
                let mirror = |(px, py): (i32, i32)| (2 * x - px, py);
                street::fine_line(canvas, mirror(from), mirror(to), Color::TEXT_FAINT);
            }
            canvas.fine_rect(x - 8, WIRE + 1, 17, 1, Color::TEXT_DIM);
        }
        end = start - 3;
        carriage += 1;
    }
}
