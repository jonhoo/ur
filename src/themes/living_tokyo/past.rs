//! Living Tokyo's screens of the past: today, and last night.

use core::fmt::Write as _;

use super::{BACKGROUND, draw_digit, street};
use crate::canvas::{Canvas, Color};
use crate::day::Day;
use crate::font;
use crate::themes::helpers::hhmm;
use crate::ui::Fits;

// ==============================================================================
// Today: the day's steps as a city
// ==============================================================================

// The day so far as a street of buildings, one an hour from midnight, each
// as tall as that hour's steps; over their roofs, the heart rate's range
// hour by hour as a pink neon tube; and above, the day's figures: TODAY on
// its sign, the resting heart rate, and the steps, big. Nothing else: the
// city is the chart, and the dark round it is left empty, so the figures
// and the shapes read first.
//
// It lives on the hour. The hour now has a beacon on its roof, blinking
// with the seconds, and the hours to come are empty lots. After dark the
// buildings' windows light, as many as the city's are at that hour
// (`street::city_lights`, doubled, as these are near): most in the
// evening, a few in the small hours; by day, none. The night's weather
// (`street::rainy`) is quiet behind it: a few stars, fewer as the day
// comes, or a fine rain in the far buildings' slate. In the dim look, the
// buildings and the tube keep their shapes, and so the data, and stand
// still: no rain, lit window, nor blinking beacon.

/// Where the hours' buildings stand: one an hour from midnight, a lot
/// [`LOT`] wide from [`STREET_LEFT`], each [`BUILDING_W`] wide on it, so
/// that the 24 end at x = 75.
const STREET_LEFT: i32 = 4;
const LOT: i32 = 3;
const BUILDING_W: i32 = 2;
/// The street's row, the buildings standing on it, and the tallest
/// building's height: the busiest hour's, but at least [`QUIET_HOUR`]
/// steps' worth, so that a quiet day looks it.
const GROUND: i32 = 67;
const TALLEST: i32 = 20;
const QUIET_HOUR: u32 = 500;
/// The heart rate's tube: the rows its ranges span, on a scale from the
/// day's lowest reading to its highest in whole tens, but at least
/// [`TUBE_BPM`] tall, so that a calm day doesn't look like a wild one.
const TUBE_TOP: i32 = 31;
const TUBE_H: i32 = 14;
const TUBE_BPM: i32 = 40;
/// The day's steps, big, under the title, at 2×.
const STEPS_Y: i32 = 13;
const STEPS_SCALE: i32 = 2;
/// The hours' labels, under the street.
const HOURS_Y: i32 = 73;

/// The sky's stars, clear of the title, the steps and the tube: over it,
/// and under it, above the roofs of all but the busiest hours; the last
/// go first as the day comes.
const TODAY_STARS: [(i32, i32); 6] = [(27, 29), (58, 46), (8, 29), (73, 49), (49, 30), (14, 47)];

