//! Living Tokyo's button grid: a vending machine glowing on a dark
//! street, a lit slot a button.

use core::fmt::Write as _;

use super::{BACKGROUND, street};
use crate::buttons::{self, Press};
use crate::canvas::{Canvas, Color};
use crate::font;
use crate::font::fine::Style;
use crate::themes::Grid;
use crate::ui::Fits;

// The grid is a drinks machine on a street corner at night, as Japan has
// on every one: a dark cabinet whose shelves glow behind its glass, a
// drink on show over each button. A slot (its part of a shelf, and its
// button in the dark bar under the shelf) is a grid button: three
// across, on two shelves (six) or three (nine, two layers). Under the
// shelves are the machine's readout, its coin slot and its tray. Round
// it, the dark street and the night: the dawn's glow over its roof, and
// the weather, rain falling over it and its light broken up on the wet
// street, or a few stars and a moth at its light.
//
// It's drawn at the panel's resolution (see `Canvas::fine_rect`), where
// a drink can be a can or a bottle with a label, a glint and a cap, and
// the shelves can glow in tones rather than glare in the text's white.
// The machine is still: what changes each second is the sky over it
// (the rain, a star, the moth) and a waiting slot's ring, so few of its
// fine rows are sent again (see `Canvas::refine`).
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
// outlines alone, on the canvas's grid, still, and only what's pressed
// or sent lit, in white.

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

/// Where a layout's machine stands, in the canvas's pixels (as
/// `buttons::at` has the taps). The columns are the same in both.
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
/// The tray, bottom left, and the readout, right of it, with the coin
/// slot at its right end.
const TRAY_X: i32 = 6;
const TRAY_W: i32 = 30;
const READOUT_X: i32 = 40;
const READOUT_W: i32 = 28;
const COINS_W: i32 = 5;

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
/// them: those under its roof are drawn over. Fewer towards dawn: the
/// first few of them.
const GRID_STARS: [(i32, i32); 5] = [(9, 3), (70, 2), (49, 9), (26, 11), (1, 40)];

/// A sent slot's code, big, in the city's lettering, dark on its lit
/// part of the shelf.
const CODE: Style = Style {
    cap: 20,
    stroke: 56,
    caps: true,
};

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
    let night = street::Night::now();
    // In the small hours, the machine saves power, as many do: its tubes
    // turned down.
    let saving = (1..5).contains(&night.hour);

    canvas.clear(BACKGROUND);
    if !dim {
        render_night(canvas, machine, night);
        render_cabinet(canvas, machine);
    }

    // The slots, and what each is doing.
    let (first, second) = grid.waiting();
    let held = grid.held();
    let mut dropped = None;
    for row in 0..machine.rows {
        let y = machine.first + machine.pitch * row;
        render_shelf(canvas, machine, y, dim, saving);
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
            render_slot(canvas, machine, y, column, button, key, dim, night.tick);
        }
    }
    render_panel(canvas, machine, sent, dropped, dim);
}

/// The night round the machine: what changes with the hour and the
/// weather. Over its roof, the dawn's glow from 06:00, two levels behind
/// the other screens' (see `street::daylight`): its roof is no horizon,
/// and its sky so low that the glow would fill it. Then on a rainy
/// night, rain falling over it, and its shelves' light broken up in the
/// wet street (where it shows); on a clear one, a few stars (fewer
/// towards dawn), one twinkling, the shelves' light pooled on the
/// street, and until the sky lightens, a moth flitting round the
/// machine's top, a step of its circuit a second.
///
/// The rain keeps to the sky, which is few rows: a fine row that changes
/// is sent whole (see `Canvas::refine`), and rain down the street's
/// narrow margins either side would change every row.
fn render_night(canvas: &mut Canvas, machine: &Machine, night: street::Night) {
    /// The moth's circuit, in the panel's pixels, round a point over
    /// the roof's left end.
    const FLIGHT: [(i32, i32); 6] = [(0, 0), (12, -3), (21, 3), (15, 6), (3, 6), (-6, 3)];
    let (top, base) = (3 * machine.top, 3 * machine.base);
    let daylight = street::daylight(night.hour);
    street::fine_horizon(canvas, 0, 240, top, daylight - 2);
    let (left, right) = (3 * BODY_X, 3 * (BODY_X + BODY_W));
    if night.rain {
        street::fine_rain(canvas, 0, top, night.tick, Color::TEXT_FAINT);
        for &(x, w) in &SEGMENTS {
            street::fine_reflection(canvas, 3 * x, 3 * w, base + 4, 5, Color::TEXT_FAINT);
        }
    } else {
        let stars = (GRID_STARS.len() as i32 - daylight).max(1) as usize;
        street::fine_stars(canvas, &GRID_STARS[..stars], Some(night.tick));
        // Fanning out from its foot, fading.
        for row in 0..5 {
            let spread = 3 + 4 * row;
            let level = if row < 1 { 2 } else { 1 };
            let (x, w) = (left - spread, right - left + 2 * spread);
            canvas.fine_tone(x, base + 1 + row, w, 1, level, Color::TEXT_DIM);
        }
        if daylight < 3 {
            // Its wings either side of its body, their tips up on even
            // seconds and down on odd ones.
            let (dx, dy) = FLIGHT[night.tick as usize % FLIGHT.len()];
            let (x, y) = (60 + dx, top - 12 + dy);
            let tips = if night.tick.is_multiple_of(2) { -2 } else { 1 };
            for (dx, dy) in [(-3, 0), (-5, tips), (1, 0), (3, tips)] {
                canvas.fine_rect(x + dx, y + dy, 3, 1, Color::TEXT_DIM);
            }
            canvas.fine_rect(x, y - 1, 1, 4, Color::TEXT);
        }
    }
    canvas.fine_rect(0, base, 240, 1, Color::TRACK);
}

