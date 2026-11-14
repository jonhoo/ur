//! Living Tokyo's alarms: the alarms screen, the phone's, the editor, and
//! what rings (an alarm, a timer, "find my watch").

use core::fmt::Write as _;

use embassy_time::Duration;

use super::layout::{ALARM_ROW, ALARM_ROWS_TOP, EDITOR_BUTTONS_Y};
use super::{BACKGROUND, draw_digit, street};
use crate::alarm;
use crate::canvas::{Canvas, Color};
use crate::font;
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
            seam(canvas, top - 1);
        }
        let lit = if Some(i) == next {
            Lit::Next
        } else if row.on {
            Lit::On
        } else {
            Lit::Dark
        };
        let mut state = heapless::String::<8>::new();
        match row.left {
            Some(left) if row.timer || lit == Lit::Next => {
                write!(state, "IN {}", until(left)).fits();
            }
            _ if row.on => state.push_str("ON").fits(),
            _ if row.timer => {}
            _ => state.push_str("OFF").fits(),
        }
        let y = top + (ALARM_ROW - 1 - 5) / 2;
        departure(canvas, y, row.kind, &row.time, &state, lit);
    }
}

/// A seam between a board's rows, at row `y`: its panels' join, dotted,
/// so a row reads as a strip to tap.
fn seam(canvas: &mut Canvas, y: i32) {
    for x in (2..78).step_by(2) {
        canvas.pixel(x, y, Color::TRACK);
    }
}

/// A departure on the alarms board, its lettering's top at `y`: its kind
/// (red if it's the next to ring), its time and its state in the
/// board's amber, "ON" in green; a dark row, all faint.
fn departure(canvas: &mut Canvas, y: i32, kind: &str, time: &str, state: &str, lit: Lit) {
    let (kind_color, text) = match lit {
        Lit::Next => (Color::HEART, Color::HEART_SHINE),
        Lit::On => (Color::HEART_SHINE, Color::HEART_SHINE),
        Lit::Dark => (Color::TEXT_FAINT, Color::TEXT_FAINT),
    };
    let state_color = if state == "ON" { Color::GOOD } else { text };
    canvas.text(KIND_X, y, kind, kind_color);
    let end = canvas.text(TIME_X, y, time, text);
    debug_assert!(
        end < STATE_RIGHT - font::small_text_width(state),
        "{state} runs into {time}"
    );
    canvas.text_right(STATE_RIGHT, y, state, state_color);
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
        street::rain(canvas, from, PLATFORM_EDGE, tick, Color::TEXT_FAINT);
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
        street::stars(canvas, &shown);
        if !dim && !shown.is_empty() && tick.is_multiple_of(3) {
            let (x, y) = shown[(tick / 3) as usize % shown.len()];
            canvas.pixel(x, y, Color::TEXT);
        }
    }
    // The rail, and the platform's edge: its yellow tactile strip, in
    // dashes. (Not `street::rails`' sleepers: in so thin a strip, one
    // line fewer keeps it calm under the board.)
    canvas.fill_rect(0, RAILS, 80, 1, Color::TEXT_FAINT);
    for x in (0..80).step_by(4) {
        canvas.fill_rect(x + 1, PLATFORM_EDGE, 2, 1, Color::HEART_SHINE);
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
        canvas.text(KIND_X, top + 1, "NONE", Color::TEXT_FAINT);
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
        canvas.text(KIND_X, y, &hhmm(alarm.time.into()), time);
        if alarm.days == 0 {
            canvas.text(TIME_X, y, "ONCE", text);
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
                canvas.text(TIME_X + 4 * day as i32, y, letter, color);
            }
        }
        // No room for "IN" beside the weekdays: the red time says it's
        // the next.
        let (state, color) = match (lit, next) {
            (Lit::Next, Some((_, left))) => (until(left), Color::HEART_SHINE),
            (_, _) if on => ("ON".try_into().unwrap_or_default(), Color::GOOD),
            _ => ("OFF".try_into().unwrap_or_default(), Color::TEXT_FAINT),
        };
        canvas.text_right(STATE_RIGHT, y, &state, color);
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
        );
    }
    // The colon ticks while a timer runs.
    if left.is_none() || dim || uptime % 2 == 0 {
        street::led_colon(canvas, EDITOR_COLON_X, EDITOR_DIGITS_Y, Color::HEART_SHINE);
    }
    let labels_y = EDITOR_DIGITS_Y + street::LED_H + 4;
    canvas.text_centered(20, labels_y, labels.0, Color::TEXT_FAINT);
    canvas.text_centered(59, labels_y, labels.1, Color::TEXT_FAINT);
    let hint = if left.is_some() {
        "LEFT"
    } else {
        "SWIPE UP/DOWN"
    };
    canvas.text_centered(40, labels_y + 8, hint, Color::TEXT_FAINT);

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
    key(canvas, 2, left, left_color);
    key(canvas, 42, right, Some(Color::ACCENT));
}

