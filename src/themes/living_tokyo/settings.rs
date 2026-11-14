//! Living Tokyo's info and settings screens, and Home Assistant's key.

use super::layout::{SETTINGS_ROW, SETTINGS_TOP};
use super::{BACKGROUND, clock, street};
use crate::bthome::Identity;
use crate::canvas::{Canvas, Color};
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
            street::sign(
                canvas,
                2,
                y - 2,
                font::small_text_width(label) + 5,
                street::SIGN_H,
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
// under the title, one under another on short cords: a tag a row, its
// label on the left and its value on the right, each a tap (screens of
// at most three rows, each 20 canvas pixels, ~7 mm, tall: a fingertip
// and some). A tag that can't change in this build hangs unlit; and
// THEME's is edged in the scheme's own neon, so the tag shows what it
// picks. That's all that lives here: the rows have the screen. Dim, the
// tags in outline.

/// The rail the first tag hangs from, under the title.
const RAIL_Y: i32 = 11;
/// A tag inside its row: how far below the row's top it hangs, and how
/// tall it is.
const TAG_DROP: i32 = 3;
const TAG_H: i32 = 14;
/// Its cords' columns, and its lettering's: the label from the left, the
/// value right-aligned to the right.
const CORDS: [i32; 2] = [8, 71];
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
    canvas.fill_rect(0, RAIL_Y, 80, 1, Color::TEXT_FAINT);
    let mut above = RAIL_Y + 1;
    for (i, row) in page.rows.iter().enumerate() {
        let top = SETTINGS_TOP + SETTINGS_ROW * i as i32 + TAG_DROP;
        for x in CORDS {
            canvas.fill_rect(x, above, 1, top - above, Color::TEXT_FAINT);
        }
        let lit = row.kind != RowKind::Fixed;
        tag(canvas, top, lit, dim);
        if row.label == "THEME" && !dim {
            // The scheme's own neon along the tag's top edge, a colour a
            // stretch: what THEME picks, shown on it.
            const NEONS: [Color; 4] = [Color::REM, Color::DEEP, Color::GOOD, Color::HEART_SHINE];
            for (j, color) in NEONS.into_iter().enumerate() {
                canvas.fill_rect(3 + 19 * j as i32, top, 18, 1, color);
            }
        }
        above = top + TAG_H;

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

/// A wooden tag across the screen from row `top`, its corners cut: the
/// slate of the far buildings, lit; the dark of the near ones, in the
/// slate's outline, unlit (and, dim, every tag so, as a half-tone under
/// letters would break them up).
fn tag(canvas: &mut Canvas, top: i32, lit: bool, dim: bool) {
    const X: i32 = 1;
    const W: i32 = 78;
    canvas.fill_rect(X, top, W, TAG_H, Color::TRACK);
    if !lit || dim {
        canvas.fill_rect(X + 1, top + 1, W - 2, TAG_H - 2, Color::SHADOW);
    }
    for (x, y) in [
        (X, top),
        (X + W - 1, top),
        (X, top + TAG_H - 1),
        (X + W - 1, top + TAG_H - 1),
    ] {
        canvas.pixel(x, y, BACKGROUND);
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
