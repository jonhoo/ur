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
// asleep and arrived when they woke (the two times at its ends), and its
// carriages are the night's stages, each as long as its share of the
// time asleep, in the stage's colour. Over it, how long that was, big,
// in the sky the wearer woke under. It lives on what it shows: a window
// is lit for each share of the night spent lying awake (dark windows,
// passengers asleep), the moon is the lower the later the wearer woke,
// and the stars fewer, as from 04:00 the sky was lightening (see
// `street::daylight`; no glow on the horizon here, where the train runs
// along it); and, while it's looked at, a star twinkles with the seconds.

/// The train's top row, the rail under it, and its length: the stages'
/// bar's, columns 4..76.
const TRAIN_Y: i32 = 53;
const TRAIN_H: i32 = 6;
const RAIL_Y: i32 = TRAIN_Y + TRAIN_H;
const TRAIN: i32 = 72;

/// The sky's stars, clear of the title, the moon and the figures; the
/// last go first as the morning comes.
const NIGHT_STARS: [(i32, i32); 7] = [
    (52, 4),
    (61, 9),
    (6, 16),
    (74, 37),
    (3, 30),
    (48, 11),
    (70, 46),
];

/// The last night's (or nap's) sleep: how long, big, the train from when
/// to when, and in which stages.
pub(super) fn render_last_night(canvas: &mut Canvas, sleep: Option<crate::activity::Sleep>) {
    canvas.clear(BACKGROUND);
    // When the wearer woke, in local time (as the offset is now), for
    // the times and the sky.
    let offset = crate::clock::utc_offset_minutes();
    let local =
        |utc_minute: i64| offset.map(|offset| (utc_minute + i64::from(offset)).rem_euclid(24 * 60));
    let woke = sleep.and_then(|sleep| local(sleep.end_utc_minute));
    let light = woke.map_or(0, |minute| street::daylight((minute / 60) as u8));
    let shown = NIGHT_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
    street::stars(canvas, &NIGHT_STARS[..shown]);
    // The twinkle: one of the stars still out, brighter, a second in
    // three; none in the dim look (the quiet night), which holds still.
    if let Some(now) = crate::clock::now()
        && shown > 0
        && now.second.is_multiple_of(3)
        && !street::dim(canvas)
    {
        let (x, y) = NIGHT_STARS[usize::from(now.minute) % shown];
        canvas.pixel(x, y, Color::TEXT);
    }
    // The moon, a crescent, high in the sky for a night that ended
    // before 04:00, a 4 px lower each hour after, down to the horizon's
    // haze by 10:00 (and high, with no time to place it by).
    let hours_after_four = woke.map_or(0, |minute| (minute / 60 - 4).clamp(0, 6) as i32);
    let moon_y = 2 + 4 * hours_after_four;
    street::disc(canvas, 69, moon_y, 9, Color::TEXT_DIM);
    street::disc(canvas, 66, moon_y - 1, 9, Color::BACKGROUND);
    street::title(canvas, "LAST NIGHT", Color::ACCENT);
    street::rails(canvas, 0, 80, RAIL_Y);

    let Some(sleep) = sleep else {
        // An empty line: no train has come in.
        canvas.text_centered(40, 36, "NO SLEEP YET", Color::TEXT_FAINT);
        return;
    };

    // Time asleep (not counting time lying awake) as H:MM, in the clock's
    // 3× digits, laid out as its are (2 px between digits, 7 px for the
    // colon), and centred.
    const TOP: i32 = 14;
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
    canvas.text_centered(40, 39, "ASLEEP", Color::TEXT_DIM);

    // The train's departure and arrival, at its ends: falling asleep and
    // waking, in local time.
    if let (Some(left), Some(arrived)) = (local(sleep.start_utc_minute), woke) {
        canvas.text(4, 46, &hhmm(left), Color::TEXT);
        canvas.text_right(76, 46, &hhmm(arrived), Color::TEXT);
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
            canvas.fill_rect(x, TRAIN_Y, width, TRAIN_H, color);
            // A carriage's windows: 2×2, a pillar of 2 between, from 2 in;
            // lit as `clock`'s are, by a permutation of their indices, so
            // that the lit share is spread down the train.
            let mut wx = x + 2;
            while wx + 2 < x + width {
                let hash = (window * 167 + 13) % 256;
                let pane = if hash < lit {
                    Color::HEART_SHINE
                } else {
                    Color::SHADOW
                };
                canvas.fill_rect(wx, TRAIN_Y + 1, 2, 2, pane);
                window += 1;
                wx += 4;
            }
            // The coupling between carriages: a gap a pixel wide.
            if x > 4 {
                canvas.fill_rect(x, TRAIN_Y, 1, TRAIN_H, BACKGROUND);
            }
        }
        x += width;
        let center = 16 + 24 * i as i32;
        canvas.text_centered(center, 67, name, color);
        canvas.text_centered(center, 74, &hhmm(i64::from(minutes)), Color::TEXT);
    }
    // The front, heading for the arrival: its roof's corner rounded off,
    // and a headlight.
    canvas.pixel(4 + TRAIN - 1, TRAIN_Y, BACKGROUND);
    canvas.pixel(4 + TRAIN - 1, TRAIN_Y + 3, Color::HEART_SHINE);
}