/// The cabinet, darker than the night: its edges catching the street's
/// light, a lit strip along its top where its maker's name would be, and
/// a plinth along its foot.
fn render_cabinet(canvas: &mut Canvas, machine: &Machine) {
    let (x, w) = (3 * BODY_X, 3 * BODY_W);
    let (top, bottom) = (3 * machine.top, 3 * machine.base.min(80));
    canvas.fine_rect(x, top, w, bottom - top, Color::SHADOW);
    canvas.fine_rect(x, top, w, 1, Color::TEXT_FAINT);
    canvas.fine_rect(x, top + 1, 1, bottom - top, Color::TRACK);
    canvas.fine_rect(x + w - 1, top + 1, 1, bottom - top, Color::TRACK);
    // The strip, a tone of the watch's own blue, glowing under the roof.
    let strip = 3 * (machine.first - machine.top) - 7;
    canvas.fine_tone(x + 9, top + 3, w - 18, strip, 1, Color::ACCENT);
    canvas.fine_rect(x + 9, top + 3, w - 18, 1, Color::ACCENT);
    if machine.base < 80 {
        canvas.fine_rect(x, bottom - 5, w, 5, Color::TRACK);
    }
}

/// A shelf's case, from the canvas's row `y`: behind its glass in a
/// frame, lit from a tube along its top, the light fading down its back
/// to the shelf the drinks stand on, less of it while `saving` power; in
/// the dim look, its outline alone, on the canvas's grid.
fn render_shelf(canvas: &mut Canvas, machine: &Machine, y: i32, dim: bool, saving: bool) {
    if dim {
        outline(canvas, DISPLAY_X, y, DISPLAY_W, machine.shelf, Color::TRACK);
        return;
    }
    let (x, w) = (3 * DISPLAY_X, 3 * DISPLAY_W);
    let (y, h) = (3 * y, 3 * machine.shelf);
    canvas.fine_rect(x - 3, y - 3, w + 6, h + 5, Color::TEXT_FAINT);
    canvas.fine_rect(x - 2, y - 2, w + 4, h + 3, Color::TRACK);
    // The tube's light, strongest under it and gone by the shelf.
    let tube = if saving {
        Color::TEXT_FAINT
    } else {
        canvas.fine_tone(x, y, w, 2 * h / 3, 1, Color::TEXT_DIM);
        Color::TEXT_DIM
    };
    canvas.fine_tone(x, y, w, h / 4, 2, tube);
    canvas.fine_rect(x, y, w, 2, tube);
    // The shelf, its lip catching the light.
    canvas.fine_rect(x, y + h - 3, w, 1, Color::TEXT_FAINT);
    canvas.fine_rect(x, y + h - 2, w, 2, Color::SHADOW);
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

    // Its part of the shelf, in the panel's pixels: down to the shelf the
    // drinks stand on.
    let (fx, fw) = (3 * x, 3 * w);
    let (fy, fh) = (3 * y, 3 * machine.shelf - 3);
    match key {
        // Sent: its part of the shelf lit in the result's colour, the code
        // on it big, in the dark (in the dim look, in its colour, ringed).
        Key::Sent(press, ok) => {
            let color = if ok { lit(Color::GOOD) } else { Color::BAD };
            let ink = if dim {
                outline(canvas, x, y, w, machine.shelf, color);
                color
            } else {
                canvas.fine_rect(fx, fy, fw, fh, color);
                Color::SHADOW
            };
            let mut code = heapless::String::<2>::new();
            write!(code, "{}", press.code).fits();
            let width = font::Face::Fine(CODE).width(&code);
            let (cx, cy) = (fx + (fw - width) / 2, fy + (fh - CODE.height()) / 2);
            canvas.fine_type(cx, cy, &code, CODE, ink, None);
        }
        // Pressed: flooded with warm light, round its drink (in the dim
        // look, ringed in white).
        Key::Down if dim => outline(canvas, x, y, w, machine.shelf, Color::TEXT),
        _ if dim => {}
        _ => {
            if key == Key::Down {
                canvas.fine_tone(fx, fy, fw, fh, 3, Color::HEART_SHINE);
            }
            if let Some(ring) = ring {
                let sides = [
                    (fx - 2, fy - 2, fw + 4, 2),
                    (fx - 2, fy + fh, fw + 4, 2),
                    (fx - 2, fy, 2, fh),
                    (fx + fw, fy, 2, fh),
                ];
                for (x, y, w, h) in sides {
                    canvas.fine_rect(x, y, w, h, ring);
                }
            }
            let shape = if button % 2 == 1 { &BOTTLE } else { &CAN };
            let drink = DRINKS[usize::from(button - 1)];
            let middle = 3 * BUTTON_X[column] + 3 * BUTTON_W / 2;
            render_drink(canvas, shape, middle, fy + fh, drink, false);
        }
    }
    if dim && ring.is_some() {
        outline(canvas, x, y, w, machine.shelf, Color::TEXT);
    }

    // The button, in the bar under the shelf, rounded: lit when pressed
    // or sent, a haze round it; else dark, its top catching the shelf's
    // light, and a small red light in it, as a machine's are lit while
    // there's a drink to be had (in the dim look, nothing unless lit, and
    // then on the canvas's grid).
    let (bx, by) = (3 * BUTTON_X[column], 3 * (y + machine.shelf) + 1);
    let bw = 3 * BUTTON_W;
    match lamp {
        Some(lamp) if dim => {
            canvas.fill_rect(BUTTON_X[column], y + machine.shelf + 1, BUTTON_W, 2, lamp)
        }
        Some(lamp) => {
            canvas.fine_tone(bx - 2, by - 1, bw + 4, 8, 2, lamp);
            canvas.fine_rect(bx + 1, by, bw - 2, 6, lamp);
            canvas.fine_rect(bx, by + 1, bw, 4, lamp);
        }
        None if !dim => {
            canvas.fine_rect(bx + 1, by, bw - 2, 6, Color::TRACK);
            canvas.fine_rect(bx, by + 1, bw, 4, Color::TRACK);
            canvas.fine_rect(bx + 1, by, bw - 2, 1, Color::TEXT_FAINT);
            canvas.fine_rect(bx + bw / 2 - 2, by + 3, 4, 1, Color::HEART);
        }
        None => {}
    }
}

