//! Living Tokyo's screens of the past: today, and last night, drawn at
//! the panel's resolution (see DESIGN.md's Resolution): every coordinate
//! here is the panel's, 0..240, but for the small labels' text, which
//! stays on the canvas's grid.

use core::fmt::Write as _;

use super::{BACKGROUND, street};
use crate::canvas::{Canvas, Color};
use crate::day::{Day, HOURS};
use crate::font;
use crate::font::fine::Style;
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
// A building is floors of two windows, as tall as the square root of its
// hour's steps: the busiest hour (or a quiet day's ceiling) sixteen
// floors, a quarter of its steps half as tall. A linear scale left a day
// with one busy hour (a run, a walk to work) as one tower among stubs a
// row high; the root keeps a quiet hour's few hundred steps a house of a
// few floors, still plainly lower. What's linear is its lit windows: a
// building of n floors has n sixteenths of its 2n windows lit, n²/8, in
// proportion to its steps, so the street's lit windows add up as the
// steps do.
//
// It lives on the hour. The hour now has a mast on its roof, its beacon
// blinking with the seconds; the hours to come are empty lots behind a
// fence. After dark (by `street::daylight`) the windows light; by day
// they're dark glass. The night's weather (`street::rainy`) is quiet
// behind it: a few stars, fewer as the day comes, or a fine rain in the
// far buildings' slate. In the dim look the buildings and the tube keep
// their shapes, and so the data, and stand still: no rain, lit window,
// nor blinking beacon.

/// Where the hours' lots are: one an hour from midnight, [`LOT`] wide from
/// [`STREET_LEFT`], each with a building [`BUILDING_W`] wide on it (two
/// windows across, a pixel from either side and two between them) and the
/// lot's last column a gap to the next. The 24 end at 228.
const STREET_LEFT: i32 = 12;
const LOT: i32 = 9;
const BUILDING_W: i32 = 8;
/// The street's row, the buildings standing on it; a floor's height (two
/// rows of windows, two of the slab between); and the most floors, the
/// busiest hour's (but at least [`QUIET_HOUR`] steps' worth, so that a
/// quiet day looks it).
const GROUND: i32 = 204;
const FLOOR: i32 = 4;
const FLOORS: u32 = 16;
const QUIET_HOUR: u32 = 2500;
/// The heart rate's tube: the rows its ranges span, on a scale from the
/// day's lowest reading to its highest in whole tens, but at least
/// [`TUBE_BPM`] tall, so that a calm day doesn't look like a wild one.
const TUBE_TOP: i32 = 92;
const TUBE_H: i32 = 36;
const TUBE_BPM: i32 = 40;
/// The day's steps, big, in the city's lettering, under the title: the
/// foot at the left, the figure after it.
const STEPS: Style = Style {
    cap: 32,
    stroke: 72,
    caps: true,
};
const STEPS_Y: i32 = 42;
const STEPS_X: i32 = 40;
/// The footprint before them: its toes, from the big one, its sole and
/// its heel, each an ellipse's box (from the figure's top).
const FOOT: [(i32, i32, i32, i32); 6] = [
    (13, 0, 6, 7),
    (20, 2, 4, 4),
    (25, 4, 3, 4),
    (28, 8, 3, 3),
    (13, 9, 15, 15),
    (15, 25, 11, 10),
];
/// The hours' labels, under the street (the canvas's rows).
const HOURS_Y: i32 = 71;

