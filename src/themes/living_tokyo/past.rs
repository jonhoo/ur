//! Living Tokyo's screens of the past: today, and last night.

use core::fmt::Write as _;

use super::{BACKGROUND, draw_digit, street};
use crate::canvas::{Canvas, Color};
use crate::day::Day;
use crate::font;
use crate::themes::helpers::hhmm;
use crate::ui::Fits;

/// Today's charts: a column per hour from midnight, [`CHART_BAR`] wide and
/// [`CHART_PITCH`] apart, from [`CHART_LEFT`] (24 of them end at x = 75).
const CHART_LEFT: i32 = 4;
const CHART_PITCH: i32 = 3;
const CHART_BAR: i32 = 2;

/// Today's steps and heart rate: for each, a figure for the day, then a
/// chart hour by hour since midnight, over a faint baseline that shows
/// how much of the day is left.
pub(super) fn render_today(canvas: &mut Canvas, day: Option<&Day>, steps: Option<u32>) {
    canvas.clear(BACKGROUND);
    canvas.text(4, 3, "TODAY", Color::ACCENT);
    // Figures end at x = 74, leaving the right edge to the hint.
    let day = day.copied().unwrap_or_default();
    let column = |hour: usize| CHART_LEFT + CHART_PITCH * hour as i32;

    // Steps: the step counter's total (which, unlike the history, counts
    // steps from before the phone set the clock), then the hours, scaled
    // to the busiest, but to at least 500 steps, so a quiet day looks it.
    const STEPS_BASE: i32 = 30;
    const STEPS_HEIGHT: u32 = 10;
    canvas.sprite(4, 11, font::FOOT_W, &font::FOOT, 1, Color::TEXT_DIM);
    canvas.text(10, 12, "STEPS", Color::TEXT_DIM);
    let mut text = heapless::String::<10>::new();
    match steps {
        Some(steps) => write!(text, "{steps}").fits(),
        None => text.push_str("--").fits(),
    }
    canvas.text_right(75, 12, &text, Color::TEXT);
    let most = u32::from(day.most_steps()).max(500);
    for (i, hour) in day.hours.iter().enumerate() {
        let height = (u32::from(hour.steps) * STEPS_HEIGHT).div_ceil(most) as i32;
        canvas.fill_rect(
            column(i),
            STEPS_BASE - height,
            CHART_BAR,
            height,
            Color::ACCENT,
        );
        canvas.fill_rect(column(i), STEPS_BASE, CHART_BAR, 1, Color::TRACK);
    }

    // The heart rate: resting (see `Day::resting`), then each hour's
    // range, from its lowest reading to its highest, on a scale from the
    // day's lowest to its highest in whole tens, but at least 40 BPM
    // tall, so a calm day doesn't look like a wild one.
    // TODO: the scale's ends as figures, if the shape alone isn't enough.
    const HEART_BASE: i32 = 64;
    const HEART_HEIGHT: i32 = 20;
    canvas.sprite(4, 35, font::HEART_W, &font::HEART, 1, Color::HEART);
    canvas.sprite(
        4,
        35,
        font::HEART_W,
        &font::HEART_SHINE,
        1,
        Color::HEART_SHINE,
    );
    canvas.text(13, 36, "RESTING", Color::TEXT_DIM);
    let mut text = heapless::String::<3>::new();
    match day.resting() {
        Some(bpm) => write!(text, "{bpm}").fits(),
        None => text.push_str("--").fits(),
    }
    canvas.text_right(75, 36, &text, Color::TEXT);
    if let Some((low, high)) = day.heart_range() {
        let floor = i32::from(low) / 10 * 10;
        let ceiling = ((i32::from(high) + 9) / 10 * 10).max(floor + 40);
        // The top of the row `bpm` is drawn in.
        let y = |bpm: u8| {
            HEART_BASE - 1 - (i32::from(bpm) - floor) * (HEART_HEIGHT - 1) / (ceiling - floor)
        };
        for (i, hour) in day.hours.iter().enumerate() {
            if hour.heart_max > 0 {
                let top = y(hour.heart_max);
                let height = y(hour.heart_min) - top + 1;
                canvas.fill_rect(column(i), top, CHART_BAR, height, Color::HEART);
            }
        }
    }
    for i in 0..day.hours.len() {
        canvas.fill_rect(column(i), HEART_BASE, CHART_BAR, 1, Color::TRACK);
    }
    for hour in [0, 6, 12, 18] {
        let mut label = heapless::String::<2>::new();
        write!(label, "{hour}").fits();
        canvas.text(column(hour), HEART_BASE + 4, &label, Color::TEXT_FAINT);
    }
}

