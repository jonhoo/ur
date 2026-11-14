//! Living Tokyo's alarms: the alarms screen, the phone's, the editor, and
//! what rings (an alarm, a timer, "find my watch").

use core::fmt::Write as _;

use embassy_time::Duration;

use super::layout::{ALARM_ROW, ALARM_ROWS_TOP, EDITOR_BUTTONS_Y};
use super::{BACKGROUND, street};
use crate::alarm;
use crate::canvas::{Canvas, Color};
use crate::font;
use crate::font::fine::Style;
use crate::themes::Editor;
use crate::themes::helpers::{duration, hhmm};
use crate::ui::{Editing, Fits, Status};

// ==============================================================================
// The alarms: a departure board
// ==============================================================================

// The watch's alarms as a station's departure board, hanging under a
// platform's roof: a row a departure (smart wake, the two alarms, the two
// timers), each its kind, when it goes and its state, in the board's
// amber LEDs. It's a board to read, not a station to look at: the rows
// have the screen, and the platform is a strip under them.
//
// What lives is the data. The next to ring is flagged (its kind in red,
// what rings) and counts down ("IN 7H05", its seconds in the last ten
// minutes); a running timer goes at the time it'll ring, counting down
// too, its colon ticking; and what's off, or a timer not running, is a
// dark row. Under the board, past the platform's edge, the night's
// weather, quietly: rain falling, or a few stars. Dim, the board alone,
// still.
//
// At the panel's resolution, the rows are the board's dot matrix, an LED
// a pixel of the font (`street::led_text`), the seams between them
// hairlines, and the rail and the platform's studded yellow edge drawn
// as what they are. The board's frame stays on the canvas's grid, as
// does a row that ticks every second (a running timer's, or one in its
// last ten minutes): a fine row that changes goes to the panel whole,
// with every fine row in its band (see `Canvas::refine`), where the
// canvas sends a changed figure as a narrow span. So a ticking row is
// the canvas's bolder lettering among the LEDs.

/// The board's columns: its kind, when it goes, and (right-aligned to
/// the last) its state.
const KIND_X: i32 = 4;
const TIME_X: i32 = 27;
const STATE_RIGHT: i32 = 76;
/// The rods the boards hang from, clear of the longest title they hang
/// beside (PHONE ALARMS).
const RODS: [i32; 2] = [59, 73];
/// The rail beyond the platform's edge, and the edge itself, the
/// last row.
const RAILS: i32 = 76;
const PLATFORM_EDGE: i32 = 79;
/// How far above the platform's edge the rain shows, at most.
const RAIN_ROWS: i32 = 24;

/// How a board's row is lit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lit {
    /// The next to ring.
    Next,
    /// On, or running.
    On,
    /// Off, or a timer not running: a dark row.
    Dark,
}

/// Seconds until something goes, for a board: the minutes and seconds
/// in its last ten minutes ("9:41"), then the minutes, rounded up
/// ("45M"), then hours and minutes ("7H05"), and past ten hours, the
/// hours ("11H"), to keep to a board's column.
fn until(seconds: i64) -> heapless::String<8> {
    let seconds = seconds.max(0);
    let minutes = (seconds + 59) / 60;
    let mut text = heapless::String::new();
    let result = if seconds < 10 * 60 {
        write!(text, "{}:{:02}", seconds / 60, seconds % 60)
    } else if minutes < 60 {
        write!(text, "{minutes}M")
    } else if minutes < 10 * 60 {
        write!(text, "{}H{:02}", minutes / 60, minutes % 60)
    } else {
        write!(text, "{}H", minutes / 60)
    };
    result.fits();
    text
}

/// A time of day, from local minutes (any day's), as HH:MM, its colon
/// left out on a `tick` (a running timer's, a second in two).
fn clock(minutes: i64, tick: bool) -> heapless::String<8> {
    let minutes = minutes.rem_euclid(24 * 60);
    let colon = if tick { ' ' } else { ':' };
    let mut text = heapless::String::new();
    write!(text, "{:02}{colon}{:02}", minutes / 60, minutes % 60).fits();
    text
}

/// One of the alarms board's rows.
struct Departure {
    kind: &'static str,
    /// When it goes (or a timer not running, how long it runs).
    time: heapless::String<8>,
    /// Seconds until it goes, if it's set to and that's known.
    left: Option<i64>,
    /// Whether it's on (a timer: running).
    on: bool,
    timer: bool,
}