/// Today: the steps and the heart rate, as the day's city.
pub(super) fn render_today(canvas: &mut Canvas, day: Option<&Day>, steps: Option<u32>, dim: bool) {
    canvas.clear(BACKGROUND);
    let day = day.copied().unwrap_or_default();
    // The local time now, for the hour and the weather; without it
    // (before the phone sets the clock), no hour is now, nor to come.
    let now = street::local_now();
    let hour = now.map(street::hour_of);
    let rain = now.is_some_and(|second| street::rainy(street::evening(second)));
    let light = hour.map_or(0, street::daylight);
    let tick = now.map_or(0, |now| now as u64);
    let lot = |hour: usize| STREET_LEFT + LOT * hour as i32;

    // The weather, behind the tube and the roofs: stars on a clear night,
    // or a fine rain on a wet one, below the figures (it would break up
    // their letters).
    if !rain {
        let shown =
            TODAY_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
        street::fine_stars(canvas, &TODAY_STARS[..shown], None);
    } else if !dim {
        street::fine_rain(canvas, 3 * (TUBE_TOP - 2), 3 * GROUND, tick, Color::TRACK);
    }

    // The hours' buildings, their windows lit after dark: twice as many as
    // the city's at this hour (they're near, and the wearer's own); by
    // day, and in the dim look, none.
    let lit = if dim || light == street::DAYLIGHT {
        0
    } else {
        (hour.map_or(0, street::city_lights) * 2).min(256)
    };
    let most = u32::from(day.most_steps()).max(QUIET_HOUR);
    let mut window = 0;
    for (i, hour_of_day) in day.hours.iter().enumerate() {
        let x = lot(i);
        // An hour to come: an empty lot, its kerb a dark dash.
        if hour.is_some_and(|now| i > usize::from(now)) {
            canvas.fill_rect(x, GROUND, BUILDING_W, 1, Color::TRACK);
            continue;
        }
        let height = (u32::from(hour_of_day.steps) * TALLEST as u32).div_ceil(most) as i32;
        building(canvas, x, height, lit, &mut window);
        canvas.fill_rect(x, GROUND, LOT, 1, Color::TEXT_FAINT);
        // The hour now: a mast on its roof, and the beacon on it, lit
        // every other second (always, in the dim look, which holds still).
        if hour.is_some_and(|now| i == usize::from(now)) {
            let roof = GROUND - height;
            canvas.fill_rect(x, roof - 2, 1, 2, Color::TEXT_FAINT);
            if dim || tick.is_multiple_of(2) {
                canvas.pixel(x, roof - 3, Color::HEART);
            }
        }
    }
    for hour in [0, 6, 12, 18] {
        let mut label = heapless::String::<2>::new();
        write!(label, "{hour}").fits();
        canvas.text(lot(hour), HOURS_Y, &label, Color::TEXT_FAINT);
    }

    tube(canvas, &day);

    // The steps, big: the step counter's total (which, unlike the hours,
    // counts steps from before the phone set the clock), the foot before
    // them.
    const DIGIT: i32 = font::DIGIT_W as i32 * STEPS_SCALE;
    const DIGITS_X: i32 = 4 + font::FOOT_W as i32 * STEPS_SCALE + 3;
    canvas.sprite(
        4,
        STEPS_Y,
        font::FOOT_W,
        &font::FOOT,
        STEPS_SCALE,
        Color::TEXT_DIM,
    );
    let mut text = heapless::String::<10>::new();
    match steps {
        Some(steps) => write!(text, "{steps}").fits(),
        None => text.push_str("--").fits(),
    }
    let mut x = DIGITS_X;
    for c in text.chars() {
        match c.to_digit(10) {
            Some(digit) => draw_digit(
                canvas,
                x,
                STEPS_Y,
                u64::from(digit),
                STEPS_SCALE,
                Color::TEXT,
            ),
            None => {
                let y = STEPS_Y + 3 * STEPS_SCALE;
                canvas.fill_rect(x + 1, y + 1, DIGIT, STEPS_SCALE, Color::SHADOW);
                canvas.fill_rect(x, y, DIGIT, STEPS_SCALE, Color::TEXT);
            }
        }
        x += DIGIT + 2;
    }
    debug_assert!(x - 2 <= 77, "today's steps run off the screen");

    // TODAY on the watch's own sign, and the resting heart rate (see
    // `Day::resting`) across from it, plainly, as a label and a figure.
    let title_end = street::title(canvas, "TODAY", Color::ACCENT);
    let mut bpm = heapless::String::<3>::new();
    match day.resting() {
        Some(rate) => write!(bpm, "{rate}").fits(),
        None => bpm.push_str("--").fits(),
    }
    let figure = 76 - font::small_text_width(&bpm);
    canvas.text(figure, 3, &bpm, Color::TEXT);
    let label = figure - 4 - font::small_text_width("RESTING");
    debug_assert!(label > title_end + 2, "RESTING runs into TODAY");
    canvas.text(label, 3, "RESTING", Color::TEXT_DIM);
}

/// One of the hours' buildings, `height` rows tall on the street from `x`:
/// a slate tower with a column of windows up its left side, every other
/// row, lit (of 256) as `lit` says, counting from `window`.
fn building(canvas: &mut Canvas, x: i32, height: i32, lit: u32, window: &mut u32) {
    if height <= 0 {
        return;
    }
    let top = GROUND - height;
    canvas.fill_rect(x, top, BUILDING_W, height, Color::TRACK);
    let mut y = GROUND - 2;
    while y > top {
        // As the clock face's windows: a permutation of their indices, so
        // that `lit` lights its share wherever they are.
        let pane = if (*window * 167 + 13) % 256 < lit {
            Color::HEART_SHINE
        } else {
            Color::SHADOW
        };
        canvas.pixel(x, y, pane);
        *window += 1;
        y -= 2;
    }
}

