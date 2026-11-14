//! Living Tokyo's notifications, calls, and music.

use super::layout::{CALL_BUTTONS_Y, CALL_TEXT_WIDTH, MUSIC_CONTROLS_Y, MUSIC_TEXT_WIDTH};
use super::{BACKGROUND, street};
use crate::canvas::{Canvas, Color};
use crate::font;
use crate::themes::helpers::fitted;
use crate::themes::{Call, Music};

// A notification is a message from somewhere else in the city: its app
// is a neon sign hanging where the other screens hang their titles, lit
// in the app's own colour (see `neon`), with when it arrived top right
// in the history, as MUSIC's state is. Who it's from is under it, centred,
// in a face that makes it the most important thing on the screen; and
// under that, where the message is from: a little noodle bar with the
// app's noren over its door (see `render_shop`), which keeps the hours
// and the night's weather. The banner is the same sign in short, over
// the bottom of whatever shows.
//
// Who it's from is the 3×5 at 2× if it fits on one line (up to about 9
// letters), or on two of about the same width (see `large`): a poster,
// the size the watch's other big text is. Anything longer is in the
// medium face (see `font::medium_glyph`), balanced over as few lines as
// it takes (see `font::wrap`), never the small 1×, which is too small to
// read at a glance. Either way it's centred, as a name on a screen of its
// own is expected to be, and as it was before the medium face: the
// balanced lines read as a block, and a short name doesn't sit in a
// corner. (The banner, which has to be compact, is the exception: see
// `render_banner`.)

/// The neon an app's sign is lit in, on the card and the banner alike:
/// one of four, by a hash of its name (FNV-1a), so that the wearer comes
/// to know an app's sign by its colour before reading it, as a street's
/// regulars know its signs. No app is picked out by name: which colour
/// an app gets is the hash's, and two may share one. Not the accent
/// (the watch's own titles) nor the red (what rings).
fn neon(app: &str) -> Color {
    const NEONS: [Color; 4] = [Color::REM, Color::DEEP, Color::GOOD, Color::HEART_SHINE];
    let hash = app.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    NEONS[hash as usize % NEONS.len()]
}

/// Text columns of the banner and the notification card, as of a call:
/// four columns (12 of the panel's pixels) clear of each edge, for a
/// little more air than the three there were. Four still leave room for
/// 18 letters at 1× and 9 at 2×, as three did; a fifth would take a
/// letter off each.
const BANNER_TEXT: core::ops::Range<i32> = 4..76;
const BANNER_TEXT_WIDTH: i32 = BANNER_TEXT.end - BANNER_TEXT.start;

/// Line pitch of the medium face: 7 px capitals, 2 accent rows, and a
/// row's gap, so an accent never touches the line above.
const MEDIUM_PITCH: i32 = font::MEDIUM_HEIGHT + font::ACCENT_ROWS + 1;

/// The most lines who it's from takes in the card: 40 letters at about 11
/// a line, with room to spare for long words.
const CARD_LINES: usize = 5;

/// How unequal two lines at 2× may be: the shorter at least 7/10 of the
/// longer, in pixels. In this all but monospaced font, a 5- and a 5-letter
/// word are equal, 4 and 5 make 79%, 5 and 7 just make it at 70%, and 4
/// and 6 (65%) or 3 and 8 (35%) don't. Below about this, the pair reads
/// as a long line with a stub hanging off it rather than as a block.
const BALANCE_TENTHS: i32 = 7;

