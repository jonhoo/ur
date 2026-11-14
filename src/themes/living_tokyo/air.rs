//! Living Tokyo's Air screen: a nearby room sensor's reading, as a small
//! lit room at night.

use core::fmt::Write as _;

use embassy_time::Instant;

use super::{BACKGROUND, draw_digit, street};
use crate::aranet::{self, Band, Sighting};
use crate::canvas::{Canvas, Color};
use crate::font;
use crate::themes::helpers::tenths;
use crate::ui::Fits;

// A small room at night, seen from inside, kept plain so the figures
// have it: the reading on the wall, a little window high up beside the
// title, and a bowl steaming on the counter at the foot.
//
// Canvas layout (80×80), y ranges inclusive:
//
//    1..9    AIR on its sign (left), the window, how old the reading
//            is (right)
//   13..33   CO₂ in ppm, in the time's digits at 3×, centred
//   38..42   "PPM CO2" (left); the band, GOOD/FAIR/POOR (right)
//   46..48   three segments, the band's lit: it reads without colour
//   54..63   temperature (left) and humidity (right) at 2×, the steam
//            rising between them
//   69..73   pressure (left); the sensor's battery (right)
//   72..76   the bowl, on the counter's edge
//
// A sensor without CO₂ (a BTHome thermometer) has the temperature big in
// its place, "°C" for its label, and no band; then the humidity at 2×
// (right), labelled (left).
//
// Its one living thing is the bowl's steam, rising higher the stuffier
// the air: a wisp in fresh air, up to the band's bar past the Aranet's
// "poor". Quieter: the window fogs over as the air gets humid, and shows
// the night (the rain on its glass, or a star; the far city's windows
// lit as its people are up); and the figures fade as the reading ages,
// the big one dimming past ten minutes, all of them faint once it's
// stale. In the dim look, the room's shapes and its figures, still.

/// The window: its frame's left column and width, top row and bottom
/// row, between the title's sign and the age.
const WINDOW_X: i32 = 25;
const WINDOW_W: i32 = 22;
const WINDOW_TOP: i32 = 1;
const WINDOW_BOTTOM: i32 = 10;
/// The counter's edge, and the bowl on it: its left column and width.
const COUNTER: i32 = 76;
const BOWL_X: i32 = 39;
const BOWL_W: i32 = 7;
/// The tallest the steam rises: up to the band's bar.
const STEAM_MAX: i32 = 20;
/// A star through the window's glass.
const WINDOW_STARS: [(i32, i32); 1] = [(41, 3)];