/// The board: smart wake's next time (weekday's or weekend's, whichever
/// comes first), each alarm's, and each timer's, in rows a tap on which
/// opens it in the editor (see `LivingTokyo::alarm_at`).
pub(super) fn render_alarms(
    canvas: &mut Canvas,
    settings: &alarm::Settings,
    state: &alarm::State,
    status: &Status,
    dim: bool,
) {
    canvas.clear(BACKGROUND);
    let now = status.local_second;
    let uptime = status.uptime_secs as i64;
    let bottom = ALARM_ROWS_TOP + ALARM_ROW * 5;
    render_platform(
        canvas,
        bottom + 1,
        status.local_second,
        status.uptime_secs,
        dim,
    );
    street::board(
        canvas,
        0,
        ALARM_ROWS_TOP - 1,
        80,
        bottom - ALARM_ROWS_TOP + 2,
        RODS,
    );
    street::title(canvas, "ALARMS", Color::ACCENT);

    // Each departure, and when it goes. Smart wake's next is the one
    // still to ring: tomorrow's, once this morning's has.
    let minute = now.map(|second| second.div_euclid(60));
    let smart = minute.map(|minute| {
        let next = alarm::next_smart(settings, minute);
        if state.smart_rang() == Some(next) {
            alarm::next_smart(settings, next)
        } else {
            next
        }
    });
    let after =
        |at: Option<i64>, on: bool| now.zip(at).filter(|_| on).map(|(now, at)| at * 60 - now);
    let mut rows: heapless::Vec<Departure, 5> = heapless::Vec::new();
    rows.push(Departure {
        kind: "SMART",
        time: clock(smart.unwrap_or(settings.smart_weekday.into()), false),
        left: after(smart, settings.smart),
        on: settings.smart,
        timer: false,
    })
    .ok();
    for i in 0..alarm::ALARMS {
        let time = settings.at_time[i];
        let at = minute.map(|minute| alarm::next_at(time, 0, minute));
        rows.push(Departure {
            kind: "ALARM",
            time: clock(time.into(), false),
            left: after(at, settings.at[i]),
            on: settings.at[i],
            timer: false,
        })
        .ok();
    }
    for t in 0..alarm::TIMERS {
        // Running: the time of day it rings, if that's known, its colon
        // ticking, as an LED clock's does. Not: how long it runs.
        let left = state.remaining(t, uptime);
        let tick = !dim && status.uptime_secs % 2 == 1;
        let time = match (left, now) {
            (Some(left), Some(now)) => clock((now + left).div_euclid(60), tick),
            _ => duration(settings.timers[t]),
        };
        rows.push(Departure {
            kind: "TIMER",
            time,
            left,
            on: left.is_some(),
            timer: true,
        })
        .ok();
    }

    let next = rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| Some((i, row.left?)))
        .min_by_key(|&(_, left)| left)
        .map(|(i, _)| i);
    for (i, row) in rows.iter().enumerate() {
        let top = ALARM_ROWS_TOP + ALARM_ROW * i as i32;
        if i > 0 {
            seam(canvas, top - 1, dim);
        }
        let lit = if Some(i) == next {
            Lit::Next
        } else if row.on {
            Lit::On
        } else {
            Lit::Dark
        };
        let mut state = heapless::String::<8>::new();
        let mut ticks = row.timer && row.on;
        match row.left {
            Some(left) if row.timer || lit == Lit::Next => {
                write!(state, "IN {}", until(left)).fits();
                ticks |= left < 10 * 60;
            }
            _ if row.on => state.push_str("ON").fits(),
            _ if row.timer => {}
            _ => state.push_str("OFF").fits(),
        }
        let y = top + (ALARM_ROW - 1 - 5) / 2;
        departure(canvas, y, row.kind, &row.time, &state, lit, !ticks);
    }
}

/// A seam between a board's rows, at the canvas's row `y`: its panels'
/// join, a hairline, so a row reads as a strip to tap. Solid in the dim
/// look, where a checkered tone flickers with the panel's inversion at
/// idle mode's refresh.
fn seam(canvas: &mut Canvas, y: i32, dim: bool) {
    let level = if dim { 4 } else { 2 };
    canvas.fine_tone(6, 3 * y + 1, 228, 1, level, Color::TRACK);
}

/// A departure on the alarms board, its lettering's top at `y`: its kind
/// (red if it's the next to ring), its time and its state in the
/// board's amber, "ON" in green; a dark row, all faint. In the board's
/// LEDs if `fine` (see `street::led_text`): a row that ticks every second
/// stays on the canvas.
fn departure(
    canvas: &mut Canvas,
    y: i32,
    kind: &str,
    time: &str,
    state: &str,
    lit: Lit,
    fine: bool,
) {
    let (kind_color, text) = match lit {
        Lit::Next => (Color::HEART, Color::HEART_SHINE),
        Lit::On => (Color::HEART_SHINE, Color::HEART_SHINE),
        Lit::Dark => (Color::TEXT_FAINT, Color::TEXT_FAINT),
    };
    let state_color = if state == "ON" { Color::GOOD } else { text };
    street::led_text(canvas, KIND_X, y, kind, kind_color, fine);
    let end = street::led_text(canvas, TIME_X, y, time, text, fine);
    let state_x = STATE_RIGHT - font::small_text_width(state);
    debug_assert!(end < state_x, "{state} runs into {time}");
    street::led_text(canvas, state_x, y, state, state_color, fine);
}

