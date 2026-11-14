//! Living Tokyo's button grid: a vending machine glowing on a dark
//! street, a lit slot a button.

use core::fmt::Write as _;

use super::{BACKGROUND, street};
use crate::buttons::{self, Press};
use crate::canvas::{Canvas, Color};
use crate::font;
use crate::themes::Grid;
use crate::ui::Fits;

// The grid is a drinks machine on a street corner at night, as Japan has
// on every one: a dark cabinet whose lit shelves glow, a drink on show
// over each button. A slot (its part of a shelf, and its button in the
// dark bar under the shelf) is a grid button: three across, on two
// shelves (six) or three (nine, two layers). Under the shelves are the
// machine's readout and its tray. Round it, the dark street, and the
// night's weather, the one thing about it that changes: rain falling
// behind it and its light broken up on the wet street, or a few stars.
//
// A touch hits the screen in thirds across, and halves (six) or thirds
// (nine) down (see `buttons::at`): each slot sits wholly inside its own
// area (checked as it's drawn), so a finger on the drink, its button or
// the gap round them presses the slot it's nearest. With six, the
// machine stands lower, with more night over it and a strip of street
// in front; with nine, it's seen closer.
//
//        six                     nine
//   ┌──────────────┐        ┌──────────────┐
//   │              │        │ ┌──────────┐ │
//   │ ┌──────────┐ │        │ │ ▓▓ ▓▓ ▓▓ │ │ thirds
//   │ │ ▓▓ ▓▓ ▓▓ │ │ halves │ │ ▓▓ ▓▓ ▓▓ │ │
//   │ │ ▓▓ ▓▓ ▓▓ │ │        │ │ ▓▓ ▓▓ ▓▓ │ │
//   │ │ tray  LED│ │        │ │ tray  LED│ │
//   │ └──────────┘ │        └──────────────┘
//   │ street       │
//   └──────────────┘
//
// What a slot shows:
//
// - pressed (held down, or a sequence's first while its second waits),
//   it flashes: its part of the shelf floods with warm light, its button
//   lit;
// - waiting (to see whether more follows), it blinks: its button lit,
//   and a ring of light round it on every other second;
// - sent, its part of the shelf lights in the "good" colour with the
//   code on it, big, the readout says how it was pressed, and a can
//   drops into the tray; one that went nowhere lights in the "bad"
//   colour, and nothing drops.
//
// In the dim look (`Grid::dim`), the machine is unlit: its shelves'
// outlines alone, still, and only what's pressed or sent lit, in white.

/// How a slot looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Up,
    /// Held down: a sequence's first button, while its second waits; or
    /// a hold's, while the finger's still on it.
    Down,
    /// Waiting to see whether more follows.
    Lit,
    /// Just sent, with its code, and whether it went anywhere.
    Sent(Press, bool),
}

/// Where a layout's machine stands. The columns are the same in both.
struct Machine {
    /// Its roof's row.
    top: i32,
    /// The first shelf's top, and the shelves' pitch.
    first: i32,
    pitch: i32,
    /// How tall a shelf's lit display is; its button bar is under it.
    shelf: i32,
    /// How many shelves: rows of slots.
    rows: i32,
    /// The top of the readout and the tray, under the shelves.
    panel: i32,
    /// The row under the machine: the street from here down (off the
    /// screen with nine).
    base: i32,
}

/// Six: two shelves, one in each half, and a strip of street in front.
const SIX: Machine = Machine {
    top: 15,
    first: 18,
    pitch: 24,
    shelf: 16,
    rows: 2,
    panel: 63,
    base: 72,
};

/// Nine: three shelves, one in each third, the machine seen closer.
const NINE: Machine = Machine {
    top: 6,
    first: 9,
    pitch: 23,
    shelf: 14,
    rows: 3,
    panel: 73,
    base: 80,
};

/// How tall a shelf's button bar is, under its display: a dark strip,
/// its buttons lit in it.
const BAR: i32 = 3;

/// The machine's front, left to right, a margin of street either side.
const BODY_X: i32 = 3;
const BODY_W: i32 = 74;
/// The shelves' displays, left to right.
const DISPLAY_X: i32 = 6;
const DISPLAY_W: i32 = 68;
/// A slot's part of its shelf, left edge and width, one a column: in
/// the middle of the screen's thirds, and well inside them.
const SEGMENTS: [(i32, i32); 3] = [(6, 19), (30, 20), (55, 19)];
/// A slot's drink and its button, under it: 6 wide, from these columns.
const BUTTON_X: [i32; 3] = [12, 37, 62];
const BUTTON_W: i32 = 6;
/// The tray, bottom left, and the readout, right of it.
const TRAY_X: i32 = 6;
const TRAY_W: i32 = 30;
const READOUT_X: i32 = 40;
const READOUT_W: i32 = 34;