/// The heart rate's range, hour by hour, as a neon tube over the roofs:
/// its top edge each hour's highest reading, its bottom edge the lowest,
/// joined from hour to hour, and closed where the readings stop.
fn tube(canvas: &mut Canvas, day: &Day) {
    let Some((low, high)) = day.heart_range() else {
        return;
    };
    let floor = i32::from(low) / 10 * 10;
    let ceiling = ((i32::from(high) + 9) / 10 * 10).max(floor + TUBE_BPM);
    // The row `bpm` is drawn in.
    let y = |bpm: u8| {
        TUBE_TOP + TUBE_H - 1 - (i32::from(bpm) - floor) * (TUBE_H - 1) / (ceiling - floor)
    };
    let range = |i: usize| {
        day.hours
            .get(i)
            .filter(|hour| hour.heart_max > 0)
            .map(|hour| (y(hour.heart_max), y(hour.heart_min)))
    };
    let span = |canvas: &mut Canvas, x: i32, a: i32, b: i32| {
        canvas.fill_rect(x, a.min(b), 1, (a - b).abs() + 1, Color::HEART);
    };
    for i in 0..day.hours.len() {
        let Some((top, bottom)) = range(i) else {
            continue;
        };
        let x = STREET_LEFT + LOT * i as i32;
        canvas.fill_rect(x, top, BUILDING_W, 1, Color::HEART);
        canvas.fill_rect(x, bottom, BUILDING_W, 1, Color::HEART);
        if i == 0 || range(i - 1).is_none() {
            span(canvas, x, top, bottom);
        }
        match range(i + 1) {
            Some((next_top, next_bottom)) => {
                span(canvas, x + BUILDING_W, top, next_top);
                span(canvas, x + BUILDING_W, bottom, next_bottom);
            }
            None => span(canvas, x + BUILDING_W - 1, top, bottom),
        }
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
// It lives on the night it shows, quietly: the rest of the screen is left
// dark, so the figures and the train read first. A carriage window is lit
// for each share of the night spent lying awake (dark windows, passengers
// asleep). The sky is the one the train came in under: the moon setting
// towards the rails the later the wearer woke, and the stars going out as
// the morning came (`street::daylight`). That night's weather
// (`street::rainy`, keyed to the evening it started): a fine rain in the
// far buildings' slate instead of the moon and stars. In the dim look (the
// quiet night), no rain.
//
// It once had the far city behind the train, the dawn's glow behind that,
// a twinkling star and the headlight's beam on the rails: each of them
// alive, all of them together too busy for the figures.

/// The train's top row, its height, the rail under it, and its length:
/// the stages' bar's, columns 4..76.
const TRAIN_Y: i32 = 56;
const TRAIN_H: i32 = 10;
const RAIL_Y: i32 = TRAIN_Y + TRAIN_H;
const TRAIN: i32 = 72;
/// The departure and arrival signs' top: in the sky over the train's
/// ends.
const BOARD_Y: i32 = 41;

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

    if !rain {
        let shown =
            NIGHT_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
        street::fine_stars(canvas, &NIGHT_STARS[..shown], None);
        // The moon, a crescent, setting: between the signs for a night
        // that ended by 04:00, 2 px lower each hour after, behind the
        // train by 10:00.
        let hours_after_four = i32::from(hour.saturating_sub(4).min(6));
        let moon_y = BOARD_Y + 2 * hours_after_four;
        street::fine_moon(canvas, 3 * 34, 3 * moon_y, 3 * 9, 25, Color::TEXT_DIM);
    }
    if rain && !dim {
        street::fine_rain(canvas, 0, 3 * TRAIN_Y, tick, Color::TRACK);
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
            street::fine_sign(
                canvas,
                3 * x,
                3 * BOARD_Y,
                3 * 23,
                3 * street::SIGN_H,
                Color::TEXT_FAINT,
            );
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
    // The front, heading for the arrival: a sloped nose and a headlight;
    // and the tail light, behind.
    let front = 4 + TRAIN - 1;
    canvas.fill_rect(front - 2, TRAIN_Y, 3, 1, BACKGROUND);
    canvas.pixel(front, TRAIN_Y + 1, BACKGROUND);
    canvas.pixel(front, TRAIN_Y + 5, Color::HEART_SHINE);
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