/// Under a board whose bottom edge is row `top`, past the platform's
/// edge: the night (rain falling on a rainy night, a few stars on a
/// clear one, one twinkling, fewer as dawn comes), the rails, and the
/// platform's edge. Dim, the rails and the edge alone.
fn render_platform(canvas: &mut Canvas, top: i32, now: Option<i64>, tick: u64, dim: bool) {
    let rain = now.is_some_and(|second| street::rainy(street::evening(second)));
    let light = street::daylight(now.map_or(0, street::hour_of));
    if !dim && rain {
        // Over the rails alone, however much sky the board leaves: rain
        // seen past the edge, not a screen of it.
        let from = (top + 1).max(PLATFORM_EDGE - RAIN_ROWS);
        street::fine_rain(canvas, 3 * from, 3 * PLATFORM_EDGE, tick, Color::TEXT_FAINT);
    } else if !rain {
        // Stars from the table where the board leaves sky, as many as
        // the dawn leaves out.
        let mut shown: heapless::Vec<(i32, i32), 12> = heapless::Vec::new();
        let count =
            PLATFORM_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
        for &star in PLATFORM_STARS[..count]
            .iter()
            .filter(|&&(_, y)| y > top && y < RAILS - 1)
        {
            shown.push(star).fits();
        }
        street::fine_stars(canvas, &shown, (!dim).then_some(tick));
    }
    // The rail beyond the edge, its sleepers' ends under it; and the
    // platform's edge, its yellow tactile strip studded, as Japan's
    // platforms have it.
    rails(canvas, 3 * RAILS);
    let edge = 3 * PLATFORM_EDGE - 1;
    canvas.fine_rect(0, edge, 240, 4, Color::HEART_SHINE);
    for x in (1..240).step_by(4) {
        canvas.fine_pixel(x, edge + 1, Color::SHADOW);
        canvas.fine_pixel(x + 2, edge + 3, Color::SHADOW);
    }
}

/// A railway line seen from the side, across the screen at the panel's
/// row `y`: the rail, lit along its head, and its sleepers' ends under
/// it.
fn rails(canvas: &mut Canvas, y: i32) {
    canvas.fine_rect(0, y, 240, 1, Color::TEXT_DIM);
    canvas.fine_rect(0, y + 1, 240, 1, Color::TEXT_FAINT);
    for x in (3..240).step_by(12) {
        canvas.fine_rect(x, y + 3, 6, 2, Color::TRACK);
    }
}

/// Stars over the rails: few, spread along the open sky, for a board of
/// any height; as the sky lightens, the last go first.
const PLATFORM_STARS: [(i32, i32); 12] = [
    (12, 71),
    (63, 70),
    (35, 46),
    (70, 33),
    (8, 52),
    (50, 60),
    (24, 30),
    (44, 72),
    (17, 63),
    (58, 41),
    (31, 56),
    (74, 50),
];

/// The phone's alarms on the same board, a row each: the time, the
/// weekdays it rings on (or "ONCE"), and whether it's on; the next to ring
/// flagged, its time red, counting down. As many rows as it has (ten,
/// at most, as many as Gadgetbridge sends), the board as tall as they
/// are, the night under it.
pub(super) fn render_phone_alarms(canvas: &mut Canvas, phone: &alarm::Phone, dim: bool) {
    canvas.clear(BACKGROUND);
    let alarms = phone.alarms();
    let now = street::local_now();
    // Ten rows a little closer, to leave the platform's edge its room.
    let pitch = if alarms.len() > 8 { 6 } else { 7 };
    let top = ALARM_ROWS_TOP;
    let bottom = top + pitch * alarms.len().max(1) as i32 + 1;
    render_platform(
        canvas,
        bottom,
        now,
        now.map_or(0, |second| second as u64),
        dim,
    );
    street::board(canvas, 0, top - 1, 80, bottom - top + 2, RODS);
    street::title(canvas, "PHONE ALARMS", Color::ACCENT);
    if alarms.is_empty() {
        street::led_text(canvas, KIND_X, top + 1, "NONE", Color::TEXT_FAINT, true);
        return;
    }

    let next = now.and_then(|now| {
        let minute = now.div_euclid(60);
        alarms
            .iter()
            .enumerate()
            .filter(|(_, alarm)| alarm.on())
            .map(|(i, alarm)| (i, alarm::next_at(alarm.time, alarm.days, minute) * 60 - now))
            .min_by_key(|&(_, left)| left)
    });
    for (row, alarm) in alarms.iter().enumerate() {
        let y = top + 1 + pitch * row as i32;
        let on = alarm.on();
        let lit = match next {
            Some((i, _)) if i == row => Lit::Next,
            _ if on => Lit::On,
            _ => Lit::Dark,
        };
        let text = if on {
            Color::HEART_SHINE
        } else {
            Color::TEXT_FAINT
        };
        let time = if lit == Lit::Next { Color::HEART } else { text };
        // In the board's LEDs, but for the next to ring in its last ten
        // minutes, whose seconds tick (see `departure`).
        let fine = !matches!(next, Some((i, left)) if i == row && left < 10 * 60);
        street::led_text(canvas, KIND_X, y, &hhmm(alarm.time.into()), time, fine);
        if alarm.days == 0 {
            street::led_text(canvas, TIME_X, y, "ONCE", text, fine);
        } else {
            // Its weekdays lit, the others dark LEDs (faint, dim, where
            // the dark's half-tone would break the letters up), so the
            // row reads as a little calendar.
            let unlit = if dim { Color::TEXT_FAINT } else { Color::TRACK };
            for (day, letter) in ["M", "T", "W", "T", "F", "S", "S"].into_iter().enumerate() {
                let color = if alarm.days >> day & 1 != 0 {
                    text
                } else {
                    unlit
                };
                street::led_text(canvas, TIME_X + 4 * day as i32, y, letter, color, fine);
            }
        }
        // No room for "IN" beside the weekdays: the red time says it's
        // the next.
        let (state, color) = match (lit, next) {
            (Lit::Next, Some((_, left))) => (until(left), Color::HEART_SHINE),
            (_, _) if on => ("ON".try_into().unwrap_or_default(), Color::GOOD),
            _ => ("OFF".try_into().unwrap_or_default(), Color::TEXT_FAINT),
        };
        let x = STATE_RIGHT - font::small_text_width(&state);
        street::led_text(canvas, x, y, &state, color, fine);
    }
}

