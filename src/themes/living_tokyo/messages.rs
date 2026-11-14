//! Living Tokyo's notifications, calls, and music.

use super::layout::{CALL_BUTTONS_Y, CALL_TEXT_WIDTH, MUSIC_CONTROLS_Y, MUSIC_TEXT_WIDTH};
use super::{BACKGROUND, street};
use crate::canvas::{Canvas, Color};
use crate::font;
use crate::font::fine::Style;
use crate::themes::helpers::{self, fitted};
use crate::themes::{Call, Music};

// A notification is a message from somewhere else in the city: its app
// is a neon sign hanging where the other screens hang their titles, lit
// in the app's own colour (see `neon`), with when it arrived top right
// in the history, as MUSIC's state is. Who it's from is under it, centred,
// in a face that makes it the most important thing on the screen; and
// under that, where the message is from: a ramen stall with the app's
// noren over its counter (see `render_shop`), drawn at the panel's own
// resolution, which keeps the hours and the night's weather. The banner is the same sign in short, over
// the bottom of whatever shows.
//
// Who it's from is in the city's own lettering (see `font::fine`), drawn
// at the panel's resolution, as it's written (a name in capitals and
// lowercase, as the phone has it): big, if it fits on a line or two
// without a word broken (a first name over a surname, typically), or
// else smaller, balanced over as few lines as it takes (see
// `font::wrap`). Either way it's centred, as a name on a screen of its
// own is expected to be: the balanced lines read as a block, and a short
// name doesn't sit in a corner. The sign's lettering is the same face,
// in capitals, as a neon tube a pen's width thick. (The banner, which
// has to be compact, keeps the canvas's small font: see
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

/// Who it's from, big (a line or two of about 9 letters) and smaller
/// (about 13 a line), in the panel's pixels: bold enough to be the most
/// important thing on the screen, and smoothed at their edges.
const NAME_LARGE: Style = Style {
    cap: 26,
    stroke: 68,
    caps: false,
};
const NAME_SMALL: Style = Style {
    cap: 16,
    stroke: 44,
    caps: false,
};

/// The most lines who it's from takes in the card: 40 letters at about 13
/// a line, with room to spare for long words.
const CARD_LINES: usize = 5;

/// Who it's from, filled greedily into `lines`: how many it takes (at
/// most [`CARD_LINES`]), and in which style. Big if it fits on two lines
/// without a word broken (no hyphen or ellipsis), else smaller. Not yet
/// balanced (see [`font::wrap`]), which only the passes that draw the
/// name need: it takes as many lines either way.
fn name(who: &str, lines: &mut [font::Line; CARD_LINES]) -> (usize, Style) {
    let room = BANNER_TEXT_WIDTH * 3;
    let count = font::fill(who, font::Face::Fine(NAME_LARGE), room, lines);
    if count <= 2
        && lines[..count]
            .iter()
            .all(|line| line.tail == font::Tail::None)
    {
        return (count, NAME_LARGE);
    }
    let count = font::fill(who, font::Face::Fine(NAME_SMALL), room, lines);
    (count.min(CARD_LINES), NAME_SMALL)
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

    // Who it's from, in the panel's pixels: its lines a cap and a half
    // apart, which leaves an accent over a capital clear of the line
    // above's descenders.
    let mut lines = [font::Line::default(); CARD_LINES];
    let (count, style) = name(who, &mut lines);
    let pitch = i32::from(style.cap) * 3 / 2 + style.height() - i32::from(style.cap);
    let (above, below) = style.reach();
    // The capitals centred between the sign and the shop's roof, which
    // is where the eye looks for them, but no nearer the sign than two
    // pixels above an accent. Accents above the first line, and
    // descenders below the last, are left out of the centring, as most
    // names have few. A long name pushes the roof down, the shop giving
    // up its height to it.
    let block = pitch * (count as i32 - 1) + style.height();
    let top = ((12 + SHOP_TOP - 3) * 3 / 2 - block / 2).max(3 * 12 + 2 + above);
    let roof = ((top + block + below - style.height() + 12) / 3).clamp(SHOP_TOP, SHOP_LOWEST);
    // Balanced (or cut with an ellipsis), only where it's drawn: in the
    // frame's own pass, and the passes of the bands it's in (see
    // `Canvas::refine`). Laying it out took a third of a band's pass.
    let rows = canvas.fine_rows();
    if top - above - 6 < rows.end && top + block - style.height() + below + 6 > rows.start {
        let face = font::Face::Fine(style);
        font::wrap(who, face, BANNER_TEXT_WIDTH * 3, &mut lines[..count]);
    }

    // The night over the roof, quietly, behind the name: a few stars
    // (fewer as the sky lightens), or the rain, in the far buildings'
    // dull slate. Dim, neither: idle mode keeps still.
    if !dim {
        if rain {
            street::fine_rain(canvas, 3 * 12, 3 * roof, tick, Color::TRACK);
        } else {
            let light = street::daylight(hour);
            let shown =
                CARD_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
            street::fine_stars(canvas, &CARD_STARS[..shown], None);
        }
    }
    render_shop(canvas, roof, tube, hour, rain, dim, tick);

    // The app gives way to the time: 4 px between them (after the sign's
    // tube), and an ellipsis.
    let time_width = font::small_text_width(time);
    let gap = if time.is_empty() { 0 } else { time_width + 6 };
    let mut line = [font::Line::default()];
    let room = (BANNER_TEXT_WIDTH - gap) * 3;
    let sign = font::Face::Fine(street::LETTERING);
    let width = match font::wrap(app, sign, room, &mut line) {
        0 => 0,
        _ => line[0].width(app, sign),
    };
    // The box round the lettering, in the canvas's pixels, a pixel of
    // the panel's or two clear of its end.
    street::hanging(canvas, (width + 2) / 3, tube, dim);
    if width > 0 {
        let (x, y) = street::LETTERING_AT;
        canvas.line(x, y, app, line[0], sign, tube);
    }
    canvas.text_right(BANNER_TEXT.end, 3, time, Color::TEXT_DIM);

    // Smoothed against the night (`TEXT_FAINT` between the name's
    // `TEXT` and the dark), but not in the dim look, whose eight colours
    // have no shade between white and black.
    let edge = (!dim).then_some(Color::TEXT_FAINT);
    // Each line where it's centred: its left and its width (its tail, an
    // ellipsis or a hyphen, included).
    let place = |line: font::Line| {
        let w = line.width(who, font::Face::Fine(style));
        (120 - w / 2, w)
    };
    // The rain passes behind the name, not through its letters, which
    // it would break up: a canvas pixel clear round each line, its
    // accents and descenders included. All of them first, as a line's
    // reaches into the one above's descenders.
    if rain && !dim {
        for (i, &line) in lines[..count].iter().enumerate() {
            let y = top + pitch * i as i32;
            let (x, w) = place(line);
            let (left, right) = (x / 3 - 1, (x + w + 2) / 3 + 1);
            let (high, low) = ((y - above) / 3 - 1, (y + below + 2) / 3 + 1);
            canvas.fill_rect(left, high, right - left, low - high, BACKGROUND);
        }
    }
    for (i, &line) in lines[..count].iter().enumerate() {
        let y = top + pitch * i as i32;
        let (x, _) = place(line);
        let end = canvas.fine_type(x, y, line.of(who), style, Color::TEXT, edge);
        let end = if line.end > line.start { end } else { x };
        canvas.fine_type(end, y, line.tail.as_str(), style, Color::TEXT, edge);
    }
}