// ==============================================================================
// Last night: the sleeper train
// ==============================================================================

// The night as an overnight sleeper train: it left when the wearer fell
// asleep and arrived when they woke (the two times on station signs at
// its ends), and its carriages are the night's stages, each as long as
// its share of the time asleep, in the stage's colour. Over it, how long
// that was, big, and the sky the wearer woke under.
//
// It lives on the night it shows. A carriage window is lit for each share
// of the night spent lying awake (dark windows, passengers asleep). The
// sky is the one the train came in under: the moon setting behind the far
// city the later the wearer woke, the city's windows as lit as its people
// were up then (`street::city_lights`), and from 04:00 the dawn's glow
// behind it, higher each hour (`street::horizon`). That night's weather
// (`street::rainy`, keyed to the evening it started): rain, or stars, one
// twinkling with the seconds. In the dim look (the quiet night), no rain,
// no glow, no twinkle and no far city.

/// The train's top row, its height, the rail under it, and its length:
/// the stages' bar's, columns 4..76.
const TRAIN_Y: i32 = 56;
const TRAIN_H: i32 = 10;
const RAIL_Y: i32 = TRAIN_Y + TRAIN_H;
const TRAIN: i32 = 72;
/// The departure and arrival signs' top: in the sky over the train's
/// ends.
const BOARD_Y: i32 = 41;
/// The far city's tallest building, behind the train.
const NIGHT_CITY: i32 = 12;

/// The sky's stars, clear of the title, the figures and the signs; the
/// last go first as the morning comes.
const NIGHT_STARS: [(i32, i32); 9] = [
    (52, 4),
    (61, 9),
    (6, 16),
    (74, 35),
    (3, 30),
    (48, 11),
    (29, 44),
    (70, 13),
    (52, 46),
];

