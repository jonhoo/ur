//! Living Tokyo's Air screen: a nearby room sensor's reading, as a small
//! lit room at night.

use core::fmt::Write as _;

use embassy_time::Instant;

use super::{BACKGROUND, street};
use crate::aranet::{self, Band, Sighting};
use crate::canvas::{Canvas, Color};
use crate::font;
use crate::font::fine::Style;
use crate::themes::helpers::tenths;
use crate::ui::Fits;

// A small room at night, seen from inside, kept plain so the figures
// have it: the reading on the wall, a little window high up beside the
// title, and a ramen bowl steaming on the counter at the foot.
//
// Canvas layout (80×80), y ranges inclusive:
//
//    1..10   AIR on its sign (left), the window, how old the reading
//            is (right)
//   13..33   CO₂ in ppm, big, centred
//   38..42   "PPM CO2" (left); the band, GOOD/FAIR/POOR (right)
//   46..48   three segments, the band's lit: it reads without colour
//   54..63   temperature (left) and humidity (right), the steam rising
//            between them
//   69..73   pressure (left); the sensor's battery (right)
//   70..76   the bowl, on the counter's edge
//
// A sensor without CO₂ (a BTHome thermometer) has the temperature big in
// its place, "°C" for its label, and no band; then the humidity (right),
// labelled (left).
//
// Its one living thing is the bowl's steam, rising higher the stuffier
// the air: a wisp in fresh air, up to the band's bar past the Aranet's
// "poor". Quieter: the window fogs over as the air gets humid, and shows
// the night (the rain running down its glass, or a star; the far city's
// windows lit as its people are up); and the figures fade as the reading
// ages, the big one dimming past ten minutes, all of them faint once
// it's stale. In the dim look, the room's shapes and its figures, still.
//
// The room and the figures are drawn at the panel's resolution, the
// figures in the city's lettering: they change only with a reading, a
// scan every few minutes at most. The labels, the age (which changes by
// the minute, but is small) and the band stay on the canvas.

/// The window, in the panel's pixels: its frame's left, top, width and
/// height, between the title's sign and the age.
const WINDOW: (i32, i32, i32, i32) = (75, 3, 66, 30);
/// A star through the window's glass.
const WINDOW_STARS: [(i32, i32); 1] = [(41, 3)];
/// The counter's top, and the bowl's middle on it, in the panel's pixels:
/// right of the middle, its chopsticks clear of the pressure.
const COUNTER: i32 = 228;
const BOWL: i32 = 124;
/// The steam's foot, over the bowl's rim, and the tallest it rises: up
/// to the band's bar.
const STEAM_FOOT: i32 = COUNTER - 10;
const STEAM_MAX: i32 = STEAM_FOOT - 3 * 49;