/// The drinks on show, one a slot: an even spread of the neons, no two
/// alike side by side or one over another. Odd slots are bottles, even
/// ones cans, so the shelves alternate as a machine's do.
const DRINKS: [Color; 9] = [
    Color::HEART,
    Color::DEEP,
    Color::HEART_SHINE,
    Color::GOOD,
    Color::REM,
    Color::ACCENT,
    Color::DEEP,
    Color::HEART_SHINE,
    Color::HEART,
];

/// A few stars over the machine, on a clear night, where there's sky for
/// them: those under its roof are drawn over.
const GRID_STARS: [(i32, i32); 5] = [(9, 3), (70, 2), (49, 9), (26, 11), (1, 40)];

/// The grid.
pub(super) fn render_buttons(canvas: &mut Canvas, grid: &Grid) {
    let Grid {
        layout,
        grid,
        sent,
        dim,
    } = *grid;
    let machine = match layout {
        buttons::Layout::Six => &SIX,
        buttons::Layout::TwoLayers => &NINE,
    };
    let now = street::local_now();
    let rain = now.is_some_and(|second| street::rainy(street::evening(second)));
    let tick = now.map_or(0, |second| second as u64);

    canvas.clear(BACKGROUND);
    if !dim {
        // The cabinet, darker than the night, its edge catching the
        // street's light; then the weather, in front of it but behind
        // its shelves.
        let (top, bottom) = (machine.top, machine.base.min(80));
        canvas.fill_rect(BODY_X, top, BODY_W, bottom - top, Color::SHADOW);
        // Its edge, down to the street (with nine, off the screen).
        let edge = (machine.base + 1).min(80) - top;
        canvas.fill_rect(BODY_X, top, BODY_W, 1, Color::TRACK);
        canvas.fill_rect(BODY_X, top, 1, edge, Color::TRACK);
        canvas.fill_rect(BODY_X + BODY_W - 1, top, 1, edge, Color::TRACK);
        if machine.base < 80 {
            canvas.fill_rect(BODY_X, machine.base, BODY_W, 1, Color::TRACK);
        }
        render_weather(canvas, machine, rain, tick);
    }
    render_shelves(canvas, machine, dim);

    // The slots, and what each is doing.
    let (first, second) = grid.waiting();
    let held = grid.held();
    let mut dropped = None;
    for row in 0..machine.rows {
        let y = machine.first + machine.pitch * row;
        for (column, &(x, w)) in SEGMENTS.iter().enumerate() {
            let button = (row * 3) as u8 + column as u8 + 1;
            let key = match (sent, first, second) {
                (Some((press, ok)), ..) if press.last() == button => Key::Sent(press, ok),
                // A hold, once what it sent has shown: pressed for as long
                // as the finger's on it, up again at the lift.
                _ if held == Some(button) => Key::Down,
                (_, _, Some(second)) if second == button => Key::Lit,
                (_, Some(first), Some(_)) if first == button => Key::Down,
                (_, Some(first), None) if first == button => Key::Lit,
                _ => Key::Up,
            };
            if let Key::Sent(_, true) = key {
                dropped = Some(DRINKS[usize::from(button - 1)]);
            }
            debug_assert!(
                [(x, y), (x + w - 1, y + machine.shelf + BAR - 1)]
                    .iter()
                    .all(|&(x, y)| buttons::at(layout, x, y) == button),
                "a slot strays out of its button's tap area"
            );
            render_slot(canvas, machine, y, column, button, key, dim, tick);
        }
    }
    render_panel(canvas, machine, sent, dropped, dim);
}