/// The last night's (or nap's) sleep: how long, big, the train from when
/// to when, and in which stages.
pub(super) fn render_last_night(
    canvas: &mut Canvas,
    sleep: Option<crate::activity::Sleep>,
    dim: bool,
) {
    canvas.clear(BACKGROUND);
    // Local seconds (as the offset is now) of falling asleep and waking,
    // for the times, the sky and the weather.
    let offset = crate::clock::utc_offset_minutes();
    let local = |utc_minute: i64| offset.map(|offset| (utc_minute + i64::from(offset)) * 60);
    let left = sleep.and_then(|sleep| local(sleep.start_utc_minute));
    let woke = sleep.and_then(|sleep| local(sleep.end_utc_minute));
    // With no night to show, tonight's.
    let now = street::local_now();
    let hour = woke.or(now).map_or(0, street::hour_of);
    let rain = left
        .or(now)
        .is_some_and(|second| street::rainy(street::evening(second)));
    let light = street::daylight(hour);
    let tick = now.map_or(0, |now| now as u64);

    if !dim {
        // Three levels at most: the fourth's would reach ASLEEP.
        street::horizon(canvas, 0, 80, TRAIN_Y, light.min(3));
    }
    if !rain {
        let shown =
            NIGHT_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
        street::stars(canvas, &NIGHT_STARS[..shown]);
        if !dim && shown > 0 && tick.is_multiple_of(3) {
            let (x, y) = NIGHT_STARS[(tick / 3) as usize % shown];
            canvas.pixel(x, y, Color::TEXT);
        }
        // The moon, a crescent, setting: over the far city for a night
        // that ended by 04:00, 2 px lower each hour after, behind its
        // roofs by 10:00.
        let hours_after_four = i32::from(hour.saturating_sub(4).min(6));
        let moon_y = BOARD_Y + 2 * hours_after_four;
        street::disc(canvas, 36, moon_y, 9, Color::TEXT_DIM);
        street::disc(canvas, 33, moon_y - 1, 9, BACKGROUND);
    }
    // The far city, but not in the dim look, where its half-toned walls
    // and lit windows would be a busy band in idle mode's few colours.
    if !dim {
        street::skyline(
            canvas,
            0,
            80,
            TRAIN_Y,
            NIGHT_CITY,
            street::city_lights(hour),
        );
    }
    if rain && !dim {
        street::rain(canvas, 0, TRAIN_Y, tick, Color::TEXT_FAINT);
        // Behind ASLEEP, whose small letters it would break up; the big
        // digits stand it.
        canvas.fill_rect(26, 34, 28, 7, BACKGROUND);
    }
    street::title(canvas, "LAST NIGHT", Color::ACCENT);
    street::rails(canvas, 0, 80, RAIL_Y);

    let Some(sleep) = sleep else {
        // An empty line: no train has come in.
        canvas.text_centered(40, 28, "NO SLEEP YET", Color::TEXT_FAINT);
        return;
    };

    // Time asleep (not counting time lying awake) as H:MM, in the clock's
    // 3× digits, laid out as its are (2 px between digits, 7 px for the
    // colon), and centred.
    const TOP: i32 = 12;
    const SCALE: i32 = 3;
    const DIGIT: i32 = font::DIGIT_W as i32 * SCALE;
    let (hours, minutes) = (sleep.asleep_minutes / 60, sleep.asleep_minutes % 60);
    let mut digits = heapless::Vec::<u16, 4>::new();
    if hours >= 10 {
        let _ = digits.push(hours / 10 % 10);
    }
    let _ = digits.push(hours % 10);
    let _ = digits.push(minutes / 10);
    let _ = digits.push(minutes % 10);
    let width = DIGIT * digits.len() as i32 + 2 * (digits.len() as i32 - 2) + 7;
    let mut x = 40 - width / 2;
    let colon_after = digits.len() - 2;
    for (i, &digit) in digits.iter().enumerate() {
        if i == colon_after {
            for row in [2, 4] {
                let y = TOP + row * SCALE;
                canvas.fill_rect(x + 1, y + 1, 3, 3, Color::SHADOW);
                canvas.fill_rect(x, y, 3, 3, Color::ACCENT);
            }
            x += 3 + 2;
        }
        draw_digit(canvas, x, TOP, u64::from(digit), SCALE, Color::TEXT);
        x += DIGIT + 2;
    }
    canvas.text_centered(40, 35, "ASLEEP", Color::TEXT_DIM);

    // The train's departure and arrival, on station signs over its ends:
    // falling asleep and waking, in local time.
    if let (Some(left), Some(woke)) = (left, woke) {
        let minute = |second: i64| second.div_euclid(60).rem_euclid(24 * 60);
        for (x, second) in [(2, left), (55, woke)] {
            street::sign(canvas, x, BOARD_Y, 23, street::SIGN_H, Color::TEXT_FAINT);
            canvas.text(x + 2, BOARD_Y + 2, &hhmm(minute(second)), Color::TEXT);
        }
    }

    // The stages (see `crate::stages`): their shares of the time asleep as
    // the train's carriages, and each one's time below its name, in that
    // stage's colour (roles of their own: the general ones collide in
    // some themes). The stages are parts of the time asleep
    // (`stages::Stages::asleep` is their sum), so the first two's shares
    // of the train leave the third its own; were they more, it would run
    // off the screen.
    debug_assert!(
        u32::from(sleep.deep_minutes) + u32::from(sleep.rem_minutes)
            <= u32::from(sleep.asleep_minutes),
        "more deep and REM sleep than sleep"
    );
    let light_sleep = sleep
        .asleep_minutes
        .saturating_sub(sleep.deep_minutes + sleep.rem_minutes);
    let stages = [
        ("DEEP", sleep.deep_minutes, Color::DEEP),
        ("REM", sleep.rem_minutes, Color::REM),
        ("LIGHT", light_sleep, Color::GREY),
    ];
    // How much of the night, from falling asleep to waking, was spent
    // lying awake, out of 256: that share of the windows is lit.
    let night = (sleep.end_utc_minute - sleep.start_utc_minute).max(1);
    let awake = (night - i64::from(sleep.asleep_minutes)).clamp(0, night);
    let lit = (awake * 256 / night) as u32;
    let total = i32::from(sleep.asleep_minutes.max(1));
    let mut x = 4;
    let mut window: u32 = 0;
    for (i, &(name, minutes, color)) in stages.iter().enumerate() {
        // The last takes what's left, so rounding can't leave a gap.
        let width = if i + 1 == stages.len() {
            4 + TRAIN - x
        } else {
            i32::from(minutes) * TRAIN / total
        };
        if width > 0 {
            carriage(canvas, x, width, color, lit, &mut window);
        }
        x += width;
        let center = 16 + 24 * i as i32;
        canvas.text_centered(center, 69, name, color);
        canvas.text_centered(center, 75, &hhmm(i64::from(minutes)), Color::TEXT);
    }
    // The front, heading for the arrival: a sloped nose, a headlight,
    // and its beam on the rails ahead; and the tail light, behind.
    let front = 4 + TRAIN - 1;
    canvas.fill_rect(front - 2, TRAIN_Y, 3, 1, BACKGROUND);
    canvas.pixel(front, TRAIN_Y + 1, BACKGROUND);
    canvas.pixel(front, TRAIN_Y + 5, Color::HEART_SHINE);
    for dx in 1..80 - front {
        for row in 4..7 {
            if (dx + row) % 2 == 0 {
                canvas.pixel(front + dx, TRAIN_Y + row, Color::HEART_SHINE);
            }
        }
    }
    canvas.pixel(4, TRAIN_Y + 5, Color::HEART);
}

