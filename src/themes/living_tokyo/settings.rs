//! Living Tokyo's info and settings screens, and Home Assistant's key.

use super::layout::{SETTINGS_ROW, SETTINGS_TOP};
use super::{BACKGROUND, clock, street};
use crate::bthome::Identity;
use crate::canvas::{Canvas, Color, FINE};
use crate::font;
use crate::themes::helpers::{self, address, hex, hms, percent, volts};
use crate::themes::{RowKind, SettingsPage};
use crate::ui::{Fits, Status};

// ==============================================================================
// Info: a station's information board
// ==============================================================================

/// The board's first row of lettering, and the spacing between rows.
const INFO_TOP: i32 = 15;
const INFO_LINE: i32 = 9;
/// The moon (the battery), top right, as on the clock face.
const INFO_MOON: (i32, i32, i32) = (67, 1, 10);

/// The watch's state, for a look (and for bug reports), on a station's
/// information board: what it says lit in the board's amber beside its
/// painted labels. Two things on it live: the uptime ticks, and the
/// phone's own small sign, round its label, is lit while it's connected
/// and dark while it isn't. Over it, top right, the moon is the battery,
/// as on the clock face. The settings that used to be at its foot have a
/// screen of their own, before it.
pub(super) fn render_info(canvas: &mut Canvas, status: &Status) {
    canvas.clear(BACKGROUND);
    let bottom = INFO_TOP + INFO_LINE * 5 - 1;
    street::board(canvas, 0, INFO_TOP - 4, 80, bottom - INFO_TOP + 6, [44, 60]);
    // The seams between its rows, at the panel's resolution, dotted:
    // midway between a row's lettering and the next's, which keeps them
    // out of the uptime's canvas rows and their band (see `refine`), so
    // its ticking stays a coarse span.
    for i in 0..4 {
        let y = 3 * (INFO_TOP + INFO_LINE * i) + 21;
        canvas.fine_tone(6, y, FINE - 12, 1, 2, Color::TRACK);
    }
    // The version without its build metadata (`0.9.0-pr.12`); the commit
    // goes in the BUILD row.
    let (release, build) = crate::version::shown();
    let mut title = heapless::String::<24>::new();
    title.push_str("UR ").fits();
    title.push_str(release).fits();
    street::title(canvas, &title, Color::ACCENT);
    let (x, y, d) = INFO_MOON;
    clock::moon(canvas, x, y, d, status.battery);

    let battery = status.battery;
    let mut charge = percent(battery.percent());
    charge.push(' ').fits();
    charge.push_str(&volts(battery.millivolts())).fits();
    let uptime = hms(status.uptime_secs);
    // "PAIRED" rather than "CONNECTED": the phone only counts as here once
    // it has written over an authenticated link (see `ble`), which takes
    // its bond, so this says both that it's connected and that it's paired
    // (and bonded: ur always stores the keys it pairs with).
    let phone = if status.connected {
        "PAIRED"
    } else if let Some(why) = status.unpaired {
        why.title()
    } else {
        "WAITING"
    };
    // Which commit this build is from, for bug reports: "DEV" for a local
    // `cargo build`, which doesn't know.
    let build = match build {
        "" => "DEV",
        build => build,
    };
    let rows = [
        ("BATTERY", charge.as_str()),
        ("POWER", battery.status()),
        ("UPTIME", &uptime),
        ("PHONE", phone),
        ("BUILD", build),
    ];
    for (i, (label, value)) in rows.into_iter().enumerate() {
        let y = INFO_TOP + INFO_LINE * i as i32;
        if label == "PHONE" {
            // Its sign, lit green while the phone's here; dark, the tube
            // unlit, while it isn't.
            let tube = if status.connected {
                Color::GOOD
            } else {
                Color::TRACK
            };
            street::fine_sign(
                canvas,
                3 * 2,
                3 * (y - 2),
                3 * (font::small_text_width(label) + 5),
                3 * street::SIGN_H,
                tube,
            );
            let ink = if status.connected {
                Color::GOOD
            } else {
                Color::TEXT_FAINT
            };
            canvas.text(4, y, label, ink);
        } else {
            canvas.text(4, y, label, Color::TEXT_DIM);
        }
        let ink = if label == "PHONE" && !status.connected {
            Color::TEXT_FAINT
        } else {
            Color::HEART_SHINE
        };
        canvas.text_right(76, y, value, ink);
    }

    // Storage mode is a long press here, which nothing else would show.
    // TODO: tapping still buzzes (a motor test); drop that once alarms
    // exercise the motor.
    canvas.text_centered(40, 68, "HOLD: STORAGE MODE", Color::TEXT_FAINT);
}