/// Who it's from at 2×, as `lines` of the small font: how many (one or
/// two), or 0 if it's not short enough. Two only split at a space, about
/// equal (see [`BALANCE_TENTHS`]): a first name and a surname, typically.
/// Never hyphenated: a broken word in large type is harder to read than
/// the same word whole in the medium face, and so is a big first line
/// over a one-word second.
fn large(who: &str, lines: &mut [font::Line; CARD_LINES]) -> usize {
    // `wrap` balances the lines already: what's left is to check how it
    // broke them. A break at a space is neither a hyphen nor a joint in a
    // link; an ellipsis at the end means it needed more than two.
    let count = font::wrap(
        who,
        font::Face::Small,
        BANNER_TEXT_WIDTH / 2,
        &mut lines[..2],
    );
    let [top, bottom, ..] = *lines;
    match count {
        1 => 1,
        2 if bottom.tail == font::Tail::None && who.as_bytes().get(top.end) == Some(&b' ') => {
            let (a, b) = (
                top.width(who, font::Face::Small),
                bottom.width(who, font::Face::Small),
            );
            if a.min(b) * 10 >= a.max(b) * BALANCE_TENTHS {
                2
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// A notification, with the whole screen (one that arrived with the screen
/// off, in the dim look, or a page of the history): the app on its sign
/// top left, and `time` top right (when it arrived, in the history;
/// nothing on a notification that has just come, as that's now), who
/// it's from below, centred, big if it's short (see [`large`]), and under
/// that, the shop (see [`render_shop`]).
pub(super) fn render_card(canvas: &mut Canvas, app: &str, who: &str, time: &str, dim: bool) {
    canvas.clear(BACKGROUND);
    let tube = neon(app);
    let now = street::local_now();
    // With no time yet, an evening.
    let hour = now.map_or(21, street::hour_of);
    let rain = now.is_some_and(|second| street::rainy(street::evening(second)));
    let tick = now.map_or(0, |second| second as u64);

    // Who it's from: at 2× (pitch, capitals' height, and accents' rows,
    // all doubled), or in the medium face.
    let mut lines = [font::Line::default(); CARD_LINES];
    let (count, face, scale, pitch, height) = match large(who, &mut lines) {
        0 => (
            font::wrap(who, font::Face::Medium, BANNER_TEXT_WIDTH, &mut lines),
            font::Face::Medium,
            1,
            MEDIUM_PITCH,
            font::MEDIUM_HEIGHT,
        ),
        count => (
            count,
            font::Face::Small,
            2,
            2 * (5 + font::ACCENT_ROWS + 2),
            2 * 5,
        ),
    };
    // The capitals centred between the sign and the shop's roof, which
    // is where the eye looks for them, but no nearer the sign than two
    // clear rows above the accents. Accents above the first line are left
    // out of the centring, as most names have none. A long name pushes
    // the roof down, the shop giving up its height to it.
    let block = pitch * (count as i32 - 1) + height;
    let top = ((12 + SHOP_TOP - 3) / 2 - block / 2).max(12 + 2 + font::ACCENT_ROWS * scale);
    let roof = (top + block + 4).clamp(SHOP_TOP, SHOP_LOWEST);

    // The night over the roofs: the dawn's glow behind them (two levels
    // at most, as more would reach the name), and the stars out, or the
    // rain. Dim, none of it: idle mode keeps still.
    if !dim {
        let light = street::daylight(hour);
        street::horizon(canvas, 0, 80, roof, light.min(2));
        if rain {
            street::rain(canvas, 12, roof, tick, Color::TEXT_FAINT);
        } else if light < street::DAYLIGHT {
            street::stars(canvas, &CARD_STARS);
        }
    }
    render_shop(canvas, roof, tube, hour, rain, dim);
    // Steam from the kitchen's vent while it cooks on a clear night,
    // rising with the seconds: beside the name, if it ends short of the
    // vent, else up to two rows short of it.
    let right = lines[..count]
        .iter()
        .map(|&line| 40 + line.width(who, face) * scale / 2)
        .max()
        .unwrap_or(0);
    let steam = if right < VENT_X - 1 {
        14
    } else {
        roof - 6 - (top + block + 2)
    };
    if !dim && !rain && Shop::at(hour) != Shop::Shut && steam >= 3 {
        street::steam(canvas, VENT_X, roof - 6, steam.min(14), tick);
    }

    // The app gives way to the time: 4 px between them (after the sign's
    // tube), and an ellipsis.
    let time_width = font::small_text_width(time);
    let gap = if time.is_empty() { 0 } else { time_width + 6 };
    let mut line = [font::Line::default()];
    let room = BANNER_TEXT_WIDTH - gap;
    let width = match font::wrap(app, font::Face::Small, room, &mut line) {
        0 => 0,
        _ => line[0].width(app, font::Face::Small),
    };
    street::hanging(canvas, width, tube);
    if width > 0 {
        canvas.line(BANNER_TEXT.start, 3, app, line[0], font::Face::Small, tube);
    }
    canvas.text_right(BANNER_TEXT.end, 3, time, Color::TEXT_DIM);

    for (i, &line) in lines[..count].iter().enumerate() {
        let y = top + pitch * i as i32;
        // The tail (an ellipsis or a hyphen) is part of the width.
        let w = line.width(who, face) * scale;
        let x = 40 - w / 2;
        if rain && !dim {
            // The rain passes behind the name, not through its letters,
            // which it would break up: a pixel clear round each line,
            // its accents' rows included.
            let accents = font::ACCENT_ROWS * scale;
            canvas.fill_rect(
                x - 1,
                y - accents - 1,
                w + 2,
                height + accents + 2,
                BACKGROUND,
            );
        }
        if scale == 2 {
            canvas.text_scaled(x, y, line.of(who), Color::TEXT, 2);
        } else {
            canvas.line(x, y, who, line, face, Color::TEXT);
        }
    }
}

/// The shop's roof, under a name of a line or two; and the lowest it
/// goes for a longer one, leaving the noren and the lanterns room.
const SHOP_TOP: i32 = 50;
const SHOP_LOWEST: i32 = 64;
/// The pavement in front of the shop, down to the bottom edge.
const PAVEMENT: i32 = 73;
/// The doorway, its noren across it, and a lantern either side.
const DOOR_X: i32 = 24;
const DOOR_W: i32 = 32;
const LANTERNS: [i32; 2] = [15, 60];
/// The kitchen's vent on the roof, near the right edge.
const VENT_X: i32 = 71;

/// A few stars over the roofs, among the name's letters as much as
/// round them: single pixels it covers where they meet.
const CARD_STARS: [(i32, i32); 6] = [(9, 17), (70, 15), (4, 38), (75, 33), (61, 45), (18, 46)];

/// How the shop is at `hour`: open in the evening, its interior lit and
/// full; in the small hours, dimmer, a regular or two left; shut from
/// 04:00 until the evening, its shutter down, its noren taken in and
/// its lanterns out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shop {
    Busy,
    Late,
    Shut,
}

impl Shop {
    fn at(hour: u8) -> Self {
        match hour {
            18..=23 => Shop::Busy,
            0..=3 => Shop::Late,
            _ => Shop::Shut,
        }
    }
}

/// The shop the notification is at, from its roof (`roof`) down: a
/// little noodle bar at night, its noren in the app's neon (`tube`), the
/// lanterns either side of its door, and its diners' backs at the
/// counter against the warm light; and the pavement in front, wet on a
/// rainy night (`rain`), the light broken up in its puddles, or dry, the
/// light spilling onto it. It keeps the hours (see [`Shop`]). In the dim
/// look, only its outline and its noren: no light, nothing moving.
fn render_shop(canvas: &mut Canvas, roof: i32, tube: Color, hour: u8, rain: bool, dim: bool) {
    let shop = if dim { Shop::Shut } else { Shop::at(hour) };
    let door_top = roof + 3;
    let door_h = PAVEMENT - door_top;
    // The wall, darker than the night, under a tiled eave: its edge, and
    // a row of tiles' ends (in the dim look, the edge alone, in grey).
    canvas.fill_rect(0, roof, 80, PAVEMENT - roof, Color::SHADOW);
    if dim {
        canvas.fill_rect(0, roof, 80, 1, Color::TRACK);
    } else {
        canvas.fill_rect(0, roof, 80, 2, Color::TRACK);
        canvas.fill_rect(0, roof, 80, 1, Color::TEXT_FAINT);
        for x in (1..80).step_by(3) {
            canvas.pixel(x, roof + 1, Color::SHADOW);
        }
        // The kitchen's vent on the roof (its steam is the card's to
        // draw, clear of the name).
        canvas.fill_rect(VENT_X, roof - 4, 3, 4, Color::TRACK);
        canvas.fill_rect(VENT_X - 1, roof - 5, 5, 1, Color::TEXT_FAINT);
    }

    // The doorway: lit, dimmer in the small hours, or shuttered.
    match shop {
        Shop::Busy => canvas.fill_rect(DOOR_X, door_top, DOOR_W, door_h, Color::HEART_SHINE),
        Shop::Late => {
            for y in door_top..PAVEMENT {
                for x in DOOR_X..DOOR_X + DOOR_W {
                    if (x + y) % 2 == 0 {
                        canvas.pixel(x, y, Color::HEART_SHINE);
                    }
                }
            }
        }
        Shop::Shut if dim => {
            // Its frame alone, a still outline.
            canvas.fill_rect(DOOR_X - 1, door_top, 1, door_h, Color::TRACK);
            canvas.fill_rect(DOOR_X + DOOR_W, door_top, 1, door_h, Color::TRACK);
        }
        Shop::Shut => {
            canvas.fill_rect(DOOR_X, door_top, DOOR_W, door_h, Color::TRACK);
            for y in (door_top + 1..PAVEMENT).step_by(2) {
                canvas.fill_rect(DOOR_X, y, DOOR_W, 1, Color::SHADOW);
            }
        }
    }
    // The diners: their backs at the counter, a head and shoulders each,
    // as many as the hour keeps (three in the evening, one late).
    let counter = PAVEMENT - 4;
    let diners: &[i32] = match shop {
        Shop::Busy => &[28, 37, 47],
        Shop::Late => &[42],
        Shop::Shut => &[],
    };
    if shop != Shop::Shut && counter - 5 > door_top + 5 {
        canvas.fill_rect(DOOR_X, counter, DOOR_W, 1, Color::SHADOW);
        for &x in diners {
            canvas.fill_rect(x + 1, counter - 6, 2, 2, Color::SHADOW);
            canvas.fill_rect(x, counter - 4, 4, 4, Color::SHADOW);
        }
    }
    // The noren, across the top of the doorway (taken in when it's shut,
    // bar in the dim look, where it's the shop's sign): three panels in
    // the app's colour, hanging from a rod, slit apart down to their
    // hems, the light showing between them; in the dim look, their
    // outlines only, still and thin.
    if shop != Shop::Shut || dim {
        let noren_h = 11.min(door_h);
        canvas.fill_rect(DOOR_X - 1, door_top, DOOR_W + 2, 1, Color::TEXT_FAINT);
        const PANEL: i32 = (DOOR_W - 4) / 3;
        for panel in 0..3 {
            let x = DOOR_X + panel * (PANEL + 2);
            let w = if panel == 2 {
                DOOR_X + DOOR_W - x
            } else {
                PANEL
            };
            canvas.fill_rect(x, door_top + 1, w, noren_h - 1, tube);
            if dim {
                canvas.fill_rect(x + 1, door_top + 1, w - 2, noren_h - 2, BACKGROUND);
            }
        }
    }
    let lit = shop != Shop::Shut;
    if !dim {
        for x in LANTERNS {
            street::lantern(canvas, x, door_top + 1, lit);
        }
    }

    // The pavement: wet, the doorway's light and the lanterns broken up
    // in it; or dry, the light spilling out onto it.
    canvas.fill_rect(0, PAVEMENT, 80, 1, Color::TRACK);
    if dim {
        return;
    }
    if rain {
        if lit {
            street::reflection(canvas, DOOR_X, DOOR_W, PAVEMENT + 2, 3, Color::HEART_SHINE);
            for x in LANTERNS {
                street::reflection(canvas, x, 5, PAVEMENT + 2, 3, Color::HEART);
            }
        }
        street::reflection(canvas, DOOR_X + 4, DOOR_W - 8, PAVEMENT + 1, 1, tube);
    } else if lit {
        for row in 1..4 {
            let inset = row * 2;
            for x in DOOR_X - inset..DOOR_X + DOOR_W + inset {
                if (x + row) % (row + 1) == 0 {
                    canvas.pixel(x, PAVEMENT + row, Color::HEART_SHINE);
                }
            }
        }
    }
}

/// Line pitch of the small font in the banner: 5 px capitals, 2 accent
/// rows, and a row's gap, so an accent never touches the line above.
const SMALL_PITCH: i32 = 5 + font::ACCENT_ROWS + 1;

/// Over the bottom of the screen, as compact as it was before the
/// redesign: a sign the width of the screen, lit in the app's neon (see
/// [`neon`]), the app (or what happened) small in it, and who it's from
/// (or what to do) small too, in the text's colour, on one
/// line or two, ending in an ellipsis if even two won't do. The history
/// has the rest. One line is 17 px, over the clock face's steps, leaving
/// the heart rate and the next alarm in view; two, 25, from just under
/// the date and the seconds, which a 4 s banner can afford; the time
/// always shows.
///
/// Small, not in the medium face that names are given on a screen of
/// their own: its 7 px capitals, their accents, and a row clear of the
/// app above them make 9 rows to the small font's 7, and the 17 px can't
/// spare them without the app crowding the tube or the text the bottom
/// edge. Rows, from the tube: 1 clear, the app's 5, 1 clear (so Å's ring
/// never touches it), 2 for accents, 5 for the capitals, and 2 clear at
/// the bottom.
pub(super) fn render_banner(canvas: &mut Canvas, app: &str, who: &str) {
    let mut lines = [font::Line::default(); 2];
    let count = font::wrap(who, font::Face::Small, BANNER_TEXT_WIDTH, &mut lines).max(1);
    let top = 63 - SMALL_PITCH * (count as i32 - 1);
    let tube = neon(app);
    street::sign(canvas, 0, top, 80, 80 - top, tube);
    fitted(
        canvas,
        (BANNER_TEXT.start, top + 2),
        app,
        font::Face::Small,
        BANNER_TEXT_WIDTH,
        tube,
    );
    for (i, &line) in lines[..count].iter().enumerate() {
        let y = top + 2 + 5 + 1 + font::ACCENT_ROWS + SMALL_PITCH * i as i32;
        canvas.line(
            BANNER_TEXT.start,
            y,
            who,
            line,
            font::Face::Small,
            Color::TEXT,
        );
    }
}

/// The caller's name scrolls within columns 4..76 (see
/// [`CALL_TEXT_WIDTH`]); the decline and answer buttons are where the UI
/// takes taps on them.
pub(super) fn render_call(canvas: &mut Canvas, call: &Call) {
    canvas.clear(BACKGROUND);
    canvas.text_centered(40, 6, "INCOMING CALL", Color::ACCENT);

    // The caller at 2×, centred if it fits, scrolling if not.
    let width = font::small_text_width(call.caller) * 2;
    let left = if width <= CALL_TEXT_WIDTH {
        40 - width / 2
    } else {
        4
    };
    canvas.marquee(
        left..4 + CALL_TEXT_WIDTH,
        24,
        call.caller,
        Color::TEXT,
        2,
        call.elapsed,
    );

    let hint = if call.silenced {
        "SILENCED"
    } else {
        "BUTTON: SILENCE"
    };
    canvas.text_centered(40, 44, hint, Color::TEXT_FAINT);

    for (x, label, color) in [(2, "DECLINE", Color::BAD), (42, "ANSWER", Color::GOOD)] {
        canvas.fill_rect(x, CALL_BUTTONS_Y, 36, 18, color);
        canvas.text_centered(x + 18, CALL_BUTTONS_Y + 7, label, BACKGROUND);
    }
}

/// Artist and track scroll within columns 3..77 (see [`MUSIC_TEXT_WIDTH`]);
/// the controls are where the UI takes taps on them.
pub(super) fn render_music(canvas: &mut Canvas, music: &Music) {
    let elapsed = music.elapsed;
    canvas.clear(BACKGROUND);
    canvas.text(3, 4, "MUSIC", Color::ACCENT);
    canvas.text_right(
        77,
        4,
        match music.playing {
            Some(true) => "PLAYING",
            Some(false) => "PAUSED",
            None => "",
        },
        Color::TEXT_DIM,
    );

    canvas.marquee(
        3..3 + MUSIC_TEXT_WIDTH,
        17,
        music.artist,
        Color::TEXT_DIM,
        1,
        elapsed,
    );
    canvas.marquee(
        3..3 + MUSIC_TEXT_WIDTH,
        29,
        music.track,
        Color::TEXT,
        2,
        elapsed,
    );

    // Previous, play/pause, next: 5×5 icons at 3×, centred in thirds.
    let play_pause = if music.playing == Some(true) {
        &font::PAUSE
    } else {
        &font::PLAY
    };
    for (x, icon) in [(6, &font::PREVIOUS), (33, play_pause), (60, &font::NEXT)] {
        canvas.sprite(x, MUSIC_CONTROLS_Y + 6, 5, icon, 3, Color::TEXT);
    }
}