/// The big figure (CO₂, or a thermometer's temperature), heavy, its ink
/// over canvas rows 13..33; the temperature and humidity under it, at
/// about the 2× small font's height.
const BIG: Style = Style {
    cap: 52,
    stroke: 112,
    caps: false,
};
const BIG_Y: i32 = 3 * 13 + 2;
const SMALL: Style = Style {
    cap: 22,
    stroke: 52,
    caps: false,
};
const SMALL_Y: i32 = 3 * 54 + 2;

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
    // The figures' edges smoothed against the night, but not in the dim
    // look, whose eight colours have no shade between.
    let edge = (!dim).then_some(Color::TEXT_FAINT);

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

    // The big figure, centred: CO₂ when the sensor measures it (dashes
    // while an Aranet4 calibrates), the temperature from a thermometer.
    let mut text = heapless::String::<8>::new();
    let thermometer = reading.co2.is_none() && reading.kind != aranet::Kind::Aranet4;
    let label = if thermometer {
        text.push_str(&temperature).fits();
        "°C"
    } else {
        match reading.co2 {
            Some(ppm) => write!(text, "{}", ppm.min(9999)).fits(),
            None => text.push_str("----").fits(),
        }
        "PPM CO2"
    };
    let width = font::Face::Fine(BIG).width(&text);
    canvas.fine_type(120 - width / 2, BIG_Y, &text, BIG, big, edge);
    canvas.text(4, 38, label, Color::TEXT_DIM);
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

    // Temperature (unless it's big already) and humidity.
    if thermometer {
        if reading.humidity.is_some() {
            canvas.text(4, 57, "HUMIDITY", Color::TEXT_DIM);
        }
    } else {
        temperature.push('°').fits();
        canvas.fine_type(12, SMALL_Y, &temperature, SMALL, value, edge);
    }
    if let Some(humidity) = reading.humidity {
        let mut text = heapless::String::<4>::new();
        write!(text, "{}%", humidity.min(100)).fits();
        let width = font::Face::Fine(SMALL).width(&text);
        canvas.fine_type(228 - width, SMALL_Y, &text, SMALL, value, edge);
    }

    // The room: the window, and the bowl on the counter, steaming by the
    // CO₂ (a wisp with none to go by): higher, and in more wisps, the
    // stuffier the air.
    render_window(canvas, reading.humidity, night, dim);
    if !dim {
        let ppm = reading.co2.map_or(0, |ppm| i32::from(ppm.min(9999)));
        let height = 8 + (ppm - 500).clamp(0, 1500) * (STEAM_MAX - 8) / 1500;
        let wisps = if ppm >= 1000 { 3 } else { 2 };
        street::fine_steam(canvas, BOWL, STEAM_FOOT, height, wisps, night.tick);
    }
    street::fine_bowl(canvas, BOWL, COUNTER);
    canvas.fine_rect(0, COUNTER, 240, 1, Color::TEXT_FAINT);
    canvas.fine_rect(0, COUNTER + 1, 240, 240 - COUNTER - 1, Color::SHADOW);

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

/// The window high on the wall, at the panel's resolution: through it,
/// the rain running down the glass or a star, and the far city's windows
/// at its foot; over it, fog as thick as the air is humid (none under
/// 40%, a light mist, then thicker at 60% and 75%); its frame, a sash
/// down its middle, and its sill. In the dim look, the frame.
fn render_window(canvas: &mut Canvas, humidity: Option<u8>, night: street::Night, dim: bool) {
    let (x, y, w, h) = WINDOW;
    let (inside, top, inner, bottom) = (x + 2, y + 2, w - 4, y + h - 2);
    if !dim {
        if night.rain {
            // Drops running down the glass, a pane's width apart.
            for i in 0..5 {
                let dx = 6 + 12 * i;
                let dy = (night.tick as i32 * 7 + i * 11).rem_euclid(bottom - top - 5);
                canvas.fine_rect(inside + dx, top + dy, 1, 5, Color::TEXT_FAINT);
            }
        } else {
            street::fine_stars(canvas, &WINDOW_STARS, None);
        }
        street::fine_skyline(
            canvas,
            inside,
            inner,
            bottom,
            15,
            street::city_lights(night.hour),
        );
        let fog = match humidity.unwrap_or(0) {
            0..40 => 0,
            40..60 => 1,
            60..75 => 2,
            _ => 3,
        };
        if fog > 0 {
            canvas.fine_tone(inside, top, inner, bottom - top, fog, Color::TEXT_FAINT);
        }
    }
    let frame = Color::TEXT_FAINT;
    canvas.fine_rect(x, y, w, 2, frame);
    canvas.fine_rect(x, bottom, w, 2, frame);
    canvas.fine_rect(x, y, 2, h, frame);
    canvas.fine_rect(x + w - 2, y, 2, h, frame);
    canvas.fine_rect(x + w / 2 - 1, y, 2, h, frame);
    canvas.fine_rect(x - 3, y + h, w + 6, 2, Color::TEXT_DIM);
}