// ==============================================================================
// The settings: menu tags on a rail
// ==============================================================================

// The settings as an izakaya's wooden menu tags, hanging from a rail
// under the title, one under another on cords: a tag a row, its label
// on the left and its value on the right, each a tap (screens of at most
// three rows, each 20 canvas pixels, ~7 mm, tall: a fingertip and some).
// At the panel's resolution, a tag is wood: a dark board warmed by the
// lamps' amber, a pixel in four, its top edge catching the light, a
// grain line along it, and an eyelet where each cord's tied on. A tag
// that can't change in this build hangs unlit, plain dark in a slate
// edge; and THEME's has the scheme's own neon along its top, so the tag
// shows what it picks. That's all that lives here: the rows have the
// screen. Dim, the tags in red outline, untoned (see `Palette::columns`
// on idle mode's checkerboards).
//
// Their lettering stays the small font's, on the canvas: small labels,
// as the plan for the city's lettering has them.

/// The rail the first tag hangs from, under the title, on the canvas's
/// grid.
const RAIL_Y: i32 = 11;
/// A tag inside its row: how far below the row's top it hangs, and how
/// tall it is.
const TAG_DROP: i32 = 3;
const TAG_H: i32 = 14;
/// Its cords' columns, in the panel's pixels; and its lettering's, on
/// the canvas's grid: the label from the left, the value right-aligned to
/// the right.
const CORDS: [i32; 2] = [25, 214];
const LABEL_X: i32 = 4;
const VALUE_RIGHT: i32 = 76;

/// Whether a settings row's value fits right of its label, with a gap.
fn row_fits(label: &str, value: &str) -> bool {
    LABEL_X + font::small_text_width(label) + 4 <= VALUE_RIGHT - font::small_text_width(value)
}

/// Which row a tap at `y` is on, of `count`: the title's strip above the
/// rows counts as the first's, a forgiving target at the screen's edge.
pub(super) fn row_at(y: i32, count: usize) -> Option<usize> {
    helpers::row_at(y.max(SETTINGS_TOP), SETTINGS_TOP, SETTINGS_ROW, count)
}

/// A settings screen: its title's sign, then each row's tag, its label
/// on the left and its value on the right. A value to step through is
/// bright; one that can't change in this build, or a screen to go to,
/// dim.
pub(super) fn render_settings(canvas: &mut Canvas, page: &SettingsPage, dim: bool) {
    canvas.clear(BACKGROUND);
    // The rail: a rod, lit along its top but in the dim look.
    let rail = 3 * RAIL_Y;
    canvas.fine_rect(0, rail, FINE, 3, Color::TEXT_FAINT);
    if !dim {
        canvas.fine_rect(0, rail, FINE, 1, Color::TEXT_DIM);
    }
    let mut above = rail + 3;
    for (i, row) in page.rows.iter().enumerate() {
        let top = SETTINGS_TOP + SETTINGS_ROW * i as i32 + TAG_DROP;
        let y = 3 * top;
        for x in CORDS {
            canvas.fine_rect(x, above, 1, y + 4 - above, Color::TEXT_FAINT);
        }
        let lit = row.kind != RowKind::Fixed;
        tag(canvas, y, lit, dim);
        if row.label == "THEME" && !dim {
            // The scheme's own neon along the tag's top edge, a colour a
            // stretch: what THEME picks, shown on it.
            const NEONS: [Color; 4] = [Color::REM, Color::DEEP, Color::GOOD, Color::HEART_SHINE];
            for (j, color) in NEONS.into_iter().enumerate() {
                canvas.fine_rect(9 + 57 * j as i32, y + 1, 54, 2, color);
            }
        }
        above = y + 3 * TAG_H;

        debug_assert!(
            row_fits(row.label, row.value),
            "{} is too long for {}",
            row.value,
            row.label
        );
        let y = top + (TAG_H - 5) / 2;
        let (label, value) = match row.kind {
            _ if !lit => (Color::TEXT_FAINT, Color::TEXT_DIM),
            RowKind::Choice => (Color::TEXT_DIM, Color::TEXT),
            RowKind::Fixed | RowKind::Link => (Color::TEXT_DIM, Color::TEXT_DIM),
        };
        canvas.text(LABEL_X, y, row.label, label);
        canvas.text_right(VALUE_RIGHT, y, row.value, value);
        // A caveat, faint, after the label: an asterisk, as on a
        // footnote, says it's about the value beside it, not one of its
        // own.
        if let Some(note) = row.note {
            canvas.text(
                LABEL_X + font::small_text_width(row.label) + 5,
                y,
                note,
                Color::TEXT_FAINT,
            );
        }
    }
    // Last, so the rail runs behind it.
    street::title(canvas, page.title, Color::ACCENT);
}