/// How a part of a [`Drink`] is painted.
#[derive(Clone, Copy)]
enum Paint {
    /// The drink's colour, solid: a can's body, a bottle's cap and label.
    Drink,
    /// A tone of it: the drink seen through a bottle's clear plastic.
    Through,
    /// The text's white: a label's band.
    White,
    /// The metal of a can's lid and rims.
    Metal,
}

/// A drink, standing, top to bottom: each part's first row, its rows,
/// its half width and its paint; its body's half width, and the rows its
/// glint and shade run down it.
struct Drink {
    height: i32,
    parts: &'static [(i8, i8, i8, Paint)],
    half: i32,
    shine: (i32, i32),
}

/// A can, 20 by 28: its lid and rim, its body, and a white band round it
/// with the maker's mark.
const CAN: Drink = Drink {
    height: 28,
    parts: &[
        (0, 1, 6, Paint::Metal),
        (1, 2, 9, Paint::Metal),
        (3, 23, 10, Paint::Drink),
        (10, 7, 10, Paint::White),
        (11, 5, 3, Paint::Drink),
        (26, 2, 9, Paint::Metal),
    ],
    half: 10,
    shine: (3, 26),
};

/// The can a sent press drops, smaller, as it lies further back in the
/// tray: 16 by 22.
const DROPPED: Drink = Drink {
    height: 22,
    parts: &[
        (0, 1, 5, Paint::Metal),
        (1, 2, 7, Paint::Metal),
        (3, 17, 8, Paint::Drink),
        (8, 5, 8, Paint::White),
        (9, 3, 2, Paint::Drink),
        (20, 2, 7, Paint::Metal),
    ],
    half: 8,
    shine: (3, 20),
};