/// The sky's stars (canvas pixels), clear of the title, the steps and the
/// tube: over it, and under it, above the roofs of all but the busiest
/// hours; the last go first as the day comes.
const TODAY_STARS: [(i32, i32); 6] = [(27, 28), (58, 44), (8, 28), (73, 47), (49, 29), (14, 45)];

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

    // The weather, behind the tube and the roofs: stars on a clear night,
    // or a fine rain on a wet one, below the figures (it would break up
    // their letters).
    if !rain {
        let shown =
            TODAY_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
        street::fine_stars(canvas, &TODAY_STARS[..shown], None);
    } else if !dim {
        street::fine_rain(canvas, TUBE_TOP - 6, GROUND, tick, Color::TRACK);
    }

    // The street: the pavement under the hours gone, a darker one under
    // the lots still to come.
    let most = u32::from(day.most_steps()).max(QUIET_HOUR);
    let lit = !dim && light < street::DAYLIGHT;
    for (i, hour_of_day) in day.hours.iter().enumerate() {
        let x = STREET_LEFT + LOT * i as i32;
        let to_come = hour.is_some_and(|now| i > usize::from(now));
        let pavement = if to_come {
            Color::TRACK
        } else {
            Color::TEXT_FAINT
        };
        canvas.fine_rect(x, GROUND, LOT, 1, pavement);
        // An hour to come: an empty lot, fenced, its mesh a faint tone
        // between a post at either end.
        if to_come {
            canvas.fine_tone(x + 1, GROUND - 6, BUILDING_W - 2, 6, 1, Color::TEXT_FAINT);
            for post in [x, x + BUILDING_W - 1] {
                canvas.fine_rect(post, GROUND - 7, 1, 7, Color::TRACK);
            }
            continue;
        }
        // Its floors: the root of its share of the busiest hour's steps,
        // in sixteenths, rounded up, so that any steps at all are a floor.
        let share = u32::from(hour_of_day.steps) * FLOORS * FLOORS;
        let floors = share.div_ceil(most).isqrt();
        let floors = (floors + u32::from(floors * floors < share.div_ceil(most))) as i32;
        let roof = GROUND - FLOOR * floors - 3;
        if floors > 0 {
            building(canvas, x, roof, i as u32, lit.then_some(16 * floors as u32));
            // A water tank on every third roof from the second, as on
            // Tokyo's older blocks: a few, at no rhythm the eye picks up.
            if i % 3 == 1 && Some(i) != hour.map(usize::from) {
                canvas.fine_rect(x + 2, roof - 5, 4, 3, Color::TRACK);
                canvas.fine_rect(x + 2, roof - 5, 4, 1, Color::TEXT_FAINT);
                canvas.fine_rect(x + 2, roof - 2, 1, 2, Color::TRACK);
                canvas.fine_rect(x + 5, roof - 2, 1, 2, Color::TRACK);
            }
        }
        // The hour now: a lattice mast on its roof (or its bare lot), its
        // legs leaning in to the top, and the beacon on it, lit every
        // other second (always, in the dim look, which holds still),
        // hazing when it is.
        if hour.is_some_and(|now| i == usize::from(now)) {
            let base = roof.min(GROUND) - 1;
            let top = base - 12;
            for leg in [x + 1, x + 5] {
                street::fine_line(canvas, (leg, base), (x + 3, top), Color::TEXT_FAINT);
            }
            if dim || tick.is_multiple_of(2) {
                if !dim {
                    canvas.fine_tone(x, top - 6, 7, 7, 1, Color::HEART);
                }
                canvas.fine_rect(x + 2, top - 3, 3, 2, Color::HEART);
                canvas.fine_rect(x + 3, top - 4, 1, 4, Color::HEART);
            } else {
                canvas.fine_pixel(x + 3, top - 2, Color::TEXT_FAINT);
            }
        }
    }
    for hour in [0, 6, 12, 18] {
        let mut label = heapless::String::<2>::new();
        write!(label, "{hour}").fits();
        canvas.text(
            (STREET_LEFT + LOT * hour) / 3,
            HOURS_Y,
            &label,
            Color::TEXT_FAINT,
        );
    }

    tube(canvas, &day);

    // The steps, big: the step counter's total (which, unlike the hours,
    // counts steps from before the phone set the clock), a footprint
    // before them. In the city's lettering, though on a walk they can
    // change each second, and a fine row that changes is sent whole (see
    // `Canvas::refine`): the screen is mostly read standing still, and
    // their bands are drawn finely anyway (the title's sign, the stars).
    for (x, y, w, h) in FOOT {
        street::fine_ellipse(canvas, x, STEPS_Y + y, w, h, Color::TEXT_DIM);
    }
    let mut text = heapless::String::<10>::new();
    match steps {
        Some(steps) => write!(text, "{steps}").fits(),
        None => text.push_str("--").fits(),
    }
    let edge = (!dim).then_some(Color::TEXT_FAINT);
    let end = canvas.fine_type(STEPS_X, STEPS_Y, &text, STEPS, Color::TEXT, edge);
    debug_assert!(end <= 240, "today's steps run off the screen");

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