/// A wooden tag across the screen from the panel's row `y`, its corners
/// rounded: lit, a dark board warmed by the lamps' amber, its top edge
/// catching their light and its bottom in shade, a line of its grain
/// across under the lettering; unlit, plain dark in the far buildings'
/// slate; and dim, every tag a red outline round the dark.
fn tag(canvas: &mut Canvas, y: i32, lit: bool, dim: bool) {
    const X: i32 = 4;
    const W: i32 = 232;
    const H: i32 = 3 * TAG_H;
    // The edge's colour, then the board inside it, its corners cut.
    let edge = match (dim, lit) {
        (true, _) => Color::TEXT_FAINT,
        (false, true) => Color::SHADOW,
        (false, false) => Color::TRACK,
    };
    canvas.fine_rect(X + 2, y, W - 4, H, edge);
    canvas.fine_rect(X, y + 2, W, H - 4, edge);
    canvas.fine_rect(X + 1, y + 1, W - 2, H - 2, edge);
    canvas.fine_rect(X + 2, y + 1, W - 4, H - 2, Color::SHADOW);
    canvas.fine_rect(X + 1, y + 2, W - 2, H - 4, Color::SHADOW);
    if lit && !dim {
        canvas.fine_tone(X + 1, y + 1, W - 2, H - 2, 1, Color::HEART_SHINE);
        canvas.fine_tone(X + 3, y + 1, W - 6, 1, 2, Color::HEART_SHINE);
        canvas.fine_rect(X + 3, y + H - 2, W - 6, 1, BACKGROUND);
        canvas.fine_tone(X + 30, y + H - 7, W - 90, 1, 2, Color::HEART_SHINE);
    }
    // The eyelets the cords are tied through.
    for x in CORDS {
        canvas.fine_rect(x - 1, y + 3, 3, 3, Color::TEXT_FAINT);
        canvas.fine_pixel(x, y + 4, BACKGROUND);
    }
}

// ==============================================================================
// Home Assistant's key
// ==============================================================================

/// Home Assistant's key as a QR code, filling the screen (see
/// [`Canvas::qr`]), for a phone's camera: what it reads is the key as
/// text (32 hex digits, which Home Assistant takes in capitals too), to
/// paste into its BTHome integration. A tap shows it as text instead.
/// The code is left as it is, with nothing round it: it must scan.
pub(super) fn render_ha_qr(canvas: &mut Canvas, identity: Option<&Identity>) {
    canvas.clear(BACKGROUND);
    let Some(identity) = identity else {
        street::title(canvas, "BTHOME KEY", Color::ACCENT);
        canvas.text_centered(40, 36, "NO KEY YET", Color::TEXT_DIM);
        return;
    };
    canvas.qr(&identity.key);
}

/// Home Assistant's key, for typing into its BTHome integration: 32 hex
/// digits, big, in four rows of two groups of four, and the address it'll
/// find the watch at, for picking it out among what it discovers.
pub(super) fn render_ha_key(canvas: &mut Canvas, identity: Option<&Identity>) {
    canvas.clear(BACKGROUND);
    street::title(canvas, "BTHOME KEY", Color::ACCENT);
    let Some(identity) = identity else {
        canvas.text_centered(40, 36, "NO KEY YET", Color::TEXT_DIM);
        return;
    };
    for (row, bytes) in identity.key.chunks(4).enumerate() {
        let mut text = [b' '; 9];
        for (i, &byte) in bytes.iter().enumerate() {
            // A space between the two groups.
            let at = 2 * i + usize::from(i >= 2);
            text[at..at + 2].copy_from_slice(&hex(byte));
        }
        // Hex digits and spaces, so never empty (and no `expect`, whose
        // `Utf8Error` `Debug` would cost flash).
        let text = core::str::from_utf8(&text).unwrap_or_default();
        // 2×: 8 px a character, so 72 px (bar the last one's gap).
        canvas.text_scaled(5, 13 + 13 * row as i32, text, Color::TEXT, 2);
    }
    canvas.text_centered(40, 69, &address(&identity.address), Color::TEXT_DIM);
}