// ==============================================================================
// The editor: one departure, big
// ==============================================================================

/// The editor's board, under the title, down to its bottom edge.
const EDITOR_BOARD_BOTTOM: i32 = 54;
/// The big digits' top, and their columns: the hours on the left half and
/// the minutes on the right, matching where to swipe.
const EDITOR_DIGITS_Y: i32 = 15;
const EDITOR_DIGITS_X: [i32; 4] = [5, 22, 43, 60];
const EDITOR_COLON_X: i32 = 38;

/// One departure, big, on the board: the setting (or a running timer's
/// time left, its colon ticking) in the board's big LED digits, how to
/// change it, and its two buttons as lit keys under it.
pub(super) fn render_editor(canvas: &mut Canvas, editor: &Editor, dim: bool) {
    let Editor {
        editing,
        settings,
        state,
        uptime_secs: uptime,
    } = *editor;
    canvas.clear(BACKGROUND);
    // A running timer shows what's left (as MM:SS under an hour); anything
    // else, the setting being edited (HH:MM).
    let left = match editing {
        Editing::Timer(t) => state.remaining(usize::from(t), uptime),
        _ => None,
    };
    let (title, value, on) = match editing {
        Editing::SmartWeekday => ("SMART: WEEKDAYS", settings.smart_weekday, settings.smart),
        Editing::SmartWeekend => ("SMART: WEEKENDS", settings.smart_weekend, settings.smart),
        Editing::At(i) => (
            ["ALARM 1", "ALARM 2"][usize::from(i)],
            settings.at_time[usize::from(i)],
            settings.at[usize::from(i)],
        ),
        Editing::Timer(t) => (
            ["TIMER 1", "TIMER 2"][usize::from(t)],
            settings.timers[usize::from(t)],
            left.is_some(),
        ),
    };
    street::board(
        canvas,
        0,
        ALARM_ROWS_TOP - 1,
        80,
        EDITOR_BOARD_BOTTOM - ALARM_ROWS_TOP + 2,
        RODS,
    );
    street::title(canvas, title, Color::ACCENT);

    let (pair, labels) = match left {
        Some(left) if left < 60 * 60 => ((left / 60, left % 60), ("MIN", "SEC")),
        // Two digits of hours: a timer is at most `alarm::MAX_TIMER`, but
        // should more ever be left, it reads 99:59 rather than overflowing.
        Some(left) => {
            let left = left.min(100 * 3600 - 1);
            ((left / 3600, left / 60 % 60), ("HOURS", "MINUTES"))
        }
        None => (
            (i64::from(value / 60), i64::from(value % 60)),
            ("HOURS", "MINUTES"),
        ),
    };
    let digits = [pair.0 / 10, pair.0 % 10, pair.1 / 10, pair.1 % 10];
    for (&digit, x) in digits.iter().zip(EDITOR_DIGITS_X) {
        street::led_digit(
            canvas,
            x,
            EDITOR_DIGITS_Y,
            digit as u64,
            Color::HEART_SHINE,
            None,
            left.is_none(),
        );
    }
    // The colon ticks while a timer runs.
    if left.is_none() || dim || uptime % 2 == 0 {
        street::led_colon(
            canvas,
            EDITOR_COLON_X,
            EDITOR_DIGITS_Y,
            Color::HEART_SHINE,
            left.is_none(),
        );
    }
    // What the digits are and how to change them, in the board's LEDs
    // too: they stay still while a timer's digits tick.
    let labels_y = EDITOR_DIGITS_Y + street::LED_H + 4;
    let hint = if left.is_some() {
        "LEFT"
    } else {
        "SWIPE UP/DOWN"
    };
    for (centre, y, text) in [
        (20, labels_y, labels.0),
        (59, labels_y, labels.1),
        (40, labels_y + 8, hint),
    ] {
        let x = centre - font::small_text_width(text) / 2;
        street::led_text(canvas, x, y, text, Color::TEXT_FAINT, true);
    }

    let (left, left_color) = match (editing, on) {
        (Editing::Timer(_), true) => ("STOP", Some(Color::BAD)),
        (Editing::Timer(_), false) => ("START", Some(Color::GOOD)),
        (_, true) => ("ON", Some(Color::GOOD)),
        (_, false) => ("OFF", None),
    };
    let right = if editing == Editing::SmartWeekday {
        "NEXT"
    } else {
        "DONE"
    };
    key(canvas, 2, left, left_color, dim);
    key(canvas, 42, right, Some(Color::ACCENT), dim);
}

