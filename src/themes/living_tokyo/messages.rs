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
// under that, where the message is from: a ramen stall with the app's
// noren over its counter (see `render_shop`), drawn at the panel's own
// resolution, which keeps the hours and the night's weather. The banner is the same sign in short, over
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

    // The night over the roof, quietly, behind the name: a few stars
    // (fewer as the sky lightens), or the rain, in the far buildings'
    // dull slate. Dim, neither: idle mode keeps still.
    if !dim {
        if rain {
            street::rain(canvas, 12, roof, tick, Color::TRACK);
        } else {
            let light = street::daylight(hour);
            let shown =
                CARD_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
            street::stars(canvas, &CARD_STARS[..shown]);
        }
    }
    render_shop(canvas, roof, tube, hour, rain, dim, tick);

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
/// The noren (see `street::fine_noren`): its panels, how wide each is,
/// and what's dyed on them, a letter a panel, as noren have their shop's
/// name.
const NOREN_X: i32 = 50;
const NOREN_PANEL: i32 = 26;
const NOREN_LETTERS: [&str; 5] = ["R", "A", "M", "E", "N"];
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
/// outlines: the eaves, the noren and its letters, the counter, the
/// stools and the lanterns, unlit.
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
        let rim = counter - 8;
        // Chopsticks across it, and the noodles' heap.
        street::fine_line(
            canvas,
            (BOWL - 13, rim - 3),
            (BOWL + 12, rim + 1),
            Color::TEXT_DIM,
        );
        street::fine_line(
            canvas,
            (BOWL - 13, rim - 2),
            (BOWL + 12, rim + 2),
            Color::TEXT_DIM,
        );
        // A black bowl, its red band, wide at the rim and narrow at the
        // foot.
        for (dy, half) in [
            (0, 10),
            (1, 10),
            (2, 9),
            (3, 9),
            (4, 8),
            (5, 7),
            (6, 6),
            (7, 4),
        ] {
            let color = if dy == 2 { Color::HEART } else { Color::SHADOW };
            canvas.fine_rect(BOWL - half, rim + dy, 2 * half, 1, color);
        }
        canvas.fine_rect(BOWL - 9, rim - 1, 18, 1, Color::TEXT_DIM);
    }

    // The noren, in the app's neon, slit into panels, its letters dyed in
    // the dark; taken in when the shop's shut, its rail bare.
    canvas.fine_rect(left, rail, right - left, 1, Color::TEXT_DIM);
    if lit {
        street::fine_noren(
            canvas,
            NOREN_X,
            rail,
            noren_h,
            NOREN_PANEL,
            &NOREN_LETTERS,
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
/// its letters solid, and its lanterns, unlit.
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
        &NOREN_LETTERS,
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

// ==============================================================================
// A call: a phone booth on a corner
// ==============================================================================

// A call is a phone ringing in a booth on a street corner: the sign over
// the corner says so, in the green of Japan's public phones, with who's
// calling big under it; below, the booth itself, lit, the phone on its
// back wall, with the far city low behind it; and in front, decline and
// answer as two lit keys, where the UI takes taps on them.
//
// Its one living thing is the handset: it jumps on its hook with each
// ring, a second on and a second off; silenced, it's still and the
// booth's light goes down. The hour and the night are quiet: the far
// city's windows lit as its people are up, and either a little rain
// past the booth (the wet ground under it holding its light) or a few
// stars over it. In the dim look, the booth's outline and the keys.

/// The booth: its left post, its width, its roof's row and its floor
/// (the ground line, which the far city stands on too).
const BOOTH_X: i32 = 29;
const BOOTH_W: i32 = 22;
const BOOTH_TOP: i32 = 37;
const BOOTH_FLOOR: i32 = 54;
/// The keys: their top (a little under where the taps on them start,
/// [`CALL_BUTTONS_Y`], which take the whole half either side), height
/// and width.
const KEY_Y: i32 = CALL_BUTTONS_Y + 3;
const KEY_H: i32 = 15;
const KEY_W: i32 = 32;

/// Stars either side of the booth, over the far city.
const BOOTH_STARS: [(i32, i32); 4] = [(9, 39), (66, 38), (24, 42), (75, 44)];

/// The caller's name scrolls within columns 4..76 (see
/// [`CALL_TEXT_WIDTH`]); the decline and answer keys are on the halves
/// where the UI takes taps on them.
pub(super) fn render_call(canvas: &mut Canvas, call: &Call, dim: bool) {
    canvas.clear(BACKGROUND);
    let night = street::Night::now();
    let ringing = !call.silenced;

    // Behind the booth: a little rain, or a few stars (fewer as the sky
    // lightens), and the far city, low.
    if !dim {
        if night.rain {
            street::rain(
                canvas,
                BOOTH_TOP - 1,
                BOOTH_FLOOR,
                night.tick,
                Color::TEXT_FAINT,
            );
        } else {
            let light = street::daylight(night.hour);
            let shown =
                BOOTH_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
            street::stars(canvas, &BOOTH_STARS[..shown]);
        }
        street::skyline(
            canvas,
            0,
            80,
            BOOTH_FLOOR,
            7,
            street::city_lights(night.hour),
        );
    }
    canvas.fill_rect(0, BOOTH_FLOOR, 80, 1, Color::TRACK);

    // The corner's sign, then who's calling, at 2×, centred if it fits,
    // scrolling if not.
    street::title(canvas, "INCOMING CALL", Color::GOOD);
    let width = font::small_text_width(call.caller) * 2;
    let left = if width <= CALL_TEXT_WIDTH {
        40 - width / 2
    } else {
        4
    };
    canvas.marquee(
        left..4 + CALL_TEXT_WIDTH,
        18,
        call.caller,
        Color::TEXT,
        2,
        call.elapsed,
    );
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

/// The booth: a glass box under a green-lit roof, its light on while it
/// rings (down once silenced), the phone on its back wall, and its
/// handset jumping on its hook on every other second of `seconds` while
/// it rings. On a rainy night, its light lies on the wet ground in front.
fn render_booth(canvas: &mut Canvas, ringing: bool, seconds: u64, rain: bool, dim: bool) {
    let (x, w, top, floor) = (BOOTH_X, BOOTH_W, BOOTH_TOP, BOOTH_FLOOR);
    let glass = top + 2;
    // The glass: lit, or nearly dark; and in the dim look, unlit.
    canvas.fill_rect(x, glass, w, floor - glass, Color::SHADOW);
    if !dim {
        let light = if ringing { 1 } else { 3 };
        for gy in glass..floor {
            for gx in x + 1..x + w - 1 {
                if (gx + gy) % 2 == 0 && (gy - glass) % light == 0 {
                    canvas.pixel(gx, gy, Color::TEXT_DIM);
                }
            }
        }
    }
    // Its frame: the posts, a rail across the glass, and the roof, its
    // edge lit green (dark in the dim look).
    for post in [x, x + w - 1] {
        canvas.fill_rect(post, glass, 1, floor - glass, Color::TEXT_FAINT);
    }
    canvas.fill_rect(x, floor - 5, w, 1, Color::TEXT_FAINT);
    canvas.fill_rect(x - 1, top, w + 2, 1, Color::TEXT_FAINT);
    let roof = if dim { Color::TRACK } else { Color::GOOD };
    canvas.fill_rect(x, top + 1, w, 1, roof);

    // The phone: a green box on the back wall, its keypad a dark patch,
    // and the handset on its hook at its left: on a ring, jumped up off
    // it and tipped out, with the ring's two marks beside it.
    let (px, py) = (x + 10, glass + 2);
    canvas.fill_rect(px, py, 6, 9, Color::GOOD);
    canvas.fill_rect(px + 1, py + 4, 4, 4, Color::SHADOW);
    let jump = ringing && !dim && seconds % 2 == 1;
    let (hx, hy) = if jump { (px - 4, py - 2) } else { (px - 3, py) };
    let handset = if dim { Color::TEXT } else { Color::SHADOW };
    canvas.fill_rect(hx + 1, hy, 2, 9, handset);
    canvas.fill_rect(hx, hy, 1, 2, handset);
    canvas.fill_rect(hx, hy + 7, 1, 2, handset);
    if jump {
        for (mx, my) in [(hx - 2, hy + 1), (hx - 2, hy + 6)] {
            canvas.fill_rect(mx, my, 1, 2, Color::TEXT);
        }
    }

    // Its light on the wet ground in front.
    if rain && !dim {
        let color = if ringing {
            Color::TEXT_DIM
        } else {
            Color::TRACK
        };
        street::reflection(canvas, x, w, floor + 1, 2, color);
    }
}

// ==============================================================================
// Music: a record bar
// ==============================================================================

// What the phone plays is a record playing in a small bar: its sign
// (MUSIC) hanging top left, the artist and the track on the marquee
// under it, and below, the turntable the controls are on: previous and
// next either side, and play or pause on the record's label, the middle
// third that a tap on it takes.
//
// Its one living thing is the record: while the music plays, the sign is
// lit, the tonearm is on the record and a sheen goes round it, a step a
// second; paused, the sign goes dark, the arm lifts to its rest and the
// record stops. The hour and the night are quieter: the bar's lamp warms
// the record (bright in the evening, low in the small hours, out from
// 04:00, as the noodle bar keeps its hours: see [`Shop`]), and over the
// marquee a little rain falls, or a few stars are out. In the dim look,
// the shapes and what they show, still.

/// The marquee's box: its top row, and its height.
const MARQUEE_TOP: i32 = 14;
const MARQUEE_H: i32 = 29;
/// The artist's and the track's rows on it (their capitals' tops).
const ARTIST_Y: i32 = 19;
const TRACK_Y: i32 = 31;
/// The record: its box's top left and its diameter, and its label's, so
/// that both are centred on (40, 63), the play or pause on the label.
const RECORD: (i32, i32, i32) = (27, 50, 26);
const LABEL: (i32, i32, i32) = (32, 55, 16);
/// The tonearm's pivot, right of the record.
const ARM: (i32, i32) = (57, 52);

/// Stars right of the sign, the last to go first as the sky lightens.
const BAR_STARS: [(i32, i32); 4] = [(47, 3), (66, 8), (36, 9), (75, 2)];

/// Where the sheen on the record is, a step a second: eight steps round,
/// a short streak along a radius each (from the record's centre).
const SHEEN: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
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
            street::rain(canvas, 0, MARQUEE_TOP - 1, night.tick, Color::TEXT_FAINT);
        } else {
            let light = street::daylight(night.hour);
            let shown =
                BAR_STARS.len() * (street::DAYLIGHT - light) as usize / street::DAYLIGHT as usize;
            street::stars(canvas, &BAR_STARS[..shown]);
        }
    }

    // The sign: lit in the bar's neon unless the music is paused, then
    // dark (and still legible).
    if music.playing == Some(false) {
        street::hanging(canvas, font::small_text_width("MUSIC"), Color::TRACK);
        canvas.text(4, 3, "MUSIC", Color::TEXT_FAINT);
    } else {
        street::title(canvas, "MUSIC", Color::REM);
    }

    // The marquee: a dark box in a dull frame, what's playing lit on it.
    street::sign(canvas, 1, MARQUEE_TOP, 78, MARQUEE_H, Color::TRACK);
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

    // The turntable, in the bar's lamplight while the bar's open.
    let (rx, ry, rd) = RECORD;
    let (lx, ly, ld) = LABEL;
    let (cx, cy) = (rx + rd / 2, ry + rd / 2);
    let lamp = match Shop::at(night.hour) {
        Shop::Busy => 3,
        Shop::Late => 2,
        Shop::Shut => 0,
    };
    if !dim && lamp > 0 {
        street::glow(canvas, rx, ry, rd, lamp, Color::HEART_SHINE);
    }
    // The record: a black disc with a rim and a groove, its label in the
    // bar's neon.
    street::disc(canvas, rx, ry, rd, Color::TRACK);
    street::disc(canvas, rx + 1, ry + 1, rd - 2, Color::SHADOW);
    if !dim {
        street::disc(canvas, rx + 5, ry + 5, rd - 10, Color::TRACK);
        street::disc(canvas, rx + 6, ry + 6, rd - 12, Color::SHADOW);
    }
    street::disc(canvas, lx, ly, ld, Color::REM);
    // Where it has turned to: a step a second while it plays; stopped,
    // at rest (and in the dim look, which holds still, not drawn).
    if !dim {
        let step = if on {
            night.tick % SHEEN.len() as u64
        } else {
            0
        };
        let (dx, dy) = SHEEN[step as usize];
        let at = |r: i32| {
            (
                cx + dx * r - i32::from(dx < 0),
                cy + dy * r - i32::from(dy < 0),
            )
        };
        // A diagonal's steps are √2 long: two of them to a straight's
        // three, about the same length.
        let (sheen, notch): (&[i32], i32) = if dx != 0 && dy != 0 {
            (&[7, 8], 5)
        } else {
            (&[9, 10, 11], 7)
        };
        if on {
            for &r in sheen {
                let (x, y) = at(r);
                canvas.pixel(x, y, Color::TEXT);
            }
        }
        // A notch in the label's edge, turning with it.
        let (x, y) = at(notch);
        canvas.pixel(x, y, Color::SHADOW);
    }

    // The tonearm, from its pivot: on the record's outer grooves while
    // it plays, down at its rest otherwise.
    let (px, py) = ARM;
    let end = if on { (cx + 10, cy + 2) } else { (px, py + 15) };
    let steps = (end.0 - px).abs().max(end.1 - py);
    for i in 0..=steps {
        let x = px + (end.0 - px) * i / steps;
        let y = py + (end.1 - py) * i / steps;
        canvas.pixel(x, y, Color::TEXT_DIM);
    }
    canvas.fill_rect(end.0 - 1, end.1, 2, 2, Color::TEXT);
    street::disc(canvas, px - 1, py - 1, 3, Color::TEXT_FAINT);

    // The controls: play or pause (whichever a tap does) in the label's
    // dark ink, as printed on it; previous and next in the text's colour.
    let play_pause = if on { &font::PAUSE } else { &font::PLAY };
    canvas.sprite(cx - 5, cy - 5, 5, play_pause, 2, Color::SHADOW);
    for (x, icon) in [(6, &font::PREVIOUS), (61, &font::NEXT)] {
        canvas.sprite(x, MUSIC_CONTROLS_Y + 6, 5, icon, 3, Color::TEXT);
    }
}