/// A nearby room sensor's latest reading (see [`crate::aranet`]): from an
/// Aranet4, the CO₂ big, since that's what the watch can't otherwise know
/// and what to act on (open a window), then what else it measures. From a
/// BTHome thermometer, the temperature big (to the tenth, as on the clock
/// face), and whatever else it sends.
///
/// Not on the clock face: CO₂ changes over tens of minutes, and its number
/// would crowd a face that's read at a glance, many times a day. The
/// temperature there comes from the same reading (see
/// `crate::temperature`).
///
/// The readings are as old as the last scan, at most a quarter of an hour
/// with a sensor around (opening this screen scans again if the last is a
/// minute old), so the age is always shown; past [`aranet::STALE`], the
/// numbers go faint.
pub(super) fn render_air(canvas: &mut Canvas, sighting: &Sighting, now: Instant, dim: bool) {
    canvas.clear(BACKGROUND);
    let night = street::Night::now();
    street::title(canvas, "AIR", Color::ACCENT);
    let reading = &sighting.reading;
    let age = sighting.age(now.as_millis());
    let mut text = heapless::String::<12>::new();
    match age {
        0..60 => text.push_str("NOW").fits(),
        60..3600 => write!(text, "{} MIN", age / 60).fits(),
        _ => write!(text, "{} H", age / 3600).fits(),
    }
    let stale = age >= aranet::STALE;
    canvas.text_right(76, 3, &text, Color::TEXT_DIM);
    // The board fades as the reading ages: its big figure dims after ten
    // minutes, and every figure goes faint once it's stale.
    let value = if stale {
        Color::TEXT_FAINT
    } else {
        Color::TEXT
    };
    let big = if age >= 10 * 60 && !stale {
        Color::TEXT_DIM
    } else {
        value
    };

    // The temperature, to the tenth: "--.-" if invalid. Within ±100 °C
    // (and a sauna's), so that big it still fits the width.
    let mut temperature = match reading.tenths() {
        Some(t) => tenths(t.clamp(-999, 1999)),
        None => {
            let mut text = heapless::String::new();
            text.push_str("--.-").fits();
            text
        }
    };

    // The big number: CO₂ when the sensor measures it (dashes while an
    // Aranet4 calibrates), the temperature from a thermometer.
    const BIG_Y: i32 = 13;
    let mut text = heapless::String::<8>::new();
    let thermometer = reading.co2.is_none() && reading.kind != aranet::Kind::Aranet4;
    if thermometer {
        draw_big_number(canvas, BIG_Y, &temperature, big);
        canvas.text(4, 38, "°C", Color::TEXT_DIM);
    } else {
        match reading.co2 {
            Some(ppm) => {
                write!(text, "{}", ppm.min(9999)).fits();
                draw_big_number(canvas, BIG_Y, &text, big);
            }
            // A digit's width each.
            None => {
                for x in [6, 23, 40, 57] {
                    canvas.fill_rect(x, BIG_Y + 9, 15, 3, big);
                }
            }
        }
        canvas.text(4, 38, "PPM CO2", Color::TEXT_DIM);
    }
    // The band: the word, and which third of the bar is lit, so it reads
    // in every theme, even those of one hue.
    if let Some(band) = reading.band {
        let color = match band {
            Band::Good => Color::GOOD,
            Band::Fair => Color::TEXT,
            Band::Poor => Color::BAD,
        };
        canvas.text_right(76, 38, band.word(), color);
        for (i, segment) in [Band::Good, Band::Fair, Band::Poor].into_iter().enumerate() {
            let lit = if segment == band { color } else { Color::TRACK };
            canvas.fill_rect(4 + 25 * i as i32, 46, 22, 3, lit);
        }
    }

    // Temperature (unless it's big already) and humidity, at 2×.
    if thermometer {
        if reading.humidity.is_some() {
            canvas.text(4, 57, "HUMIDITY", Color::TEXT_DIM);
        }
    } else {
        temperature.push('°').fits();
        canvas.text_scaled(4, 54, &temperature, value, 2);
    }
    if let Some(humidity) = reading.humidity {
        let mut text = heapless::String::<4>::new();
        write!(text, "{}%", humidity.min(100)).fits();
        canvas.text_scaled(76 - 2 * font::small_text_width(&text), 54, &text, value, 2);
    }

    // The room: the window, and the bowl on the counter, steaming by the
    // CO₂ (a wisp with none to go by).
    render_window(canvas, reading.humidity, night, dim);
    canvas.fill_rect(BOWL_X, COUNTER - 3, BOWL_W, 3, Color::HEART);
    canvas.fill_rect(BOWL_X - 1, COUNTER - 4, BOWL_W + 2, 1, Color::TEXT_DIM);
    canvas.fill_rect(BOWL_X - 6, COUNTER, BOWL_W + 12, 1, Color::TEXT_FAINT);
    if !dim {
        let height = reading.co2.map_or(3, |ppm| {
            3 + (i32::from(ppm.min(9999)) - 500).clamp(0, 1500) * (STEAM_MAX - 3) / 1500
        });
        street::steam(canvas, BOWL_X + 2, COUNTER - 6, height, night.tick);
    }

    // Pressure, and the sensor's own battery, red when it wants changing:
    // each if the sensor sends it.
    let mut text = heapless::String::<10>::new();
    if let Some(pressure) = reading.pressure {
        // Widened: a BTHome sensor's pressure is clamped to `u16::MAX`
        // tenths (see `bthome_sensor`), and adding 5 to that overflowed.
        write!(text, "{} HPA", (u32::from(pressure) + 5) / 10).fits();
    }
    canvas.text(4, 69, &text, Color::TEXT_DIM);
    if let Some(battery) = reading.battery {
        let mut text = heapless::String::<8>::new();
        write!(text, "BAT {}%", battery.min(100)).fits();
        let color = if battery <= 15 {
            Color::BAD
        } else {
            Color::TEXT_DIM
        };
        canvas.text_right(76, 69, &text, color);
    }
}