/// One of the editor's buttons, 36 wide from the canvas's `x`, at the
/// panel's resolution: a key, its face lit in `lit` (or unlit, dark, for
/// a switch that's off), its top edge catching the light, in a slate
/// bezel deeper at the bottom, as a key stands proud of its panel; its
/// label lettered as a sign's. In the dim look, no light on its edge: a
/// checkered tone there flickers at idle mode's refresh.
fn key(canvas: &mut Canvas, x: i32, label: &str, lit: Option<Color>, dim: bool) {
    let (x, y, w, h) = (3 * x, 3 * EDITOR_BUTTONS_Y, 3 * 36, 3 * 18);
    rounded(canvas, x, y, w, h, Color::TRACK);
    let (face, ink) = match lit {
        Some(color) => (color, BACKGROUND),
        None => (Color::SHADOW, Color::TEXT_DIM),
    };
    let (fw, fh) = (w - 6, h - 9);
    rounded(canvas, x + 3, y + 3, fw, fh, face);
    if !dim {
        canvas.fine_tone(x + 5, y + 4, fw - 4, 1, 2, Color::TEXT);
    }
    let style = street::LETTERING;
    let lx = x + (w - font::Face::Fine(style).width(label)) / 2;
    let ly = y + 3 + (fh - style.height()) / 2;
    canvas.fine_type(lx, ly, label, style, ink, None);
}

/// A box `w` by `h` from the panel's `(x, y)`, its corners rounded by a
/// pixel, as [`street::fine_sign`]'s.
fn rounded(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, color: Color) {
    canvas.fine_rect(x + 2, y, w - 4, h, color);
    canvas.fine_rect(x, y + 2, w, h - 4, color);
    canvas.fine_rect(x + 1, y + 1, w - 2, h - 2, color);
}

// ==============================================================================
// An alarm ringing: a level crossing
// ==============================================================================

// A level crossing at night, its two red lamps flashing in turn and its
// boom down across the screen: what rings and won't stop until it's
// dealt with. The crossing stands on the right; the time and what to do
// read down the left, the title on a red sign, the time above the boom
// and the hints below it. Behind, the far city along the horizon, and
// the road in front.
//
// It's drawn at the panel's resolution, so that it reads as a crossing
// without being told: the crossbuck's yellow and black boards, each lamp
// a lens under its hood in a dark backplate, the boom's slanting stripes
// and its little lamps, the gate's box at the post, the rail and its
// sleepers. The time is in the city's lettering: it changes by the
// minute. What changes every second is kept to few rows: the lamps and
// their haze within one band of the canvas's eight rows (see
// `Canvas::refine`), the boom's lamps, and the road's light under them.
//
// It lives on the hour and the night. The city's windows are lit as its
// people are up (`street::city_lights`: most in the evening, hardly any
// at 02:00), and from 04:00 the sky behind it glows, higher each hour
// (`street::fine_horizon`), so 02:00, 05:00 and 06:30 are three skies at a
// glance. On a rainy night (`street::rainy`, a third of them) the rain
// falls and the road is wet, the lit lamp broken up in it and the rain
// splashing; on a clear one, the stars are out, one twinkling, and the
// lit lamp's red pools on the road. The lamps take turns each second. In
// the dim look (the quiet night), no weather, no glow, no twinkle and no
// far city's half-toned walls: the lamps alone flash.

/// The horizon: the rails, the far city's ground, and the road's top
/// (a canvas row).
const CROSSING_GROUND: i32 = 69;
/// The far city's tallest building (canvas rows).
const CROSSING_CITY: i32 = 15;
/// The crossing, in the panel's pixels: the post's left (it's 6 wide),
/// the lamps' middles either side of it, and the boom's top (it's 5
/// thick).
const POST: i32 = 207;
const LAMPS: [i32; 2] = [188, 231];
const LAMP_Y: i32 = 60;
const BOOM: i32 = 99;
/// A lamp's lens, and its backplate, across.
const LENS: i32 = 14;
const PLATE: i32 = 20;
/// The time, big, in the city's lettering, and its capitals' top left.
const TIME: Style = Style {
    cap: 40,
    stroke: 104,
    caps: true,
};
const TIME_AT: (i32, i32) = (12, 45);

/// Stars over the crossing, clear of the title, the time, the hints and
/// the crossbuck; as the sky lightens, the last go first.
const CROSSING_STARS: [(i32, i32); 10] = [
    (58, 3),
    (6, 31),
    (40, 30),
    (24, 52),
    (62, 36),
    (59, 11),
    (55, 44),
    (12, 54),
    (36, 50),
    (64, 49),
];