/// A bottle, 18 by 36: its cap, the drink through its neck and
/// shoulders, and its label, a white band across it.
const BOTTLE: Drink = Drink {
    height: 36,
    parts: &[
        (0, 4, 3, Paint::Drink),
        (4, 1, 4, Paint::Metal),
        (5, 3, 3, Paint::Through),
        (8, 1, 5, Paint::Through),
        (9, 1, 6, Paint::Through),
        (10, 1, 7, Paint::Through),
        (11, 1, 8, Paint::Through),
        (12, 23, 9, Paint::Through),
        (17, 11, 9, Paint::Drink),
        (20, 3, 9, Paint::White),
        (35, 1, 8, Paint::Through),
    ],
    half: 9,
    shine: (9, 35),
};

/// A drink, `shape`, in `color`, in the panel's pixels: standing with
/// its foot on row `foot`, centred on column `x`; or `lying` in the tray,
/// its foot at column `x` and its top to the right, centred on row
/// `foot`. Its glint down its left (its top, lying) and its shade down
/// its right, over whatever it's painted.
fn render_drink(canvas: &mut Canvas, shape: &Drink, x: i32, foot: i32, color: Color, lying: bool) {
    let height = shape.height;
    // Rows `from` to `from + rows` of it, `across` from its middle to
    // `across + w`.
    let mut part = |from: i32, rows: i32, across: i32, w: i32, level: u8, color: Color| {
        let (x, y, w, h) = if lying {
            (x + height - from - rows, foot + across, rows, w)
        } else {
            (x + across, foot - height + from, w, rows)
        };
        canvas.fine_tone(x, y, w, h, level, color);
    };
    for &(from, rows, half, paint) in shape.parts {
        let (level, color) = match paint {
            Paint::Drink => (4, color),
            Paint::Through => (3, color),
            Paint::White => (4, Color::TEXT),
            Paint::Metal => (4, Color::TEXT_DIM),
        };
        let (from, rows, half) = (i32::from(from), i32::from(rows), i32::from(half));
        part(from, rows, -half, 2 * half, level, color);
    }
    let ((from, to), half) = (shape.shine, shape.half);
    part(from, to - from, 3 - half, 1, 3, Color::TEXT);
    part(from, to - from, half - 4, 4, 2, Color::SHADOW);
}

/// The tray, left, and the readout and coin slot, right, under the
/// shelves: the can a sent press dropped (`dropped`, in its drink's
/// colour), and how it was pressed, as a machine's readout counts the
/// coins put in.
fn render_panel(
    canvas: &mut Canvas,
    machine: &Machine,
    sent: Option<(Press, bool)>,
    dropped: Option<Color>,
    dim: bool,
) {
    let y = machine.panel;
    let (fy, fh) = (3 * y, 21.min(240 - 3 * y));
    if !dim {
        // The tray: a dark opening in a frame, its flap swung in at the
        // top; the readout, a dark window; the coin slot, a slit in a
        // plate, lit round it.
        let panels = [
            (TRAY_X, TRAY_W),
            (READOUT_X, READOUT_W),
            (READOUT_X + READOUT_W + 1, COINS_W),
        ];
        for (x, w) in panels {
            canvas.fine_rect(3 * x, fy, 3 * w, fh, Color::TRACK);
            canvas.fine_rect(3 * x, fy, 3 * w, 1, Color::TEXT_FAINT);
        }
        for (x, w) in &panels[..2] {
            canvas.fine_rect(3 * x + 2, fy + 2, 3 * w - 4, fh - 4, BACKGROUND);
        }
        let slit = 3 * (READOUT_X + READOUT_W + 1) + 6;
        canvas.fine_rect(slit - 1, fy + 4, 5, 12, Color::GOOD);
        canvas.fine_rect(slit, fy + 5, 3, 10, BACKGROUND);
    }
    // The can, lying at the tray's foot, its top to the right, behind the
    // flap. Not in the dim look, whose tray is unlit: the readout says
    // it went.
    if !dim {
        let (tx, tw) = (3 * TRAY_X + 2, 3 * TRAY_W - 4);
        if let Some(color) = dropped {
            render_drink(canvas, &DROPPED, tx + tw - 30, fy + 11, color, true);
        }
        canvas.fine_tone(tx, fy + 2, tw, 6, 2, Color::TEXT_FAINT);
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

/// A rectangle's outline on the canvas's grid, a pixel thick: the dim
/// look's.
fn outline(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, color: Color) {
    canvas.fill_rect(x, y, w, 1, color);
    canvas.fill_rect(x, y + h - 1, w, 1, color);
    canvas.fill_rect(x, y, 1, h, color);
    canvas.fill_rect(x + w - 1, y, 1, h, color);
}