/// The window high on the wall: through it, the rain running down the
/// glass or a star, and the far city's windows at its foot; over it, fog
/// as thick as the air is humid (none under 40%, a light mist, then
/// thicker at 60% and 75%). In the dim look, its frame.
fn render_window(canvas: &mut Canvas, humidity: Option<u8>, night: street::Night, dim: bool) {
    let (x, w, top, bottom) = (WINDOW_X, WINDOW_W, WINDOW_TOP, WINDOW_BOTTOM);
    let (inside, inner) = (x + 1, w - 2);
    if !dim {
        if night.rain {
            // Its streaks, within the glass.
            for i in 0..4 {
                let rx = inside + 2 + i * 5;
                let ry = top + 1 + ((night.tick as i32 + i * 3) % 5);
                canvas.fill_rect(rx, ry, 1, 2, Color::TEXT_FAINT);
            }
        } else {
            street::fine_stars(canvas, &WINDOW_STARS, None);
        }
        street::fine_skyline(
            canvas,
            3 * inside,
            3 * inner,
            3 * bottom,
            3 * 5,
            street::city_lights(night.hour),
        );
        // Fog: every `period`th pixel, staggered row to row.
        let period = match humidity.unwrap_or(0) {
            0..40 => 0,
            40..60 => 6,
            60..75 => 3,
            _ => 2,
        };
        if period > 0 {
            for fy in top + 1..bottom {
                for fx in inside..inside + inner {
                    if (fx + fy * (1 + period / 3)) % period == 0 {
                        canvas.pixel(fx, fy, Color::TEXT_FAINT);
                    }
                }
            }
        }
    }
    // The frame, and its cross bar.
    let frame = Color::TEXT_FAINT;
    canvas.fill_rect(x, top, w, 1, frame);
    canvas.fill_rect(x, bottom, w, 1, frame);
    canvas.fill_rect(x, top, 1, bottom - top, frame);
    canvas.fill_rect(x + w - 1, top, 1, bottom - top, frame);
    canvas.fill_rect(x + w / 2, top, 1, bottom - top, frame);
}
/// `text` (digits, '.', '-') in the time's digits at 3×, centred, with
/// its top at `y`: the point is the glyphs' bottom row, a dash their
/// middle one (three of the digits' pixels each).
fn draw_big_number(canvas: &mut Canvas, y: i32, text: &str, color: Color) {
    let advance = |c: char| match c {
        '.' => 5,
        '-' => 11,
        _ => 17,
    };
    let width: i32 = text.chars().map(advance).sum::<i32>() - 2;
    let mut x = 40 - width / 2;
    for c in text.chars() {
        let mark = |canvas: &mut Canvas, dy: i32, w: i32| {
            canvas.fill_rect(x + 1, y + dy + 1, w, 3, Color::SHADOW);
            canvas.fill_rect(x, y + dy, w, 3, color);
        };
        match c {
            '.' => mark(canvas, 18, 3),
            '-' => mark(canvas, 9, 9),
            // Anything else isn't a digit either: drawn as one too big
            // (see `digit_glyph`), rather than underflowing.
            digit => {
                let value = digit.to_digit(10).unwrap_or(u32::MAX);
                draw_digit(canvas, x, y, u64::from(value), 3, color)
            }
        }
        x += advance(c);
    }
}