/// The title, the time, big, and how to stop it, beside the crossing.
pub(super) fn render_ringing(canvas: &mut Canvas, kind: alarm::Kind, status: &Status, dim: bool) {
    canvas.clear(BACKGROUND);
    // The seconds it's been up, for the lamps and the rain; the hour,
    // for the sky and the city; the evening, for the weather.
    let tick = status.uptime_secs;
    let hour = status.local_second.map_or(0, street::hour_of);
    let rain = status
        .local_second
        .is_some_and(|second| street::rainy(street::evening(second)));
    let light = street::daylight(hour);

    // The sky: the dawn's glow, then the stars still out on a clear
    // night (one brighter, a second in three), then the rain on a wet
    // one; and the far city in front of the glow.
    let ground = 3 * CROSSING_GROUND;
    if !dim {
        // Three levels at most: the fourth's would reach the hints.
        street::fine_horizon(canvas, 0, 240, ground, light.min(3));
    }
    if !rain {
        let shown =
            CROSSING_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
        street::fine_stars(canvas, &CROSSING_STARS[..shown], (!dim).then_some(tick));
    }
    if !dim {
        let lights = street::city_lights(hour);
        street::fine_skyline(canvas, 0, 240, ground, 3 * CROSSING_CITY, lights);
    }
    if rain && !dim {
        street::fine_rain(canvas, 0, ground, tick, Color::TEXT_FAINT);
        // The rain passes behind the time and the hints, not through
        // their letters, which it would break up.
        canvas.fill_rect(3, 14, 56, 18, BACKGROUND);
        canvas.fill_rect(3, 37, 50, 15, BACKGROUND);
    }
    let lit = render_crossing(canvas, tick.is_multiple_of(2), dim);
    if !dim {
        render_road(canvas, lit, rain, tick);
    }

    let title = match kind {
        alarm::Kind::Smart => "GOOD MORNING",
        alarm::Kind::At(_) | alarm::Kind::Phone(_) | alarm::Kind::Snooze => "ALARM",
        alarm::Kind::Timer(_) => "TIME'S UP",
    };
    let end = street::title(canvas, title, Color::HEART);
    debug_assert!(end < 61, "the ringing's title runs into the crossbuck");
    if let Some(second) = status.local_second {
        // The hours and the minutes in the city's lettering, and between
        // them a colon of two red dots, as a clock's, at their middle
        // rather than the face's at its baseline.
        let minute = second.div_euclid(60).rem_euclid(24 * 60);
        let edge = (!dim).then_some(Color::TEXT_FAINT);
        let (mut x, y) = TIME_AT;
        for (i, part) in [minute / 60, minute % 60].into_iter().enumerate() {
            if i == 1 {
                for dot in [12, 30] {
                    street::fine_ellipse(canvas, x, y + dot, 7, 7, Color::HEART);
                }
                x += 7 + TIME.gap();
            }
            let mut text = heapless::String::<4>::new();
            write!(text, "{part:02}").fits();
            x = canvas.fine_type(x, y, &text, TIME, Color::TEXT, edge);
        }
        debug_assert!(x < LAMPS[0] - PLATE / 2, "the time runs into the lamps");
    }
    canvas.text(4, 38, "TAP: SNOOZE", Color::TEXT_DIM);
    canvas.text(4, 46, "BUTTON: STOP", Color::TEXT_DIM);
}

/// The road in front of the crossing, under the rails. Wet on a rainy
/// night: the lit lamp (its middle at the panel's column `lit`) broken
/// up in the road under it, and the rain splashing, moving with `tick`.
/// Dry on a clear one: the lit lamp's red light pooled on it.
fn render_road(canvas: &mut Canvas, lit: i32, rain: bool, tick: u64) {
    // From the canvas's band of rows under the far city's (see
    // `Canvas::refine`), so that a second's change of the light on it
    // sends only the road's rows.
    let road = 3 * (CROSSING_GROUND + 3);
    if !rain {
        street::fine_glow(canvas, lit - 30, road - 2, 60, 12, Color::HEART);
        return;
    }
    street::fine_reflection(canvas, lit - 12, 24, road, 6, Color::HEART);
    // Splashes: a drop's crown, here and there, somewhere new each
    // second.
    for i in 0..6_u64 {
        let x = ((i * 67 + tick * 53) % 160) as i32 + 8;
        let y = road + 5 + ((i * 7 + tick * 3) % 19) as i32;
        canvas.fine_rect(x - 1, y, 3, 1, Color::TEXT_FAINT);
        canvas.fine_pixel(x - 2, y - 1, Color::TEXT_FAINT);
        canvas.fine_pixel(x + 2, y - 1, Color::TEXT_FAINT);
        canvas.fine_pixel(x, y - 2, Color::TEXT_FAINT);
    }
}