/// Hour `hour`'s building, from column `x` and its roof at row `top` down
/// to the street: a slate wall, a parapet along its roof, and its windows,
/// two a floor, each 2×2, lit (of 256, by [`street::lit_window`]) as
/// `lit` says, or dark; by day and in the dim look (`None`), all dark.
fn building(canvas: &mut Canvas, x: i32, top: i32, hour: u32, lit: Option<u32>) {
    canvas.fine_rect(x, top, BUILDING_W, GROUND - top, Color::TRACK);
    canvas.fine_rect(x, top, BUILDING_W, 1, Color::TEXT_FAINT);
    // Only the floors being drawn: the rest are walked past.
    let rows = canvas.fine_rows();
    let mut y = top + 3;
    let mut window = hour * 32;
    while y < GROUND.min(rows.end) {
        for column in [1, 5] {
            let pane = match lit {
                Some(lit) if street::lit_window(window, lit) => Color::HEART_SHINE,
                _ => Color::SHADOW,
            };
            if y + 2 > rows.start {
                canvas.fine_rect(x + column, y, 2, 2, pane);
            }
            window += 1;
        }
        y += FLOOR;
    }
}

/// The heart rate's range, hour by hour, as a neon tube over the roofs:
/// its top edge each hour's highest reading, its bottom edge the lowest,
/// bent from hour to hour (at each lot's middle), and closed where the
/// readings start and stop, half a lot on. Its edges are the tube's glass,
/// lit; between them a faint tone of the same pink, the gas's glow, so
/// that a steady hour is a bright line and a wild one a wide, hazy tube.
///
/// Drawn a column at a time, each edge joined to the next column's, so
/// that a steep bend stays a line.
fn tube(canvas: &mut Canvas, day: &Day) {
    // Only in the passes that draw its rows (see `Canvas::refine`): its
    // columns' walk is most of the screen's drawing.
    let rows = canvas.fine_rows();
    if rows.end <= TUBE_TOP || rows.start >= TUBE_TOP + TUBE_H {
        return;
    }
    let Some((low, high)) = day.heart_range() else {
        return;
    };
    let floor = i32::from(low) / 10 * 10;
    let ceiling = ((i32::from(high) + 9) / 10 * 10).max(floor + TUBE_BPM);
    // Each hour's highest and lowest readings' rows, worked out once, as
    // bytes (the tube is in rows 92..128), to keep it small on the stack.
    let mut readings = [None; HOURS];
    for (reading, hour) in readings.iter_mut().zip(&day.hours) {
        let y = |bpm: u8| {
            (TUBE_TOP + TUBE_H - 1 - (i32::from(bpm) - floor) * (TUBE_H - 1) / (ceiling - floor))
                as u8
        };
        if hour.heart_max > 0 {
            *reading = Some((y(hour.heart_max), y(hour.heart_min)));
        }
    }
    let at = |i: i32| {
        let (top, bottom) = (*readings.get(usize::try_from(i).ok()?)?)?;
        Some((i32::from(top), i32::from(bottom)))
    };
    // The tube's edges at column `x`: between two hours' middles, from
    // one's to the other's; past the last of a run, flat, for 3 columns.
    let edges = |x: i32| {
        let i = (x - centre(0)).div_euclid(LOT);
        let from = centre(i);
        match (at(i), at(i + 1)) {
            (Some((top, bottom)), Some((next_top, next_bottom))) => {
                let along = |a: i32, b: i32| a + (b - a) * (x - from) / LOT;
                Some((along(top, next_top), along(bottom, next_bottom)))
            }
            (Some(edges), None) if x - from <= 3 => Some(edges),
            (None, Some(edges)) if from + LOT - x <= 3 => Some(edges),
            _ => None,
        }
    };
    let (mut before, mut here) = (None, edges(STREET_LEFT));
    for x in STREET_LEFT..STREET_LEFT + LOT * HOURS as i32 {
        let after = edges(x + 1);
        if let Some((top, bottom)) = here {
            let (next_top, next_bottom) = after.unwrap_or((top, bottom));
            let (high, low) = (top.min(next_top), bottom.max(next_bottom));
            let low_edge = bottom.min(next_bottom);
            canvas.fine_tone(x, high, 1, low - high + 1, 1, Color::HEART);
            canvas.fine_rect(x, high, 1, (top - next_top).abs() + 1, Color::HEART);
            canvas.fine_rect(
                x,
                low_edge,
                1,
                (bottom - next_bottom).abs() + 1,
                Color::HEART,
            );
            // A run's ends, closed.
            if before.is_none() || after.is_none() {
                canvas.fine_rect(x, top, 1, bottom - top + 1, Color::HEART);
            }
        }
        (before, here) = (here, after);
    }
}