/// The night round the machine, its one changing part. On a rainy night,
/// rain falling past it, and its shelves' light broken up in the wet
/// street in front (where it shows). On a clear one, a few stars, one
/// twinkling, and a moth flitting round the machine's light over its
/// roof, a step of its circuit a second.
fn render_weather(canvas: &mut Canvas, machine: &Machine, rain: bool, tick: u64) {
    /// The moth's circuit, round a point over the roof.
    const FLIGHT: [(i32, i32); 6] = [(0, 0), (4, -1), (7, 1), (5, 2), (1, 2), (-2, 1)];
    if rain {
        street::fine_rain(canvas, 0, 240, tick, Color::TEXT_FAINT);
        let street = machine.base;
        if street < 80 {
            for &(x, w) in &SEGMENTS {
                street::reflection(canvas, x, w, street + 1, 3, Color::TEXT_DIM);
            }
        }
    } else {
        street::fine_stars(canvas, &GRID_STARS, Some(tick));
        let (dx, dy) = FLIGHT[tick as usize % FLIGHT.len()];
        let (x, y) = (22 + dx, machine.top - 4 + dy);
        // Its wings, up on even seconds and level on odd ones, over its
        // body.
        let wings = if tick.is_multiple_of(2) { -1 } else { 0 };
        canvas.pixel(x - 1, y + wings, Color::TEXT_DIM);
        canvas.pixel(x + 1, y + wings, Color::TEXT_DIM);
        canvas.pixel(x, y, Color::TEXT);
    }
}

/// The shelves, lit, without their slots; in the dim look, unlit, their
/// outlines alone.
fn render_shelves(canvas: &mut Canvas, machine: &Machine, dim: bool) {
    for row in 0..machine.rows {
        let y = machine.first + machine.pitch * row;
        let color = if dim { Color::TRACK } else { Color::TEXT };
        if dim {
            outline(canvas, DISPLAY_X, y, DISPLAY_W, machine.shelf, color);
        } else {
            canvas.fill_rect(DISPLAY_X, y, DISPLAY_W, machine.shelf, color);
            // The shelf the drinks stand on.
            let lip = y + machine.shelf - 1;
            canvas.fill_rect(DISPLAY_X, lip, DISPLAY_W, 1, Color::TEXT_FAINT);
        }
    }
}

/// A slot: its part of the shelf at `y`, in `column`, a drink standing on
/// show there, and its button in the bar under it, as `key` has them.
#[allow(clippy::too_many_arguments)]
fn render_slot(
    canvas: &mut Canvas,
    machine: &Machine,
    y: i32,
    column: usize,
    button: u8,
    key: Key,
    dim: bool,
    tick: u64,
) {
    let (x, w) = SEGMENTS[column];
    // The display's rows, less the shelf's lip.
    let h = machine.shelf - 1;
    let bx = BUTTON_X[column];
    let by = y + machine.shelf + 1;
    // In the dim look, what's lit is white, and what failed red.
    let lit = |color: Color| if dim { Color::TEXT } else { color };
    let (lamp, ring) = match key {
        Key::Up => (None, None),
        Key::Down => (Some(lit(Color::HEART_SHINE)), None),
        // A waiting slot's ring blinks; in the dim look, it holds still.
        Key::Lit => {
            let on = dim || tick.is_multiple_of(2);
            (
                Some(lit(Color::HEART_SHINE)),
                on.then(|| lit(Color::HEART_SHINE)),
            )
        }
        Key::Sent(_, true) => (Some(lit(Color::GOOD)), None),
        Key::Sent(_, false) => (Some(Color::BAD), None),
    };

    match key {
        // Sent: its part of the shelf lit in the result's colour, the code
        // on it big, in the dark (in the dim look, white on black, ringed).
        Key::Sent(press, ok) => {
            let color = if ok { lit(Color::GOOD) } else { Color::BAD };
            let ink = if dim {
                outline(canvas, x, y, w, machine.shelf, color);
                color
            } else {
                canvas.fill_rect(x, y, w, h, color);
                BACKGROUND
            };
            let mut code = heapless::String::<2>::new();
            write!(code, "{}", press.code).fits();
            let width = font::small_text_width(&code) * 2;
            canvas.text_scaled(x + (w - width) / 2, y + (h - 10) / 2, &code, ink, 2);
        }
        // Pressed: flooded with warm light, the drink dark against it (in
        // the dim look, ringed in white).
        Key::Down if dim => outline(canvas, x, y, w, machine.shelf, Color::TEXT),
        _ if dim => {}
        _ => {
            if key == Key::Down {
                canvas.fill_rect(x, y, w, h, Color::HEART_SHINE);
            }
            if let Some(ring) = ring {
                outline(canvas, x, y, w, h, ring);
            }
            let drink = DRINKS[usize::from(button - 1)];
            let foot = y + h;
            if button % 2 == 1 {
                bottle(canvas, bx, foot - 12, drink);
            } else {
                can(canvas, bx, foot - 8, drink);
            }
        }
    }
    if dim && ring.is_some() {
        outline(canvas, x, y, w, machine.shelf, Color::TEXT);
    }

    // The button: lit when pressed or sent; else dark, its top edge
    // catching the shelf's light (in the dim look, nothing).
    match lamp {
        Some(lamp) => canvas.fill_rect(bx, by, BUTTON_W, 2, lamp),
        None if !dim => canvas.fill_rect(bx, by, BUTTON_W, 1, Color::TEXT_FAINT),
        None => {}
    }
}