/// The bar's roof, under a name of a line or two; and the lowest it goes
/// for a longer one, leaving the noren room. Canvas rows.
const SHOP_TOP: i32 = 47;
const SHOP_LOWEST: i32 = 64;

// The noodle bar itself is drawn at the panel's resolution (see
// `Canvas::fine_rect`), in its pixels: 240 a side.

/// The street, which the bar's stools and its menu board stand on.
const GROUND: i32 = 228;
/// How far the counter's top (where the bowl stands; under it, the
/// stall's front) is above the street: a share of the stall's height,
/// from its roof, so that a long name, which pushes the roof down,
/// squeezes the stall rather than burying its stools.
const COUNTER_MAX: i32 = 28;
const COUNTER_MIN: i32 = 20;
/// The stall's height (from its roof to the street) the counter has
/// [`COUNTER_MAX`] of, under a name of a line or two.
const STALL_H: i32 = 78;
/// The stall's two posts (their left columns), the noren hanging between
/// them; the roof overhangs them to `EAVES`.
const POSTS: [i32; 2] = [46, 190];
const POST_W: i32 = 4;
const EAVES: core::ops::Range<i32> = 30..210;
/// The noren (see `street::fine_noren`): where its panels start, how
/// many, and how wide each is.
const NOREN_X: i32 = 50;
const NOREN_PANELS: i32 = 5;
const NOREN_PANEL: i32 = 26;
/// The lanterns' middles, hanging from the eaves' ends.
const LANTERNS: [i32; 2] = [38, 202];
/// The stools' middles, along the counter.
const STOOLS: [i32; 4] = [74, 106, 138, 170];
/// The bowl's middle, on the counter at the third stool.
const BOWL: i32 = 138;
/// The menu board standing on the street, right of the stall: its left
/// column and width.
const MENU: (i32, i32) = (213, 22);

/// A few stars over the roof, in the corners the name leaves; as the sky
/// lightens, the last go first.
const CARD_STARS: [(i32, i32); 4] = [(70, 15), (6, 40), (74, 44), (12, 20)];

/// How the shop is at `hour`: open in the evening, brightly lit; in the
/// small hours, dimmer, its steam thinner; shut from 04:00 until the
/// evening, its shutter down, its noren taken in and its lanterns out.
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