/// One of the editor's buttons, 36 wide from `x`: a panel key, its face
/// lit in `lit` (or unlit, dark, for a switch that's off) in a slate
/// bezel, a row deeper at the bottom, as a key stands proud of its
/// panel.
fn key(canvas: &mut Canvas, x: i32, label: &str, lit: Option<Color>) {
    const W: i32 = 36;
    const H: i32 = 18;
    let y = EDITOR_BUTTONS_Y;
    canvas.fill_rect(x, y, W, H, Color::TRACK);
    let (face, ink) = match lit {
        Some(color) => (color, BACKGROUND),
        None => (Color::SHADOW, Color::TEXT_DIM),
    };
    canvas.fill_rect(x + 1, y + 1, W - 2, H - 3, face);
    canvas.text_centered(x + W / 2, y + 6, label, ink);
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
// It lives on the hour and the night. The city's windows are lit as its
// people are up (`street::city_lights`: most in the evening, hardly any
// at 02:00), and from 04:00 the sky behind it glows, higher each hour
// (`street::horizon`), so 02:00, 05:00 and 06:30 are three skies at a
// glance. On a rainy night (`street::rainy`, a third of them) the rain
// falls and the road is wet, the lit lamp and the windows broken up in
// it; on a clear one, the stars are out, one twinkling. The lamps take
// turns each second. In the dim look (the quiet night), no weather, no
// glow, no twinkle: the lamps alone flash.

/// The horizon: the rails, the far city's ground, and the road's top.
const CROSSING_GROUND: i32 = 69;
/// The post's left column (it's 2 px wide), under the crossbuck.
const CROSSING_POST: i32 = 69;
/// The boom's top row: down, across the screen to the post.
const CROSSING_BOOM: i32 = 33;
/// The lamps' lenses, each 6 across in a dark backplate 8 across, either
/// side of the post: their left edges, and their top.
const CROSSING_LENSES: [i32; 2] = [60, 73];
const CROSSING_LENS_Y: i32 = 17;
const LENS: i32 = 6;
/// The far city's tallest building.
const CROSSING_CITY: i32 = 15;

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
    if !dim {
        // Three levels at most: the fourth's would reach the hints.
        street::horizon(canvas, 0, 80, CROSSING_GROUND, light.min(3));
    }
    if !rain {
        let shown =
            CROSSING_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
        street::stars(canvas, &CROSSING_STARS[..shown]);
        if !dim && shown > 0 && tick.is_multiple_of(3) {
            let (x, y) = CROSSING_STARS[(tick / 3) as usize % shown];
            canvas.pixel(x, y, Color::TEXT);
        }
    }
    street::skyline(
        canvas,
        0,
        80,
        CROSSING_GROUND,
        CROSSING_CITY,
        street::city_lights(hour),
    );
    if rain && !dim {
        street::rain(canvas, 0, CROSSING_GROUND, tick, Color::TEXT_FAINT);
        // The rain passes behind the time and the hints, not through
        // their letters, which it would break up.
        canvas.fill_rect(3, 14, 56, 17, BACKGROUND);
        canvas.fill_rect(3, 37, 50, 15, BACKGROUND);
    }
    let lit = render_crossing(canvas, tick.is_multiple_of(2));
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
        // The clock's 2× digits, closer round the colon than the
        // editor's, to leave the crossing its room.
        let minute = second.div_euclid(60).rem_euclid(24 * 60);
        let digits = [
            minute / 600,
            minute / 60 % 10,
            minute % 60 / 10,
            minute % 10,
        ];
        for (&digit, x) in digits.iter().zip([4, 16, 34, 46]) {
            draw_digit(canvas, x, 15, digit as u64, 2, Color::TEXT);
        }
        for y in [19, 25] {
            canvas.fill_rect(29, y, 2, 2, Color::HEART);
        }
    }
    canvas.text(4, 38, "TAP: SNOOZE", Color::TEXT_DIM);
    canvas.text(4, 46, "BUTTON: STOP", Color::TEXT_DIM);
}

/// The road in front of the crossing, under the rails. Wet on a rainy
/// night: the lit lamp (its lens's left edge at `lit`) broken up in the
/// puddles under it, and the rain splashing, moving with `tick`. Dry on
/// a clear one: the lit lamp's red light pooled on it.
fn render_road(canvas: &mut Canvas, lit: i32, rain: bool, tick: u64) {
    let road = CROSSING_GROUND + 2;
    if rain {
        street::reflection(canvas, lit - 1, LENS + 2, road + 1, 4, Color::HEART);
        // Splashes: a drop's ring, three pixels, here and there, moving
        // on each second.
        for i in 0..5_u64 {
            let x = ((i * 23 + tick * 17) % 54) as i32 + 2;
            let y = road + 2 + ((i * 3 + tick) % 7) as i32;
            if y < 79 {
                canvas.pixel(x - 1, y + 1, Color::TEXT_FAINT);
                canvas.pixel(x, y, Color::TEXT_FAINT);
                canvas.pixel(x + 1, y + 1, Color::TEXT_FAINT);
            }
        }
    } else {
        // A pool of light, widening and thinning away from the lamp: a
        // checkerboard on the first row, every third pixel on the next,
        // every fourth on the last.
        for row in 0..3 {
            let y = road + 1 + row;
            for x in lit - 1 - row..lit + LENS + 1 + row {
                if (x + row) % (2 + row) == 0 {
                    canvas.pixel(x, y, Color::HEART);
                }
            }
        }
    }
}

