//! Living Tokyo's alarms: the alarms screen, the phone's, the editor, and
//! what rings (an alarm, a timer, "find my watch").

use core::fmt::Write as _;

use embassy_time::Duration;

use super::layout::{ALARMS_TOP, EDITOR_BUTTONS_Y, SMART_TOP, TIMERS_TOP};
use super::{BACKGROUND, draw_digit, street};
use crate::alarm;
use crate::canvas::{Canvas, Color};
use crate::themes::Editor;
use crate::themes::helpers::{countdown, duration, hhmm};
use crate::ui::{Editing, Fits, Status};

/// Smart wake across the top (label and state, then both times), then the
/// two alarms and the two timers in pairs of cells, each cell a label, its
/// time, and its state.
pub(super) fn render_alarms(
    canvas: &mut Canvas,
    settings: &alarm::Settings,
    state: &alarm::State,
    status: &Status,
) {
    canvas.clear(BACKGROUND);
    canvas.text(4, 3, "ALARMS", Color::ACCENT);
    let switch = |on: bool| {
        if on {
            ("ON", Color::GOOD)
        } else {
            ("OFF", Color::TEXT_FAINT)
        }
    };

    let (text, color) = switch(settings.smart);
    canvas.text(4, SMART_TOP + 1, "SMART WAKE", Color::TEXT_DIM);
    canvas.text_right(77, SMART_TOP + 1, text, color);
    let mut times = heapless::String::<20>::new();
    write!(
        times,
        "{} / {}",
        hhmm(settings.smart_weekday.into()),
        hhmm(settings.smart_weekend.into())
    )
    .fits();
    canvas.text(4, SMART_TOP + 8, &times, Color::TEXT);

    // Hairlines between the bands, so the pairs read as pairs.
    for y in [ALARMS_TOP - 2, TIMERS_TOP - 2] {
        canvas.fill_rect(4, y, 72, 1, Color::TEXT_FAINT);
    }
    let cell = |canvas: &mut Canvas,
                column: usize,
                top: i32,
                label: &str,
                value: &str,
                status: (&str, Color)| {
        let x = 4 + 40 * column as i32;
        canvas.text(x, top + 1, label, Color::TEXT_DIM);
        canvas.text(x, top + 8, value, Color::TEXT);
        canvas.text(x, top + 15, status.0, status.1);
    };
    for i in 0..alarm::ALARMS {
        let time = hhmm(settings.at_time[i].into());
        cell(
            canvas,
            i,
            ALARMS_TOP,
            ["ALARM 1", "ALARM 2"][i],
            &time,
            switch(settings.at[i]),
        );
    }
    for t in 0..alarm::TIMERS {
        // While running: time left, ticking; otherwise the duration.
        let left = state.remaining(t, status.uptime_secs as i64);
        let (value, running) = match left {
            Some(left) => (countdown(left), ("RUNNING", Color::GOOD)),
            None => (duration(settings.timers[t]), ("", Color::TEXT_FAINT)),
        };
        cell(
            canvas,
            t,
            TIMERS_TOP,
            ["TIMER 1", "TIMER 2"][t],
            &value,
            running,
        );
    }
}

/// The phone's alarms, a row each: the time, the weekdays it rings on (or
/// "ONCE"), and whether it's on. Ten rows fit, as many as Gadgetbridge
/// sends.
pub(super) fn render_phone_alarms(canvas: &mut Canvas, phone: &alarm::Phone) {
    canvas.clear(BACKGROUND);
    canvas.text(4, 3, "PHONE ALARMS", Color::ACCENT);
    for (row, alarm) in phone.alarms().iter().enumerate() {
        let y = 11 + 7 * row as i32;
        let on = alarm.on();
        let text = if on { Color::TEXT } else { Color::TEXT_FAINT };
        canvas.text(4, y, &hhmm(alarm.time.into()), text);
        if alarm.days == 0 {
            canvas.text(26, y, "ONCE", text);
        } else {
            // Its weekdays bright, the others faint, so the row reads as
            // a little calendar.
            for (day, letter) in ["M", "T", "W", "T", "F", "S", "S"].into_iter().enumerate() {
                let color = if on && alarm.days >> day & 1 != 0 {
                    Color::TEXT
                } else {
                    Color::TEXT_FAINT
                };
                canvas.text(26 + 4 * day as i32, y, letter, color);
            }
        }
        let (state, color) = if on {
            ("ON", Color::GOOD)
        } else {
            ("OFF", Color::TEXT_FAINT)
        };
        canvas.text_right(77, y, state, color);
    }
}

/// One setting, big, with how to change it.
pub(super) fn render_editor(canvas: &mut Canvas, editor: &Editor) {
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
    canvas.text_centered(40, 4, title, Color::ACCENT);

    // Two pairs of the clock's 2× digits (10 px cells, 2 px gaps), the
    // colon between; for settings, hours on the left half and minutes on
    // the right, matching where to swipe.
    let (pair, labels, color) = match left {
        Some(left) if left < 60 * 60 => ((left / 60, left % 60), ("MIN", "SEC"), Color::ACCENT),
        // Two digits of hours: a timer is at most `alarm::MAX_TIMER`, but
        // should more ever be left, it reads 99:59 rather than overflowing.
        Some(left) => {
            let left = left.min(100 * 3600 - 1);
            (
                (left / 3600, left / 60 % 60),
                ("HOURS", "MINUTES"),
                Color::ACCENT,
            )
        }
        None => (
            (i64::from(value / 60), i64::from(value % 60)),
            ("HOURS", "MINUTES"),
            Color::TEXT,
        ),
    };
    let digits = [pair.0 / 10, pair.0 % 10, pair.1 / 10, pair.1 % 10];
    for (&digit, x) in digits.iter().zip([12, 24, 44, 56]) {
        draw_digit(canvas, x, 22, digit as u64, 2, color);
    }
    for y in [26, 32] {
        canvas.fill_rect(39, y, 2, 2, Color::ACCENT);
    }
    canvas.text_centered(20, 44, labels.0, Color::TEXT_FAINT);
    canvas.text_centered(60, 44, labels.1, Color::TEXT_FAINT);
    let hint = if left.is_some() {
        "LEFT"
    } else {
        "SWIPE UP/DOWN"
    };
    canvas.text_centered(40, 51, hint, Color::TEXT_FAINT);

    let (left, left_color) = match (editing, on) {
        (Editing::Timer(_), true) => ("STOP", Color::BAD),
        (Editing::Timer(_), false) => ("START", Color::GOOD),
        (_, true) => ("ON", Color::GOOD),
        (_, false) => ("OFF", Color::TEXT_FAINT),
    };
    let right = if editing == Editing::SmartWeekday {
        "NEXT"
    } else {
        "DONE"
    };
    canvas.fill_rect(2, EDITOR_BUTTONS_Y, 36, 18, left_color);
    canvas.text_centered(20, EDITOR_BUTTONS_Y + 7, left, BACKGROUND);
    canvas.fill_rect(42, EDITOR_BUTTONS_Y, 36, 18, Color::ACCENT);
    canvas.text_centered(60, EDITOR_BUTTONS_Y + 7, right, BACKGROUND);
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