/// The noodle bar the notification is at, from its roof (`roof`, a
/// canvas row) down, drawn at the panel's resolution: a ramen stall seen
/// from the street, its noren in the app's neon (`tube`) over the counter,
/// lit from inside as the hour has it (see [`Shop`]), a bowl steaming on
/// the counter (`tick` lifts the steam), red stools along it, a red
/// lantern at each end of the eaves, and its menu board out on the street.
/// A rainy night (`rain`) wets the street, the lights broken up in it; a
/// clear one leaves the stall's light pooled on it. In the dim look,
/// outlines: the eaves, the noren, the counter, the stools and the
/// lanterns, unlit.
fn render_shop(
    canvas: &mut Canvas,
    roof: i32,
    tube: Color,
    hour: u8,
    rain: bool,
    dim: bool,
    tick: u64,
) {
    let shop = if dim { Shop::Shut } else { Shop::at(hour) };
    let lit = shop != Shop::Shut;
    // Panel rows: the roof's top, its underside (the noren's rail), and
    // the noren's hem, which gives way to the counter for a long name.
    let top = roof * 3;
    let rail = top + 7;
    let counter = GROUND - ((GROUND - top) * COUNTER_MAX / STALL_H).clamp(COUNTER_MIN, COUNTER_MAX);
    let noren_h = (counter - rail - 14).clamp(8, 26);
    let hem = rail + noren_h;
    let (left, right) = (POSTS[0], POSTS[1] + POST_W);
    if dim {
        render_shop_dim(canvas, top, rail, hem, counter, tube);
        return;
    }

    // The stall's inside: lit as the hour has it, or shuttered: boards
    // across, dark, to the counter.
    match shop {
        Shop::Busy => {
            canvas.fine_rect(left, rail, right - left, counter - rail, Color::HEART_SHINE)
        }
        Shop::Late => {
            canvas.fine_rect(left, rail, right - left, counter - rail, Color::SHADOW);
            canvas.fine_tone(
                left,
                rail,
                right - left,
                counter - rail,
                2,
                Color::HEART_SHINE,
            );
        }
        Shop::Shut => {
            canvas.fine_rect(left, rail, right - left, counter - rail, Color::TRACK);
            for y in (rail + 3..counter).step_by(4) {
                canvas.fine_rect(left, y, right - left, 1, Color::SHADOW);
            }
        }
    }

    // Inside, behind the counter: the stock pot, a dull drum with its
    // lid, at the cook's end.
    if lit {
        let (px, pw, ph) = (58, 22, 13);
        canvas.fine_rect(px, counter - ph, pw, ph, Color::TRACK);
        canvas.fine_rect(px - 2, counter - ph, pw + 4, 2, Color::TEXT_FAINT);
        canvas.fine_rect(px - 3, counter - ph + 4, 3, 2, Color::TEXT_FAINT);
        canvas.fine_rect(px + pw, counter - ph + 4, 3, 2, Color::TEXT_FAINT);
    }

    // The roof: a dark slab, its tiles' ends along its front, its edge
    // catching the light.
    canvas.fine_rect(EAVES.start, top, EAVES.len() as i32, 7, Color::TRACK);
    canvas.fine_rect(EAVES.start, top, EAVES.len() as i32, 1, Color::TEXT_FAINT);
    for x in (EAVES.start + 2..EAVES.end - 2).step_by(6) {
        canvas.fine_rect(x, top + 2, 2, 3, Color::SHADOW);
    }
    canvas.fine_rect(
        EAVES.start + 2,
        rail - 1,
        EAVES.len() as i32 - 4,
        1,
        Color::SHADOW,
    );

    // The posts, wood: amber over the dark, lit by the stall's light.
    for x in POSTS {
        canvas.fine_rect(x, rail, POST_W, GROUND - rail, Color::SHADOW);
        if lit {
            canvas.fine_tone(x, rail, POST_W, GROUND - rail, 1, Color::HEART_SHINE);
        }
    }

    // The bowl on the counter, under the noren's hem (unless a long name
    // has squeezed the stall so far that the noren hides it).
    let bowl = lit && hem <= counter - 10;
    if bowl {
        street::fine_bowl(canvas, BOWL, counter);
    }

    // The noren, in the app's neon, slit into panels, the shop's crest
    // dyed on it; taken in when the shop's shut, its rail bare.
    canvas.fine_rect(left, rail, right - left, 1, Color::TEXT_DIM);
    if lit {
        street::fine_noren(
            canvas,
            NOREN_X,
            rail,
            noren_h,
            NOREN_PANEL,
            NOREN_PANELS,
            tube,
            false,
        );
    }

    // The bowl's steam, rising in front of the noren, thinner in the
    // small hours.
    if bowl {
        let (height, wisps) = if shop == Shop::Busy { (34, 3) } else { (20, 2) };
        let height = height.min(counter - 10 - rail);
        street::fine_steam(canvas, BOWL, counter - 10, height, wisps, tick);
    }

    // The counter: its top, lit along its edge, over the stall's front,
    // dark boards.
    canvas.fine_rect(left - 6, counter, right - left + 12, 4, Color::SHADOW);
    canvas.fine_tone(
        left - 6,
        counter,
        right - left + 12,
        4,
        2,
        Color::HEART_SHINE,
    );
    canvas.fine_rect(left - 6, counter, right - left + 12, 1, Color::TEXT_DIM);
    canvas.fine_rect(
        left,
        counter + 4,
        right - left,
        GROUND - counter - 4,
        Color::SHADOW,
    );
    for x in (left + 8..right - 4).step_by(12) {
        canvas.fine_rect(x, counter + 4, 1, GROUND - counter - 4, Color::TRACK);
    }

    // The stools: red seats, a highlight along each, on chrome posts.
    for x in STOOLS {
        let seat = counter + 9;
        canvas.fine_rect(x - 1, seat + 6, 2, GROUND - seat - 8, Color::TEXT_FAINT);
        canvas.fine_rect(x - 5, GROUND - 2, 10, 2, Color::TEXT_FAINT);
        street::fine_ellipse(canvas, x - 9, seat, 18, 7, Color::HEART);
        canvas.fine_rect(x - 6, seat, 12, 1, Color::HEART_SHINE);
    }

    for x in LANTERNS {
        street::fine_lantern(canvas, x, rail + 4, lit);
    }

    // The menu board, out on the street: a dark board on its legs, a bowl
    // drawn at its head and the dishes' lines, chalk-white, under it.
    let (mx, mw) = MENU;
    let board = GROUND - 30;
    street::fine_line(
        canvas,
        (mx + 2, board + 20),
        (mx - 1, GROUND - 1),
        Color::TEXT_FAINT,
    );
    street::fine_line(
        canvas,
        (mx + mw - 3, board + 20),
        (mx + mw, GROUND - 1),
        Color::TEXT_FAINT,
    );
    canvas.fine_rect(mx, board, mw, 22, Color::TEXT_FAINT);
    canvas.fine_rect(mx + 1, board + 1, mw - 2, 20, Color::SHADOW);
    canvas.fine_rect(mx + 7, board + 4, 8, 2, Color::HEART);
    canvas.fine_rect(mx + 8, board + 6, 6, 1, Color::HEART);
    for (row, w) in [(10, 14), (14, 10), (18, 12)] {
        canvas.fine_rect(mx + 4, board + row, w, 1, Color::TEXT_DIM);
    }

    // The street: wet, the stall's lights broken up in it (once it's
    // shut, only the wet's own sheen); dry, the light pooled on it.
    canvas.fine_rect(0, GROUND, 240, 1, Color::TRACK);
    if rain {
        if lit {
            street::fine_reflection(
                canvas,
                left,
                right - left,
                GROUND + 3,
                5,
                Color::HEART_SHINE,
            );
            for x in LANTERNS {
                street::fine_reflection(canvas, x - 5, 10, GROUND + 3, 5, Color::HEART);
            }
        } else {
            street::fine_reflection(canvas, 6, 228, GROUND + 3, 4, Color::TEXT_FAINT);
        }
    } else if lit {
        // Fanning out from the counter, fading.
        for row in 0..5 {
            let spread = 4 + 3 * row;
            let (x, w) = (left - spread, right - left + 2 * spread);
            let level = if row < 1 { 2 } else { 1 };
            canvas.fine_tone(x, GROUND + 1 + row, w, 1, level, Color::HEART_SHINE);
        }
    }
}