/// The crossing: the crossbuck, the post, the lamps (the left one lit if
/// `left`, else the right), the boom with its own small lamps and its
/// gate's box, and the rail. Returns the lit lamp's middle, for its
/// light on the road.
fn render_crossing(canvas: &mut Canvas, left: bool, dim: bool) -> i32 {
    let ground = 3 * CROSSING_GROUND;
    // The post, up from the rail into the crossbuck, striped at its
    // foot as the boom is. Plain, it's on the canvas's grid, two of its
    // pixels wide (`POST` is on it): drawn finely, it would make every
    // row it stands in a fine one, sent again whenever a lamp beside it
    // changes its band.
    canvas.fill_rect(POST / 3, 7, 2, CROSSING_GROUND - 17, Color::TEXT_FAINT);
    striped(canvas, POST, ground - 30, 6, 30);

    // The crossbuck: two boards crossed over the post, each striped
    // across, yellow and black, as Japan's are.
    let rows = canvas.fine_rows();
    for y in rows.start.max(4)..rows.end.min(38) {
        let dx = (y - 4) * 46 / 34;
        for (from, sign) in [(186, 1), (241, -1)] {
            let middle = from + sign * dx;
            for x in middle - 4..middle + 5 {
                // Bands across the board: along it, every 7 pixels.
                let along = (3 * sign * x + 4 * y).rem_euclid(70);
                let color = if along < 35 {
                    Color::HEART_SHINE
                } else {
                    Color::TRACK
                };
                canvas.fine_pixel(x, y, color);
            }
        }
    }

    // The lamps: their bracket across the post, the lit one's haze, and
    // each a lens in a dark backplate under its hood, the lit one red
    // with a gleam on it.
    let lit = usize::from(!left);
    let [l, r] = LAMPS;
    canvas.fine_rect(l, LAMP_Y - 1, r - l, 3, Color::TEXT_FAINT);
    if !dim {
        street::fine_glow(canvas, LAMPS[lit] - 20, LAMP_Y - 12, 40, 24, Color::HEART);
    }
    for (i, &x) in LAMPS.iter().enumerate() {
        // A disc `d` across round the lamp's middle, `dy` lower.
        let mut disc = |d: i32, dy: i32, color: Color| {
            let top = LAMP_Y - d / 2 + dy;
            street::fine_ellipse(canvas, x - d / 2, top, d, d, color);
        };
        // The hood, rimmed, standing out over the top of the backplate.
        disc(PLATE + 4, -3, Color::TEXT_FAINT);
        disc(PLATE + 2, -2, Color::TRACK);
        disc(PLATE, 0, Color::SHADOW);
        let lens = if i == lit { Color::HEART } else { Color::TRACK };
        disc(LENS, 0, lens);
        if i == lit && !dim {
            let corner = LAMP_Y - LENS / 2 + 3;
            canvas.fine_tone(x - LENS / 2 + 3, corner, 4, 2, 2, Color::TEXT);
        }
    }

    // The boom, down, from the left edge to the post, its two small
    // lamps flashing with the left one; and the gate's box at the post,
    // which it swings from.
    striped(canvas, 0, BOOM, POST, 5);
    for x in [42, 120] {
        let lamp = if left { Color::HEART } else { Color::TRACK };
        canvas.fine_rect(x, BOOM - 3, 5, 3, lamp);
    }
    // The box on the canvas's grid too, but for its lid's edge.
    canvas.fill_rect(POST / 3 - 2, BOOM / 3 - 2, 6, 6, Color::TRACK);
    canvas.fine_rect(POST - 6, BOOM - 6, 18, 1, Color::TEXT_FAINT);

    rails(canvas, ground);
    LAMPS[lit]
}

/// A run of a crossing's yellow and black, `w` by `h` from the panel's
/// `(x, y)`: bands 7 pixels wide slanting a pixel a row, the black as
/// the far buildings' slate, as black proper would vanish into the night
/// and leave yellow dashes.
fn striped(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
    let rows = canvas.fine_rows();
    for py in y.max(rows.start)..(y + h).min(rows.end) {
        let mut from = x;
        while from < x + w {
            let phase = (from + py).rem_euclid(14);
            let (color, run) = if phase < 7 {
                (Color::HEART_SHINE, 7 - phase)
            } else {
                (Color::TRACK, 14 - phase)
            };
            let run = run.min(x + w - from);
            canvas.fine_rect(from, py, run, 1, color);
            from += run;
        }
    }
}

// ==============================================================================
// Finding the watch: HERE in a puddle
// ==============================================================================

// "Find my watch" is HERE, big, lying in a puddle in the street at
// night, with a drop falling into it each second and its ripples
// spreading round the word: what catches the eye from across a room
// (or from under a cushion). The title on a red sign, as what rings is,
// and how to stop it under the puddle.
//
// Its one living thing is the water: a ring a second spreads round
// HERE, one to three at once, each fainter as it spreads. On a clear
// night, a drop gathers on the corner of an eave over the street and
// falls, a second each; on a rainy one it rains, and the longer the
// phone looks the harder it rains, the puddle pocked with more drops
// each second. The hour is quiet: the far city's windows, low on the
// horizon, lit as its people are up. In the dim look, the rings still
// spread (it's calling for someone), but there's no weather or city.
//
// At the panel's resolution, as the rings are what moves and a ring a
// canvas pixel thick was a stair, not water. They change the rows round
// HERE every second, which go to the panel whole (see `Canvas::refine`):
// it shows for a minute at most, while the phone looks.

/// The horizon: the far city's ground, and the puddle's far edge (a
/// canvas row).
const FIND_GROUND: i32 = 30;
/// HERE, big, in the city's lettering.
const HERE: Style = Style {
    cap: 42,
    stroke: 96,
    caps: true,
};
/// The ripples' middle, and HERE's, in the panel's pixels.
const RIPPLE: (i32, i32) = (120, 153);
/// The corner of the eave the drop falls from on a clear night, the
/// panel's (the eave runs from it to the right edge): its gutter's end,
/// over the ripples' middle.
const EAVE: (i32, i32) = (118, 45);
/// Stars, left of the eave and clear of the title.
const FIND_STARS: [(i32, i32); 3] = [(8, 16), (27, 20), (16, 13)];