/// The crossing: the crossbuck, the post, the lamps (the left one lit if
/// `left`, else the right), the boom with its own small lamps, and the
/// rails. Returns the lit lens's left edge, for its reflection.
fn render_crossing(canvas: &mut Canvas, left: bool) -> i32 {
    // Yellow and black, as Japan paints its crossings, bands `band` long
    // along a run: the black as the far buildings' slate, as black
    // proper would vanish into the night and leave yellow dashes.
    let striped = |i: i32, band: i32| {
        if (i / band) % 2 == 0 {
            Color::HEART_SHINE
        } else {
            Color::TRACK
        }
    };
    let post = CROSSING_POST;
    canvas.fill_rect(post, 12, 2, CROSSING_GROUND - 12, Color::TEXT_FAINT);
    for y in CROSSING_GROUND - 10..CROSSING_GROUND {
        canvas.fill_rect(post, y, 2, 1, striped(y - CROSSING_GROUND, 2));
    }
    // The crossbuck, a 17×12 X over the post, each arm 2 px thick: yellow,
    // with a thin dark band every fourth row, so that it reads as one
    // bold X at a glance rather than as dashes.
    for i in 0..12 {
        let color = if i % 4 == 3 {
            Color::TRACK
        } else {
            Color::HEART_SHINE
        };
        let dx = i * 15 / 11;
        canvas.fill_rect(62 + dx, 1 + i, 2, 1, color);
        canvas.fill_rect(76 - dx, 1 + i, 2, 1, color);
    }

    // The lamps' bracket, then each lamp: the lit one hazing red round
    // its dark backplate.
    let [first, last] = CROSSING_LENSES;
    canvas.fill_rect(
        first + 2,
        CROSSING_LENS_Y - 2,
        last - first,
        1,
        Color::TEXT_FAINT,
    );
    let lit = usize::from(!left);
    let x = CROSSING_LENSES[lit];
    street::glow(canvas, x, CROSSING_LENS_Y, LENS, 3, Color::HEART);
    for (i, &x) in CROSSING_LENSES.iter().enumerate() {
        street::disc(canvas, x - 1, CROSSING_LENS_Y - 1, LENS + 2, Color::TRACK);
        let lens = if i == lit {
            Color::HEART
        } else {
            Color::SHADOW
        };
        street::disc(canvas, x, CROSSING_LENS_Y, LENS, lens);
    }

    // The boom, down, from the left edge to the post, in yellow and
    // black, its two small lamps flashing with the left one.
    for x in 0..post {
        canvas.fill_rect(x, CROSSING_BOOM, 1, 2, striped(x, 4));
    }
    if left {
        for x in [14, 40] {
            canvas.fill_rect(x, CROSSING_BOOM - 1, 2, 1, Color::HEART);
        }
    }

    street::rails(canvas, 0, 80, CROSSING_GROUND);
    x
}

/// "Find my watch": HERE, big, with rings round it that spread a ring a
/// second, as a ping does, so the screen catches the eye from across a
/// room (or from under a cushion), and how to stop it. `elapsed` is how
/// long the phone has been looking.
pub(super) fn render_find(canvas: &mut Canvas, elapsed: Duration) {
    canvas.clear(BACKGROUND);
    canvas.text_centered(40, 6, "FIND MY WATCH", Color::ACCENT);
    // "HERE" at 3×: 45×15, at x 17..62, y 28..43.
    let (x, y, w, h) = (17, 28, 45, 15);
    canvas.text_scaled(x, y, "HERE", Color::TEXT, 3);
    // One to three rings, 4 px apart, outlined a pixel thick.
    let rings = elapsed.as_secs() % 3 + 1;
    for ring in 1..=rings as i32 {
        let gap = 4 * ring;
        let (left, top, width, height) = (x - gap, y - gap, w + 2 * gap, h + 2 * gap);
        canvas.fill_rect(left, top, width, 1, Color::ACCENT);
        canvas.fill_rect(left, top + height - 1, width, 1, Color::ACCENT);
        canvas.fill_rect(left, top, 1, height, Color::ACCENT);
        canvas.fill_rect(left + width - 1, top, 1, height, Color::ACCENT);
    }
    canvas.text_centered(40, 66, "TAP: FOUND IT", Color::TEXT_DIM);
}