/// One of the train's carriages, `width` wide from `x`, in a stage's
/// `color`: a rounded roof, a row of windows (lit, of 256, as `lit` says,
/// counting from `window`), a dark livery stripe, the skirt, and its
/// wheels on the rail, with a coupling's gap before it unless it's the
/// last carriage (the train's left end).
fn carriage(canvas: &mut Canvas, x: i32, width: i32, color: Color, lit: u32, window: &mut u32) {
    let y = TRAIN_Y;
    canvas.fill_rect(x, y + 1, width, 7, color);
    if width > 2 {
        canvas.fill_rect(x + 1, y, width - 2, 1, color);
    }
    // Windows: 2×3, a pillar of 2 between, from 2 in; lit as `clock`'s
    // are, by a permutation of their indices, so that the lit share is
    // spread down the train.
    let mut wx = x + 2;
    while wx + 2 < x + width {
        let pane = if (*window * 167 + 13) % 256 < lit {
            Color::HEART_SHINE
        } else {
            Color::SHADOW
        };
        canvas.fill_rect(wx, y + 2, 2, 3, pane);
        *window += 1;
        wx += 4;
    }
    canvas.fill_rect(x, y + 6, width, 1, Color::SHADOW);
    canvas.fill_rect(x, y + 8, width, 2, Color::SHADOW);
    let mut wheel = x + 2;
    while wheel + 2 <= x + width {
        canvas.fill_rect(wheel, y + 9, 2, 1, Color::TEXT_FAINT);
        wheel += 6;
    }
    if x > 4 {
        canvas.fill_rect(x, y, 1, TRAIN_H - 2, BACKGROUND);
    }
}