/// A can standing, 6 wide and 8 tall from (`x`, `y`): its lid, and its
/// body in `color` with a glint down it.
fn can(canvas: &mut Canvas, x: i32, y: i32, color: Color) {
    canvas.fill_rect(x + 1, y, 4, 1, Color::TEXT_FAINT);
    canvas.fill_rect(x, y + 1, 6, 7, color);
    canvas.fill_rect(x + 1, y + 2, 1, 4, Color::TEXT);
}

/// A bottle standing, 6 wide and 12 tall from (`x`, `y`): its cap in
/// `color`, its clear neck, and its body in `color`.
fn bottle(canvas: &mut Canvas, x: i32, y: i32, color: Color) {
    canvas.fill_rect(x + 2, y, 2, 1, color);
    canvas.fill_rect(x + 2, y + 1, 2, 2, Color::TEXT_FAINT);
    canvas.fill_rect(x + 1, y + 3, 4, 1, Color::TEXT_FAINT);
    canvas.fill_rect(x, y + 4, 6, 8, color);
    canvas.fill_rect(x + 1, y + 5, 1, 4, Color::TEXT);
}

/// The tray, left, and the readout, right, under the shelves: the can a
/// sent press dropped (`dropped`, in its drink's colour), and how it was
/// pressed, as a machine's readout counts the coins put in.
fn render_panel(
    canvas: &mut Canvas,
    machine: &Machine,
    sent: Option<(Press, bool)>,
    dropped: Option<Color>,
    dim: bool,
) {
    let y = machine.panel;
    let h = 7.min(80 - y);
    if !dim {
        // A dark slot under a flap, and a dark panel: both darker than the
        // cabinet's edge, a pixel of it round them.
        canvas.fill_rect(TRAY_X, y, TRAY_W, h, BACKGROUND);
        canvas.fill_rect(TRAY_X, y, TRAY_W, 1, Color::TRACK);
        canvas.fill_rect(READOUT_X, y, READOUT_W, h, BACKGROUND);
    }
    // The can, lying at the tray's foot, its lid to the right, and over
    // it the streaks of its fall. Not in the dim look, whose tray is
    // unlit: the readout says it went.
    if let Some(color) = dropped.filter(|_| !dim) {
        let (cx, cy) = (TRAY_X + TRAY_W - 14, y + h - 4);
        canvas.fill_rect(cx, cy, 8, 4, color);
        canvas.fill_rect(cx + 7, cy, 1, 4, Color::TEXT_FAINT);
        canvas.fill_rect(cx + 1, cy + 1, 5, 1, Color::TEXT);
        for sx in [cx + 1, cx + 5] {
            canvas.fill_rect(sx, y + 1, 1, cy - y - 1, Color::TEXT_DIM);
        }
    }
    if let Some((press, ok)) = sent {
        let color = match (ok, dim) {
            (false, _) => Color::BAD,
            (true, true) => Color::TEXT,
            (true, false) => Color::GOOD,
        };
        let how = match press.gesture {
            buttons::Gesture::Tap => "TAP",
            buttons::Gesture::DoubleTap => "DOUBLE",
            // Never shown: a release leaves the hold's on the screen.
            buttons::Gesture::Hold | buttons::Gesture::Release => "HOLD",
            buttons::Gesture::LongPress => "LONG",
        };
        canvas.text_centered(READOUT_X + READOUT_W / 2, y + 1, how, color);
    }
}

/// A rectangle's outline, a pixel thick.
fn outline(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, color: Color) {
    canvas.fill_rect(x, y, w, 1, color);
    canvas.fill_rect(x, y + h - 1, w, 1, color);
    canvas.fill_rect(x, y, 1, h, color);
    canvas.fill_rect(x + w - 1, y, 1, h, color);
}