/// The bar in the dim look (idle mode's eight colours, still): its
/// outlines in the structure's red and the noren's in the app's colour,
/// and its lanterns, unlit.
fn render_shop_dim(canvas: &mut Canvas, top: i32, rail: i32, hem: i32, counter: i32, tube: Color) {
    let line = Color::TEXT_FAINT;
    let (left, right) = (POSTS[0], POSTS[1] + POST_W);
    canvas.fine_rect(EAVES.start, top, EAVES.len() as i32, 1, line);
    canvas.fine_rect(EAVES.start, rail - 1, EAVES.len() as i32, 1, line);
    for x in POSTS {
        canvas.fine_rect(x, rail, 1, GROUND - rail, line);
        canvas.fine_rect(x + POST_W - 1, rail, 1, GROUND - rail, line);
    }
    street::fine_noren(
        canvas,
        NOREN_X,
        rail,
        hem - rail,
        NOREN_PANEL,
        NOREN_PANELS,
        tube,
        true,
    );
    canvas.fine_rect(left - 6, counter, right - left + 12, 1, line);
    for x in LANTERNS {
        street::fine_lantern(canvas, x, rail + 4, false);
    }
    for x in STOOLS {
        canvas.fine_rect(x - 8, counter + 9, 16, 1, line);
        canvas.fine_rect(x - 1, counter + 10, 1, GROUND - counter - 10, line);
    }
    canvas.fine_rect(0, GROUND, 240, 1, line);
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
    street::fine_sign(canvas, 0, 3 * top, 240, 3 * (80 - top), tube);
    // The app in a sign's lettering, in its rows (see above), a pixel
    // down as on a title.
    fitted(
        canvas,
        (3 * BANNER_TEXT.start, 3 * (top + 2) + 1),
        app,
        font::Face::Fine(street::LETTERING),
        3 * BANNER_TEXT_WIDTH,
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

// ==============================================================================
// A call: a phone booth on a corner
// ==============================================================================

// A call is a phone ringing in a booth on a street corner: the sign over
// the corner says so, in the green of Japan's public phones, with who's
// calling big under it; below, the booth itself, lit, the phone on its
// back wall, with the far city low behind it; and in front, decline and
// answer as two lit keys, where the UI takes taps on them.
//
// Its one living thing is the handset: it jumps off its hook with each
// ring, a second on and a second off; silenced, it's still and the
// booth's light goes down. The hour and the night are quiet: the far
// city's windows lit as its people are up, and either a little rain
// past the booth (the wet ground under it holding its light) or a few
// stars over it, its light pooled on the dry ground. In the dim look,
// the booth's outline and the keys.
//
// Who's calling is in the city's lettering, as on a notification's card,
// on a line, big or smaller, if it fits on one: then it never scrolls
// (see `call_overflow`). A longer name scrolls at 2× in the canvas's
// small font, as a fine row would be drawn again and sent whole ten
// times a second (see `Canvas::refine`). The booth is drawn at the
// panel's resolution, the keys on the canvas.

/// The booth, in the panel's pixels: its left, its roof's top, its width
/// and its floor (the ground, which the far city stands on too).
const BOOTH: (i32, i32, i32, i32) = (98, 105, 44, 162);
/// The keys: their top (a little under where the taps on them start,
/// [`CALL_BUTTONS_Y`], which take the whole half either side), height
/// and width.
const KEY_Y: i32 = CALL_BUTTONS_Y + 3;
const KEY_H: i32 = 15;
const KEY_W: i32 = 32;
/// The middle of who's calling, lettered finely: the panel's row.
const CALLER_Y: i32 = 64;

/// Stars either side of the booth, over the far city.
const BOOTH_STARS: [(i32, i32); 4] = [(9, 39), (66, 38), (24, 42), (75, 44)];

/// The size who's calling is lettered in, if it fits on a line: the
/// card's big or its smaller (see [`NAME_LARGE`]); or none, and it
/// scrolls.
fn caller_style(caller: &str) -> Option<Style> {
    [NAME_LARGE, NAME_SMALL]
        .into_iter()
        .find(|&style| font::Face::Fine(style).width(caller) <= 3 * CALL_TEXT_WIDTH)
}

/// How far who's calling overflows its room (see `Theme::call_overflow`):
/// not at all if it's lettered finely, which never scrolls.
pub(super) fn call_overflow(caller: &str) -> i32 {
    match caller_style(caller) {
        Some(_) => 0,
        None => helpers::overflow(caller, 2, CALL_TEXT_WIDTH),
    }
}

/// Who's calling is in columns 4..76 (see [`CALL_TEXT_WIDTH`]); the
/// decline and answer keys are on the halves where the UI takes taps on
/// them.
pub(super) fn render_call(canvas: &mut Canvas, call: &Call, dim: bool) {
    canvas.clear(BACKGROUND);
    let night = street::Night::now();
    let ringing = !call.silenced;
    let (_, top, _, floor) = BOOTH;

    // Behind the booth: a little rain, or a few stars (fewer as the sky
    // lightens), and the far city, low.
    if !dim {
        if night.rain {
            street::fine_rain(canvas, top, floor, night.tick, Color::TEXT_FAINT);
        } else {
            let light = street::daylight(night.hour);
            let shown =
                BOOTH_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
            street::fine_stars(canvas, &BOOTH_STARS[..shown], None);
        }
        street::fine_skyline(
            canvas,
            0,
            240,
            floor,
            3 * 7,
            street::city_lights(night.hour),
        );
    }
    canvas.fine_rect(0, floor, 240, 2, Color::TRACK);

    // The corner's sign, then who's calling: lettered if it fits, else
    // scrolling at 2×.
    street::sign_title(canvas, "INCOMING CALL", Color::GOOD, Color::GOOD, dim);
    match caller_style(call.caller) {
        Some(style) => {
            let w = font::Face::Fine(style).width(call.caller);
            let edge = (!dim).then_some(Color::TEXT_FAINT);
            let (x, y) = (120 - w / 2, CALLER_Y - style.height() / 2);
            canvas.fine_type(x, y, call.caller, style, Color::TEXT, edge);
        }
        None => canvas.marquee(
            4..4 + CALL_TEXT_WIDTH,
            18,
            call.caller,
            Color::TEXT,
            2,
            call.elapsed,
        ),
    }
    let hint = if ringing {
        "BUTTON: SILENCE"
    } else {
        "SILENCED"
    };
    canvas.text_centered(40, 30, hint, Color::TEXT_FAINT);

    render_booth(canvas, ringing, call.elapsed.as_secs(), night.rain, dim);

    // The keys: lit, red to decline and green to answer, their corners
    // rounded off.
    for (x, label, color) in [(4, "DECLINE", Color::BAD), (44, "ANSWER", Color::GOOD)] {
        canvas.fill_rect(x + 1, KEY_Y, KEY_W - 2, KEY_H, color);
        canvas.fill_rect(x, KEY_Y + 1, KEY_W, KEY_H - 2, color);
        canvas.text_centered(x + KEY_W / 2, KEY_Y + 5, label, BACKGROUND);
    }
}

/// The booth, at the panel's resolution: a glass box under a roof with
/// a green band, lit from inside while it rings (the light down once
/// silenced), the green phone on its back wall, and its handset jumping
/// off its hook on every other second of `seconds` while it rings. Its
/// light lies on the ground in front: broken up on a wet night, pooled
/// on a dry one. In the dim look, its outline, the phone and the
/// handset, unlit.
fn render_booth(canvas: &mut Canvas, ringing: bool, seconds: u64, rain: bool, dim: bool) {
    let (x, top, w, floor) = BOOTH;
    let glass = top + 9;

    // Its light, hazing round it while it rings.
    if ringing && !dim {
        street::fine_glow(
            canvas,
            x - 16,
            top + 2,
            w + 32,
            floor - top,
            Color::TEXT_DIM,
        );
    }
    // The glass: lit as bright as it rings (or not at all, dim); the
    // panel along its foot, and a rail across it.
    canvas.fine_rect(x, glass, w, floor - glass, Color::SHADOW);
    if !dim {
        let light = if ringing { 2 } else { 1 };
        canvas.fine_tone(x, glass, w, floor - glass, light, Color::TEXT_DIM);
    }
    let frame = Color::TEXT_FAINT;
    canvas.fine_rect(x, floor - 8, w, 8, Color::TRACK);
    canvas.fine_rect(x, floor - 22, w, 1, frame);
    // The posts, either side and between the door's two panes.
    for post in [x, x + w / 2 - 1, x + w - 2] {
        canvas.fine_rect(post, glass, 2, floor - glass, frame);
    }
    // The roof, overhanging, over its band in the phones' green (dark in
    // the dim look, half lit once silenced).
    canvas.fine_rect(x - 3, top, w + 6, 4, Color::TRACK);
    canvas.fine_rect(x - 3, top, w + 6, 1, frame);
    canvas.fine_rect(x, top + 4, w, 5, Color::SHADOW);
    if !dim {
        let band = if ringing { 4 } else { 2 };
        canvas.fine_tone(x, top + 4, w, 5, band, Color::GOOD);
    }

    // The phone: a green box on the back wall, on a shelf, its little
    // display and its keypad dark; its handset hung on its left.
    let (px, py, pw, ph) = (x + 18, glass + 9, 14, 20);
    canvas.fine_rect(x + 4, py + ph, w - 8, 2, frame);
    canvas.fine_rect(px, py, pw, ph, Color::GOOD);
    canvas.fine_rect(px + 3, py + 2, 8, 3, Color::SHADOW);
    for row in 0..4 {
        for column in 0..3 {
            canvas.fine_rect(px + 3 + 3 * column, py + 8 + 3 * row, 2, 2, Color::SHADOW);
        }
    }
    // The handset: an earpiece and a mouthpiece on a grip, on its hook;
    // on a ring, jumped up off it and tipped out, the ring's marks beside
    // it.
    let jump = ringing && !dim && seconds % 2 == 1;
    let (hx, hy, tip) = if jump {
        (px - 7, py - 5, -3)
    } else {
        (px - 7, py, 0)
    };
    let handset = if dim { Color::TEXT } else { Color::SHADOW };
    canvas.fine_rect(hx - 1 + tip, hy, 6, 4, handset);
    canvas.fine_rect(hx - 1, hy + 15, 6, 4, handset);
    for dx in 0..3 {
        street::fine_line(canvas, (hx + tip + dx, hy + 3), (hx + dx, hy + 15), handset);
    }
    if jump {
        for (from, to) in [
            ((-5, -2), (-9, -5)),
            ((-6, 3), (-10, 3)),
            ((-5, 8), (-9, 11)),
        ] {
            let at = |(dx, dy): (i32, i32)| (hx + tip + dx, hy + dy);
            street::fine_line(canvas, at(from), at(to), Color::SHADOW);
        }
    }

    // Its light on the ground in front: broken up on a wet night, pooled
    // on a dry one, fading out from the booth.
    if !dim {
        if rain {
            let color = if ringing {
                Color::TEXT_DIM
            } else {
                Color::TRACK
            };
            street::fine_reflection(canvas, x, w, floor + 2, 3, color);
        } else {
            let level = if ringing { 2 } else { 1 };
            for row in 0..4 {
                let spread = 2 + 3 * row;
                let level = if row < 2 { level } else { 1 };
                canvas.fine_tone(
                    x - spread,
                    floor + 2 + row,
                    w + 2 * spread,
                    1,
                    level,
                    Color::TEXT_DIM,
                );
            }
        }
    }
}

// ==============================================================================
// Music: a record bar
// ==============================================================================

// What the phone plays is a record playing in a small bar: its sign
// (MUSIC) hanging top left, the artist and the track on the marquee
// under it, and below, the turntable under the bar's lamp, with the
// controls on and beside it: previous and next either side, and play or
// pause on the record's label, the middle third that a tap on it takes.
//
// Its one living thing is the record: while the music plays, the sign is
// lit, the tonearm is on the record and the lamp's sheen goes round it, a
// step a second; paused, the sign goes dark, the arm lifts back to its
// rest and the record stops. The hour and the night are quieter: the lamp
// over the turntable keeps the noodle bar's hours (bright in the evening,
// low in the small hours, out from 04:00: see [`Shop`]), its light pooled
// on the plinth while it's on, and over the marquee a little rain falls,
// or a few stars are out. In the dim look, the shapes and what they show,
// still.
//
// The turntable is drawn at the panel's resolution, the marquee on the
// canvas. A long name scrolls ten times a second, and a fine row is drawn
// again with every frame and sent whole whenever it changes (see
// `Canvas::refine`): the marquee's rows have no fine drawing, so a scroll
// sends only the canvas's narrow spans, and the turntable's bands, drawn
// again each frame, are sent only as the record turns, once a second.

/// The marquee's box: its top row, and its height.
const MARQUEE_TOP: i32 = 14;
const MARQUEE_H: i32 = 29;
/// The artist's and the track's rows on it (their capitals' tops).
const ARTIST_Y: i32 = 19;
const TRACK_Y: i32 = 31;

// The turntable is in the panel's pixels.

/// The record's middle, under the middle third's taps (the play or pause
/// is on its label), its radius, and its label's.
const RECORD: (i32, i32) = (120, 180);
const RECORD_R: i32 = 31;
const LABEL_R: i32 = 11;
/// The plinth: its left, top, width and height. Its rows stop short of
/// the canvas's band from row 72, which then has no fine rows to draw
/// again with every frame of a scroll.
const PLINTH: (i32, i32, i32, i32) = (74, 143, 94, 71);
/// The tonearm's pivot, at the plinth's top right; and where its stylus
/// is: on the record's grooves while it plays, on its rest beside the
/// record while it doesn't.
const PIVOT: (i32, i32) = (157, 154);
const ON_RECORD: (i32, i32) = (138, 191);
const ON_REST: (i32, i32) = (160, 198);
/// The lamp, hanging from the marquee's foot over the turntable: its
/// shade's middle, and its foot.
const LAMP: (i32, i32) = (120, 140);
/// Previous and next, either side of the plinth: their left columns, and
/// how tall each is.
const SKIPS: [i32; 2] = [34, 187];
const SKIP_H: i32 = 27;
const _: () = assert!(
    RECORD.1 - SKIP_H / 2 >= 3 * MUSIC_CONTROLS_Y,
    "the controls are where their taps are"
);

/// Stars right of the sign, the last to go first as the sky lightens.
const BAR_STARS: [(i32, i32); 4] = [(47, 3), (66, 8), (36, 9), (75, 2)];

/// Which way the lamp's sheen on the record points, a step a second:
/// eight steps round, in sixteenths of a radius.
const TURN: [(i32, i32); 8] = [
    (0, -16),
    (11, -11),
    (16, 0),
    (11, 11),
    (0, 16),
    (-11, 11),
    (-16, 0),
    (-11, -11),
];

/// The artist and the track scroll within columns 3..77 (see
/// [`MUSIC_TEXT_WIDTH`]); the controls are where the UI takes taps on
/// them, in thirds from [`MUSIC_CONTROLS_Y`].
pub(super) fn render_music(canvas: &mut Canvas, music: &Music, dim: bool) {
    canvas.clear(BACKGROUND);
    let night = street::Night::now();
    let on = music.playing == Some(true);

    // The night over the marquee: a little rain, or a few stars.
    if !dim {
        if night.rain {
            street::fine_rain(
                canvas,
                0,
                3 * (MARQUEE_TOP - 1),
                night.tick,
                Color::TEXT_FAINT,
            );
        } else {
            let light = street::daylight(night.hour);
            let shown =
                BAR_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
            street::fine_stars(canvas, &BAR_STARS[..shown], None);
        }
    }

    // The sign: lit in the bar's neon unless the music is paused, then
    // dark (and still legible).
    let (tube, ink) = match music.playing {
        Some(false) => (Color::TRACK, Color::TEXT_FAINT),
        _ => (Color::REM, Color::REM),
    };
    street::sign_title(canvas, "MUSIC", tube, ink, dim);

    // The marquee: a dark box in a dull frame, what's playing lit on it,
    // all on the canvas (see above).
    canvas.fill_rect(1, MARQUEE_TOP, 78, MARQUEE_H, Color::TRACK);
    canvas.fill_rect(2, MARQUEE_TOP + 1, 76, MARQUEE_H - 2, Color::SHADOW);
    let columns = 3..3 + MUSIC_TEXT_WIDTH;
    let elapsed = music.elapsed;
    canvas.marquee(
        columns.clone(),
        ARTIST_Y,
        music.artist,
        Color::TEXT_DIM,
        1,
        elapsed,
    );
    canvas.marquee(columns, TRACK_Y, music.track, Color::TEXT, 2, elapsed);

    let lamp = if dim {
        Shop::Shut
    } else {
        Shop::at(night.hour)
    };
    let step = if on {
        night.tick % TURN.len() as u64
    } else {
        0
    };
    render_turntable(canvas, on, lamp, step as usize, dim);

    // Previous and next either side, in the text's colour: a bar, and a
    // triangle pointing away from the record.
    let (_, cy) = RECORD;
    let (top, half) = (cy - SKIP_H / 2, SKIP_H / 2);
    let [previous, next] = SKIPS;
    canvas.fine_rect(previous, top, 4, SKIP_H, Color::TEXT);
    arrow(canvas, previous + 5 + half, top, SKIP_H, -1, Color::TEXT);
    arrow(canvas, next, top, SKIP_H, 1, Color::TEXT);
    canvas.fine_rect(next + half + 1, top, 4, SKIP_H, Color::TEXT);
}

/// A triangle `h` rows tall (odd) from `top`, its flat side on column
/// `x`, pointing `towards` the right (1) or the left (-1), its tip
/// half its height away.
fn arrow(canvas: &mut Canvas, x: i32, top: i32, h: i32, towards: i32, color: Color) {
    for row in 0..h {
        let w = h / 2 + 1 - (row - h / 2).abs();
        let left = if towards > 0 { x } else { x - w + 1 };
        canvas.fine_rect(left, top + row, w, 1, color);
    }
}

/// The turntable, at the panel's resolution: the lamp over it (lit as
/// `lamp` has it), the plinth, the record (its sheen pointing `step` of
/// [`TURN`] while it plays, and the notch in its label so), the tonearm
/// (on the record while it's `on`, on its rest otherwise), and play or
/// pause on the label, whichever a tap does.
fn render_turntable(canvas: &mut Canvas, on: bool, lamp: Shop, step: usize, dim: bool) {
    let (px, py, pw, ph) = PLINTH;
    let (cx, cy) = RECORD;
    let lit = lamp != Shop::Shut;

    // The lamp, on a short cord from the marquee: a dark shade, wide at
    // its foot, its bulb under it lit as the bar's hours have it, and in
    // the evening, a haze round it.
    let (lx, ly) = LAMP;
    if lamp == Shop::Busy {
        street::fine_glow(canvas, lx - 22, ly - 12, 44, 14, Color::HEART_SHINE);
    }
    let cord = 3 * (MARQUEE_TOP + MARQUEE_H);
    canvas.fine_rect(lx, cord, 1, ly - 9 - cord, Color::TEXT_FAINT);
    for row in 0..9 {
        let half = 3 + row;
        canvas.fine_rect(lx - half, ly - 9 + row, 2 * half + 1, 1, Color::TRACK);
    }
    canvas.fine_rect(lx - 3, ly - 9, 7, 1, Color::TEXT_FAINT);
    let bulb = match lamp {
        Shop::Busy => Color::HEART_SHINE,
        Shop::Late => Color::HEART,
        Shop::Shut => Color::TEXT_FAINT,
    };
    street::fine_ellipse(canvas, lx - 4, ly - 1, 9, 5, bulb);

    // The plinth, dark wood, its grain faint, in a faint edge, its
    // corners rounded; and the lamp's light pooled on it, wider while
    // it's bright.
    canvas.fine_rect(px, py, pw, ph, Color::TEXT_FAINT);
    canvas.fine_rect(px + 1, py + 1, pw - 2, ph - 2, Color::SHADOW);
    // Its grain is a half-tone in the dim look, which keeps to shapes.
    if !dim {
        for y in (py + 4..py + ph - 2).step_by(6) {
            canvas.fine_rect(px + 3, y, pw - 6, 1, Color::TRACK);
        }
    }
    for (x, y) in [
        (px, py),
        (px + pw - 1, py),
        (px, py + ph - 1),
        (px + pw - 1, py + ph - 1),
    ] {
        canvas.fine_pixel(x, y, BACKGROUND);
    }
    if lit {
        let (w, h) = if lamp == Shop::Busy {
            (96, 60)
        } else {
            (64, 36)
        };
        street::fine_glow(canvas, lx - w / 2, py + 1, w, h, Color::HEART_SHINE);
    }

    // The record on its platter's metal rim: black, its grooves catching
    // the light in rings, its label in the bar's neon.
    let disc = |canvas: &mut Canvas, r: i32, color: Color| {
        street::fine_ellipse(canvas, cx - r, cy - r, 2 * r, 2 * r, color);
    };
    disc(canvas, RECORD_R + 2, Color::TEXT_FAINT);
    disc(canvas, RECORD_R, Color::SHADOW);
    if !dim {
        for r in [27, 22, 17] {
            disc(canvas, r, Color::TRACK);
            disc(canvas, r - 1, Color::SHADOW);
        }
    }
    disc(canvas, LABEL_R, Color::REM);
    if !dim {
        // The lamp's sheen across it, as two opposite streaks along a
        // radius, the far one fainter, going round while it plays; and the
        // notch in the label's edge, turning with it.
        let (dx, dy) = TURN[step];
        let at = |r: i32, sign: i32| (cx + sign * dx * r / 16, cy + sign * dy * r / 16);
        if on {
            for (sign, color) in [(1, Color::TEXT_DIM), (-1, Color::TEXT_FAINT)] {
                street::fine_line(canvas, at(LABEL_R + 3, sign), at(RECORD_R - 2, sign), color);
                let (x, y) = at(LABEL_R + 5, sign);
                let (x1, y1) = at(RECORD_R - 4, sign);
                street::fine_line(canvas, (x + 1, y), (x1 + 1, y1), color);
            }
        }
        let (x, y) = at(LABEL_R - 2, 1);
        canvas.fine_rect(x - 1, y - 1, 2, 2, Color::SHADOW);
    }

    // The tonearm, from its pivot: its counterweight behind, the arm a
    // tube two pixels thick, the headshell at its end; on the record's
    // grooves while it plays, or lifted back to its rest.
    let (ax, ay) = PIVOT;
    let end = if on { ON_RECORD } else { ON_REST };
    canvas.fine_rect(ON_REST.0 - 2, ON_REST.1 + 1, 5, 4, Color::TEXT_FAINT);
    canvas.fine_rect(ax - 2, ay - 12, 6, 6, Color::TEXT_FAINT);
    canvas.fine_rect(ax, ay - 6, 2, 6, Color::TEXT_DIM);
    for dx in [0, 1] {
        street::fine_line(canvas, (ax + dx, ay), (end.0 + dx, end.1), Color::TEXT_DIM);
    }
    canvas.fine_rect(end.0 - 2, end.1 - 2, 5, 5, Color::TEXT);
    street::fine_ellipse(canvas, ax - 4, ay - 4, 10, 10, Color::TEXT_FAINT);
    canvas.fine_rect(ax, ay, 2, 2, Color::SHADOW);

    // Play or pause, printed on the label in its dark ink.
    if on {
        for x in [cx - 5, cx + 2] {
            canvas.fine_rect(x, cy - 6, 3, 12, Color::SHADOW);
        }
    } else {
        arrow(canvas, cx - 3, cy - 6, 13, 1, Color::SHADOW);
    }
}