/// `elapsed` is how long the phone has been looking.
pub(super) fn render_find(canvas: &mut Canvas, elapsed: Duration, dim: bool) {
    canvas.clear(BACKGROUND);
    let night = street::Night::now();
    let seconds = elapsed.as_secs();
    let (cx, cy) = RIPPLE;
    let ground = 3 * FIND_GROUND;

    // The sky, the far city, and the eave over the street.
    if !dim {
        if night.rain {
            // Harder the longer it looks: another fall of it at 20 s,
            // and a third at 40, each a few rows behind the last.
            for fall in 0..(1 + seconds / 20).min(3) {
                let tick = night.tick + fall * 2;
                street::fine_rain(canvas, 0, ground, tick, Color::TEXT_FAINT);
            }
        } else {
            let light = street::daylight(night.hour);
            let shown =
                FIND_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
            street::fine_stars(canvas, &FIND_STARS[..shown], None);
        }
        let lights = street::city_lights(night.hour);
        street::fine_skyline(canvas, 0, 240, ground, 3 * 8, lights);
    }
    canvas.fine_rect(0, ground, 240, 1, Color::TRACK);
    if !dim {
        street::fine_reflection(canvas, 0, 240, ground + 3, 3, Color::TRACK);
    }
    eave(canvas, dim);
    if !dim && !night.rain {
        // The drop: gathering under the gutter's end, then falling;
        // both within one band of the canvas's rows (see
        // `Canvas::refine`), so that it sends only that band's.
        let ey = EAVE.1 + 3;
        if seconds.is_multiple_of(2) {
            street::fine_ellipse(canvas, cx - 1, ey, 3, 4, Color::TEXT);
        } else {
            canvas.fine_rect(cx, ey + 10, 1, 3, Color::TEXT_FAINT);
            street::fine_ellipse(canvas, cx - 1, ey + 13, 3, 5, Color::TEXT);
        }
    }

    street::title(canvas, "FIND MY WATCH", Color::HEART);

    // The ripples round HERE: one to three, a ring a second, widening
    // and fading. The widest stays under the far edge.
    let rings = seconds % 3 + 1;
    for ring in 1..=rings as i32 {
        let ry = 3 * (8 + 4 * ring);
        let rx = 2 * ry + 30;
        // Solid in the dim look, where a checkered tone flickers at
        // idle mode's refresh.
        let level = if dim { 4 } else { (5 - ring) as u8 };
        street::fine_ring(
            canvas,
            (cx - rx, cy - ry, 2 * rx, 2 * ry),
            2,
            level,
            Color::ACCENT,
        );
    }

    // The puddle: the drops of the rain pocking it, more the longer the
    // phone has looked (a pair at first, twenty after a minute), each a
    // small ring, somewhere new each second.
    if night.rain && !dim {
        let drops = (2 + seconds / 3).min(20);
        for i in 0..drops {
            let x = ((i * 113 + seconds * 41) % 228) as i32 + 6;
            let y = ground + 8 + ((i * 89 + seconds * 23) % 130) as i32;
            canvas.fine_rect(x - 1, y - 1, 3, 1, Color::TEXT_FAINT);
            canvas.fine_rect(x - 1, y + 1, 3, 1, Color::TEXT_FAINT);
            canvas.fine_pixel(x - 2, y, Color::TEXT_FAINT);
            canvas.fine_pixel(x + 2, y, Color::TEXT_FAINT);
        }
    }

    // HERE, and how to stop it, clear of the water's marks.
    let (w, h) = (font::Face::Fine(HERE).width("HERE"), HERE.height());
    let (x, y) = (cx - w / 2, cy - h / 2);
    canvas.fine_rect(x - 4, y - 4, w + 8, h + 8, BACKGROUND);
    let edge = (!dim).then_some(Color::TEXT_FAINT);
    canvas.fine_type(x, y, "HERE", HERE, Color::TEXT, edge);
    let hint = "TAP: FOUND IT";
    let width = font::small_text_width(hint);
    canvas.fill_rect(40 - width / 2 - 2, 69, width + 4, 9, BACKGROUND);
    canvas.text_centered(40, 71, hint, Color::TEXT_DIM);
}

/// The eave over the street, from [`EAVE`] to the right edge, seen from
/// under it: the roof rising away in rows of tiles, the tiles' round
/// ends along its edge, and the fascia under them. In the dim look, the
/// tiles' ends alone.
fn eave(canvas: &mut Canvas, dim: bool) {
    let (x, y) = EAVE;
    if !dim {
        // The roof, its end cut back as a gable's, its courses of tiles
        // running away above their round ends.
        for row in 0..14 {
            let from = x + 4 + (14 - row) / 2;
            canvas.fine_rect(from, y - 14 + row, 240 - from, 1, Color::SHADOW);
        }
        for course in [y - 11, y - 7] {
            let from = x + 4 + (y - course) / 2;
            canvas.fine_rect(from, course, 240 - from, 1, Color::TRACK);
        }
        for tx in (x + 4..240).step_by(7) {
            street::fine_ellipse(canvas, tx, y - 5, 6, 8, Color::TRACK);
            street::fine_ring(canvas, (tx, y - 5, 6, 8), 1, 2, Color::TEXT_FAINT);
        }
    }
    // The gutter along its edge, a half-round lit along its lip, from
    // whose end the drop falls.
    canvas.fine_rect(x, y, 240 - x, 1, Color::TEXT_FAINT);
    if !dim {
        canvas.fine_rect(x, y + 1, 240 - x, 2, Color::TRACK);
    }
}