/// The middle of hour `i`'s lot.
fn centre(i: i32) -> i32 {
    STREET_LEFT + LOT * i + BUILDING_W / 2
}

// ==============================================================================
// Last night: the sleeper train
// ==============================================================================

// The night as an overnight sleeper train: it left when the wearer fell
// asleep and arrived when they woke (the two times on station signs, on
// their posts behind its ends), and its carriages are the night's stages,
// each stage as long as its share of the time asleep, in the stage's
// colour, a carriage or two of it. Over it, how long that was, big, and
// the sky the wearer woke under.
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

/// The train: its length (the stages' bar's, from [`TRAIN_X`]), its roof's
/// row, its body's height, and the rail its wheels run on.
const TRAIN_X: i32 = 12;
const TRAIN: i32 = 216;
const TRAIN_Y: i32 = 168;
const BODY_H: i32 = 24;
const RAIL_Y: i32 = 198;
/// How long a carriage is at most: a stage longer is coupled carriages.
const CARRIAGE: i32 = 76;
/// The departure and arrival signs: their boxes' top, size and lefts.
const BOARD_Y: i32 = 123;
const BOARD_W: i32 = 69;
const BOARD_H: i32 = 27;
const BOARDS_X: [i32; 2] = [6, 165];

/// How long the night's sleep was, big, in the city's lettering.
const ASLEEP: Style = Style {
    cap: 46,
    stroke: 96,
    caps: true,
};
const ASLEEP_Y: i32 = 38;

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
    let rain = left
        .or(now)
        .is_some_and(|second| street::rainy(street::evening(second)));
    let minute = woke
        .or(now)
        .map_or(0, |second| second.div_euclid(60) % (24 * 60));
    let light = street::daylight((minute / 60) as u8);
    let tick = now.map_or(0, |now| now as u64);

    if !rain {
        let shown =
            NIGHT_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
        street::fine_stars(canvas, &NIGHT_STARS[..shown], None);
        // The moon, a crescent, setting: between the signs for a night
        // that ended by 04:00, a pixel lower every ten minutes after,
        // behind the train by 10:00.
        let set = (minute as i32 - 4 * 60).clamp(0, 6 * 60) / 10;
        street::fine_moon(canvas, 102, BOARD_Y + set, 27, 25, Color::TEXT_DIM);
    }
    if rain && !dim {
        street::fine_rain(canvas, 0, TRAIN_Y, tick, Color::TRACK);
        // Behind ASLEEP, whose small letters it would break up; the big
        // figures stand it.
        canvas.fill_rect(26, 34, 28, 7, BACKGROUND);
    }
    street::title(canvas, "LAST NIGHT", Color::ACCENT);
    // The rail, and the sleepers' ends under it.
    canvas.fine_rect(0, RAIL_Y, 240, 2, Color::TEXT_FAINT);
    for x in (3..240).step_by(9) {
        canvas.fine_rect(x, RAIL_Y + 2, 5, 2, Color::TRACK);
    }

    let Some(sleep) = sleep else {
        // An empty line: no train has come in.
        canvas.text_centered(40, 28, "NO SLEEP YET", Color::TEXT_FAINT);
        return;
    };

    // Time asleep (not counting time lying awake) as H:MM, centred, the
    // colon in the watch's blue, as the clock's.
    let edge = (!dim).then_some(Color::TEXT_FAINT);
    let face = font::Face::Fine(ASLEEP);
    let (hours, minutes) = (sleep.asleep_minutes / 60, sleep.asleep_minutes % 60);
    let mut h = heapless::String::<5>::new();
    let mut m = heapless::String::<2>::new();
    write!(h, "{hours}").fits();
    write!(m, "{minutes:02}").fits();
    let gap = ASLEEP.gap();
    let width = face.width(&h) + face.width(":") + face.width(&m) + 2 * gap;
    let x = 120 - width / 2;
    let x = canvas.fine_type(x, ASLEEP_Y, &h, ASLEEP, Color::TEXT, edge);
    let x = canvas.fine_type(x, ASLEEP_Y, ":", ASLEEP, Color::ACCENT, None);
    canvas.fine_type(x, ASLEEP_Y, &m, ASLEEP, Color::TEXT, edge);
    canvas.text_centered(40, 35, "ASLEEP", Color::TEXT_DIM);

    // The train's departure and arrival, on station signs on two posts
    // each, behind its ends: falling asleep and waking, in local time.
    if let (Some(left), Some(woke)) = (left, woke) {
        let minute = |second: i64| second.div_euclid(60).rem_euclid(24 * 60);
        for (x, second) in BOARDS_X.into_iter().zip([left, woke]) {
            for post in [x + 10, x + BOARD_W - 11] {
                let foot = BOARD_Y + BOARD_H;
                canvas.fine_rect(post, foot, 2, TRAIN_Y - foot, Color::TEXT_FAINT);
            }
            street::fine_sign(canvas, x, BOARD_Y, BOARD_W, BOARD_H, Color::TEXT_FAINT);
            let time = hhmm(minute(second));
            let w = font::Face::Fine(street::LETTERING).width(&time);
            let y = BOARD_Y + (BOARD_H - street::LETTERING.height()) / 2;
            canvas.fine_type(
                x + (BOARD_W - w) / 2,
                y,
                &time,
                street::LETTERING,
                Color::TEXT,
                edge,
            );
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
    let mut x = TRAIN_X;
    let mut window: u32 = 0;
    // The front carriage's colour, for its nose's outline.
    let mut nose = Color::GREY;
    for (i, &(name, minutes, color)) in stages.iter().enumerate() {
        // The last takes what's left, so rounding can't leave a gap; a
        // stage longer than a carriage is several, of a length.
        let width = if i + 1 == stages.len() {
            TRAIN_X + TRAIN - x
        } else {
            i32::from(minutes) * TRAIN / total
        };
        let cars = (width + CARRIAGE - 1) / CARRIAGE;
        if cars > 0 {
            nose = color;
        }
        for car in 0..cars {
            let from = x + width * car / cars;
            let to = x + width * (car + 1) / cars;
            carriage(canvas, (from, to - from), color, lit, &mut window, dim);
        }
        x += width;
        let center = 16 + 24 * i as i32;
        canvas.text_centered(center, 69, name, color);
        canvas.text_centered(center, 75, &hhmm(i64::from(minutes)), Color::TEXT);
    }
    // The front, heading for the arrival: its nose sloping back to the
    // roof, a windscreen along the slope, and a headlight; and the tail
    // light, behind.
    let front = TRAIN_X + TRAIN;
    for row in 0..12 {
        let cut = (12 - row) * 3 / 2;
        canvas.fine_rect(front - cut, TRAIN_Y + row, cut, 1, BACKGROUND);
        canvas.fine_pixel(front - cut - 1, TRAIN_Y + row, nose);
        canvas.fine_rect(front - cut - 5, TRAIN_Y + row, 3, 1, Color::SHADOW);
    }
    canvas.fine_rect(front - 6, TRAIN_Y + 15, 4, 2, Color::HEART_SHINE);
    canvas.fine_rect(TRAIN_X + 2, TRAIN_Y + 15, 3, 2, Color::HEART);
}

/// One of the train's carriages, `width` wide from `x`, in a stage's
/// `color`: a rounded roof, a row of windows (lit, of 256, as `lit` says,
/// counting from `window`), a dark livery stripe under them, the
/// underframe and two bogies of wheels on the rail (one, under a short
/// carriage); and a gangway's bellows in the gap before it, unless it's
/// the first carriage (the train's left end). In the dim look, its body
/// an outline round the lit windows: filled, the light sleep's grey (a
/// half-tone there) was noise.
fn carriage(
    canvas: &mut Canvas,
    (x, width): (i32, i32),
    color: Color,
    lit: u32,
    window: &mut u32,
    dim: bool,
) {
    let (y, body) = (TRAIN_Y, x + 1..x + width - 1);
    let w = body.len() as i32;
    if w < 4 {
        return;
    }
    if x > TRAIN_X {
        canvas.fine_rect(x - 1, y + 5, 3, 14, Color::SHADOW);
    }
    canvas.fine_rect(body.start + 2, y, w - 4, 1, color);
    canvas.fine_rect(body.start, y + 1, w, BODY_H - 1, color);
    if dim {
        canvas.fine_rect(body.start + 1, y + 1, w - 2, BODY_H - 2, Color::SHADOW);
    } else {
        // The roof's curve, shaded.
        canvas.fine_tone(body.start + 1, y + 1, w - 2, 2, 2, Color::SHADOW);
    }
    // Windows: 6×7, a pillar of 3 between, from 4 in; lit as `clock`'s
    // are, by a permutation of their indices, so that the lit share is
    // spread down the train.
    let mut wx = body.start + 4;
    while wx + 6 + 4 <= body.end {
        let pane = if street::lit_window(*window, lit) {
            Color::HEART_SHINE
        } else {
            Color::SHADOW
        };
        canvas.fine_rect(wx, y + 5, 6, 7, pane);
        *window += 1;
        wx += 9;
    }
    // The stripe and the underframe; in the dim look, the outline's dark
    // inside is them.
    if !dim {
        canvas.fine_rect(body.start, y + 14, w, 2, Color::SHADOW);
        canvas.fine_rect(body.start, y + BODY_H - 2, w, 3, Color::SHADOW);
    }
    // A bogie near either end (or one in the middle): two wheels each,
    // dark steel, or the outline's colour.
    let steel = if dim { color } else { Color::TEXT_FAINT };
    let bogies = if w >= 30 {
        [body.start + 3, body.end - 15]
    } else {
        [body.start + w / 2 - 6; 2]
    };
    if w >= 14 {
        for bogie in bogies {
            for wheel in [bogie, bogie + 7] {
                street::fine_ellipse(canvas, wheel, RAIL_Y - 6, 6, 6, steel);
            }
        }
    }
}
