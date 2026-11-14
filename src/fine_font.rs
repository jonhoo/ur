//! Living Tokyo's lettering at the panel's resolution: a monoline face,
//! drawn as strokes rather than stored as pixels, for names, titles and
//! big figures that don't change every second (see
//! `Canvas::fine_type`).
//!
//! The canvas's fonts are pixel art, at 3× on the panel; this face is
//! drawn at the panel's own 240×240, at any size and weight. Each glyph is
//! a few strokes on a grid (straight lines, and quarter-ellipse arcs
//! flattened as they're drawn), traced with a round pen: a neon tube's
//! lettering, or a station sign's rounded sans, which is the city at
//! night's own type. Stored as its strokes' points, the whole of
//! printable ASCII (lowercase too, which the pixel fonts have no room
//! for) and Latin-1's letters take about 1.3 KB, where a bitmap face at
//! one size of names would take three times that, and another size as
//! much again.
//!
//! The grid: capitals from y = 0 (their top) to 10 (the baseline), the
//! lowercase's x-height at 3, descenders to 13, in units of a tenth of
//! the cap height ([`Style::cap`] pixels), with halves. Latin-1's
//! accented letters are their base letter and an accent over it (or a
//! cedilla under it), as in the pixel fonts (see `font::decompose`), but
//! keeping their case.

// ==============================================================================
// The glyphs
// ==============================================================================
//
// Each glyph is written as its strokes, for the eye to check against the
// grid above: `x,y` a point (`+` after a coordinate adds half a unit, so
// `3+,0` is 3½ across at the top); `h` or `v` before a point reaches it
// along a quarter of an ellipse that leaves the point before it
// horizontally or vertically (so `4,0 h8,4` is an O's top right corner,
// centred on 4,4); `/` lifts the pen; and `=N` sets the glyph's width (in
// half units) when it isn't its rightmost point's, so that the digits all
// take the same, and a time doesn't shift as it changes. A stroke of one
// point is a dot. They're turned into [`POINTS`] as the firmware's built.

/// Printable ASCII, from the space to `~`.
const ASCII: [&str; 95] = [
    "=4", // space
    "0,0 0,7 / 0,10",
    "0,0 0,2 / 2,0 2,2",
    "2,0 1,10 / 5,0 4,10 / 0,3+ 6,3+ / 0,6+ 6,6+",
    "6,1+ v3,0 h0,2+ v3,5 h6+,7+ v3,10 h0,8+ / 3,-1 3,11",
    "0,10 7,0 / 1+,0 h3,1+ v1+,3 h0,1+ v1+,0 / 5+,7 h7,8+ v5+,10 h4,8+ v5+,7",
    "7,10 1+,3+ 1+,2 v3,0+ h4+,2 v2,5 h0,7+ v3,10 h6,7",
    "0,0 0,2",
    "2,-1 h0,3 0,7 v2,11",
    "0,-1 h2,3 2,7 v0,11",
    "2,0 2,4 / 0,1 4,3 / 0,3 4,1",
    "0,6+ 6,6+ / 3,3+ 3,9+",
    "1,10 0,12",
    "0,6+ 4,6+",
    "0,10",
    "0,11 5,-1",
    // The digits, all as wide, so that a time keeps its place as it
    // changes.
    "3,0 h6,3 6,7 v3,10 h0,7 0,3 v3,0",
    "=12 1,2 3+,0 3+,10",
    "0,2+ v3,0 h6,2+ 6,3 0,10 6,10",
    "0,1+ v3,0 h6,2+ v3,5 1+,5 / 3,5 h6,7+ v3,10 h0,8+",
    "=12 5,10 5,0 0,7 6+,7",
    "6,0 0,0 0,4 3,4 h6,7 v3,10 h0,8+",
    "0,7 v3,4 h6,7 v3,10 h0,7 0,4 v3,0 h5+,1+",
    "0,0 6,0 2,10",
    "3,5 h0+,2+ v3,0 h5+,2+ v3,5 h6,7+ v3,10 h0,7+ v3,5",
    "6,3 v3,6 h0,3 v3,0 h6,3 6,6 v3,10 h0+,9",
    "0,4 / 0,10",
    "1,4 / 1,10 0,12",
    "6,3 0,6+ 6,10",
    "0,5 6,5 / 0,8 6,8",
    "0,3 6,6+ 0,10",
    "0,2+ v3,0 h6,2+ v3,5 3,7 / 3,10",
    "6,3+ 6,7 v7,8 h8,7 8,4 v4,0+ h0,4 0,6 v4,10 h7,9 / 6,5+ v4,3+ h2,5+ v4,7+ h6,5+",
    // The capitals. Round ones (C, D, G, O, Q) are a unit wider than
    // square ones, as in any sans; I is a bare stem, told from l by l's
    // foot.
    "0,10 3+,0 7,10 / 1,7 6,7",
    "0,5 4,5 h7,7+ v4,10 0,10 0,0 3+,0 h6+,2+ v3+,5",
    "7,2 v4,0 h0,4 0,6 v4,10 h7,8",
    "0,0 0,10 3,10 h7,6 7,4 v3,0 0,0",
    "6,0 0,0 0,10 6,10 / 0,5 5,5",
    "6,0 0,0 0,10 / 0,5 5,5",
    "7,2 v4,0 h0,4 0,6 v4,10 h7,7 7,5+ 4,5+",
    "0,0 0,10 / 7,0 7,10 / 0,5 7,5",
    "0,0 0,10",
    "5,0 5,7 v2+,10 h0,7+",
    "0,0 0,10 / 6,0 0,6+ / 2+,4+ 6+,10",
    "0,0 0,10 6,10",
    "0,10 0,0 4,7 8,0 8,10",
    "0,10 0,0 7,10 7,0",
    "4,0 h8,4 8,6 v4,10 h0,6 0,4 v4,0",
    "0,10 0,0 4,0 h7,2+ 7,3 v4,5+ 0,5+",
    "4,0 h8,4 8,6 v4,10 h0,6 0,4 v4,0 / 5,7+ 8,10+",
    "0,10 0,0 4,0 h7,2+ 7,3 v4,5+ 0,5+ / 4,5+ 7,10",
    "6,1+ v3,0 h0,2+ v3,5 h6+,7+ v3,10 h0,8+",
    "0,0 7,0 / 3+,0 3+,10",
    "0,0 0,6+ v3+,10 h7,6+ 7,0",
    "0,0 3+,10 7,0",
    "0,0 2+,10 5,2 7+,10 10,0",
    "0,0 7,10 / 7,0 0,10",
    "0,0 3+,5 7,0 / 3+,5 3+,10",
    "0,0 7,0 0,10 7,10",
    "2,-1 0,-1 0,11 2,11",
    "0,-1 5,11",
    "0,-1 2,-1 2,11 0,11",
    "0,3 2+,0 5,3",
    "0,12 6,12",
    "0,0 1,1",
    // The lowercase: a single-storey a and g, as signs have them, and
    // bowls as round as the capitals'.
    "6,3 6,10 / 6,6+ v3,3 h0,6+ v3,10 h6,6+",
    "0,0 0,10 / 0,6+ v3,3 h6,6+ v3,10 h0,6+",
    "5+,4+ v3,3 h0,6+ v3,10 h5+,8+",
    "6,0 6,10 / 6,6+ v3,3 h0,6+ v3,10 h6,6+",
    "0,6+ 6,6+ v3,3 h0,6+ v3,10 h5+,9",
    "5,1 v3+,0 h2,1+ 2,10 / 0,3 4+,3",
    "6,3 6,10+ v3,13 h0+,11+ / 6,6+ v3,3 h0,6+ v3,10 h6,6+",
    "0,0 0,10 / 0,6 v3,3 h6,6 6,10",
    "0,3 0,10 / 0,0+",
    "1+,3 1+,11+ v0,13 / 1+,0+",
    "0,0 0,10 / 5,3 0,7 / 2,5+ 5+,10",
    "0,0 0,8+ v1+,10",
    "0,3 0,10 / 0,5 v2,3 h4,5 4,10 / 4,5 v6,3 h8,5 8,10",
    "0,3 0,10 / 0,6 v3,3 h6,6 6,10",
    "3,3 h6,6+ v3,10 h0,6+ v3,3",
    "0,3 0,13 / 0,6+ v3,3 h6,6+ v3,10 h0,6+",
    "6,3 6,13 / 6,6+ v3,3 h0,6+ v3,10 h6,6+",
    "0,3 0,10 / 0,6 v3,3 4,3",
    "5+,4 v3,3 h0+,4+ v3,6+ h5+,8+ v3,10 h0,9",
    "1+,1 1+,8+ v3,10 4,10 / 0,3 4,3",
    "0,3 0,7 v3,10 h6,7 / 6,3 6,10",
    "0,3 3,10 6,3",
    "0,3 2,10 4+,4 7,10 9,3",
    "0,3 6,10 / 6,3 0,10",
    "0,3 3,10 / 6,3 1+,13",
    "0,3 5+,3 0,10 5+,10",
    "3,-1 h1+,0+ 1+,3+ v0,5 h1+,6+ 1+,9+ v3,11",
    "0,-1 0,12",
    "0,-1 h1+,0+ 1+,3+ v3,5 h1+,6+ 1+,9+ v0,11",
    "0,7 v1+,6 h3,7 v4+,8 h6,7",
];

/// Latin-1's letters that aren't a letter and an accent, its symbols
/// that notifications use, and a few of the typographic ones phones send
/// (see `font::decompose`), as [`EXTRA`] draws them, in its order.
/// Anything else is drawn as `?`.
const EXTRAS: &str = "ÆæØøßÐðÞþı°×÷…«»·¡¿£€±";
const EXTRA: [&str; 22] = [
    "0,10 4+,0 10,0 / 4+,0 4+,10 10,10 / 2,6 9,6",
    "5,3 5,10 / 5,6+ v2+,3 h0,6+ v2+,10 h5,6+ / 5,6+ 10,6+ v7+,3 h5,6+ v7+,10 h10,9",
    "4,0 h8,4 8,6 v4,10 h0,6 0,4 v4,0 / 0,11 8,-1",
    "3,3 h6,6+ v3,10 h0,6+ v3,3 / 0,10+ 6,2+",
    "0,10 0,2+ v2+,0 h5,2+ v2+,5 h6,7+ v3+,10 2,10",
    "1,0 1,10 4,10 h8,6 8,4 v4,0 1,0 / 0,5 3,5",
    "3,3 h6,6+ v3,10 h0,6+ v3,3 / 6,6+ 6,5 v2,0 / 2,2 5,0+",
    "0,0 0,10 / 0,2+ 4,2+ h7,5 v4,7+ 0,7+",
    "0,0 0,13 / 0,6+ v3,3 h6,6+ v3,10 h0,6+",
    // The dotless i, under an accent.
    "0,3 0,10",
    "1+,0 h3,1+ v1+,3 h0,1+ v1+,0",
    "0,4+ 4,8+ / 4,4+ 0,8+",
    "0,6+ 5,6+ / 2+,3+ / 2+,9+",
    "0,10 / 3,10 / 6,10",
    "2+,4 0,6+ 2+,9 / 5+,4 3,6+ 5+,9",
    "0,4 2+,6+ 0,9 / 3,4 5+,6+ 3,9",
    "0,6+",
    "0,3 / 0,6 0,13",
    "6,10+ v3,13 h0,10+ v3,8 3,6 / 3,3",
    "6,1 v4,0 h2,2+ 2,10 / 0,10 6,10 / 0,5+ 4+,5+",
    "8,2 v5,0 h1,4 1,6 v5,10 h8,8 / 0,4 5,4 / 0,6 5,6",
    "0,5 6,5 / 3,2 3,8 / 0,10 6,10",
];
const _: () = {
    // A character a byte that doesn't continue one.
    let (bytes, mut chars, mut i) = (EXTRAS.as_bytes(), 0, 0);
    while i < bytes.len() {
        chars += (bytes[i] & 0xC0 != 0x80) as usize;
        i += 1;
    }
    assert!(chars == EXTRA.len(), "a glyph for each of `EXTRAS`");
};

/// The accents, by `font::decompose`'s symbols for them, in a box four
/// units wide (centred on 2) and a unit and a half tall from y = 0,
/// which [`Glyph`] puts over its letter; and the cedilla, under it.
const MARKS: &[u8; 7] = b"'`^~:o,";
const MARK: [&str; 7] = [
    "1,1+ 3,0",
    "1,0 3,1+",
    "0,1+ 2,0 4,1+",
    "0,1+ 1+,0+ 2+,1 4,0",
    "0+,1 / 3+,1",
    "2,-1 h3,0 v2,1 h1,0 v2,-1",
    "2,10 2,11 3,11 h4,12 v3,13 1,13",
];

const GLYPHS: usize = ASCII.len() + EXTRA.len() + MARK.len();

/// Every glyph's source, in one list: [`ASCII`], [`EXTRA`], [`MARK`].
const fn source(glyph: usize) -> &'static str {
    if glyph < ASCII.len() {
        ASCII[glyph]
    } else if glyph < ASCII.len() + EXTRA.len() {
        EXTRA[glyph - ASCII.len()]
    } else {
        MARK[glyph - ASCII.len() - EXTRA.len()]
    }
}

// ==============================================================================
// Stored: a `u16` a point
// ==============================================================================

/// How a point is reached from the one before it: the pen put down
/// there, a line, or a quarter arc leaving horizontally or vertically.
const MOVE: u16 = 0;
const LINE: u16 = 1;
const ARC_H: u16 = 2;
const ARC_V: u16 = 3;

/// A point, packed: x in half units (bits 0–5), y in half units from
/// two units above the capitals (bits 6–11), and how it's reached (bits
/// 12–13).
const fn pack(kind: u16, x: i32, y: i32) -> u16 {
    assert!(
        x >= 0 && x < 64 && y >= -4 && y < 60,
        "a point off the grid"
    );
    kind << 12 | ((y + 4) as u16) << 6 | x as u16
}

/// Parses `src` (see the glyphs' notation above): its points into
/// `out` from `at`, if given; returns how many there are, and its width
/// in half units.
const fn parse(src: &str, mut out: Option<(&mut [u16], usize)>) -> (usize, i32) {
    let b = src.as_bytes();
    let (mut i, mut count, mut width, mut fixed) = (0, 0, 0, -1);
    let mut kind = MOVE;
    while i < b.len() {
        match b[i] {
            b' ' => i += 1,
            b'/' => {
                kind = MOVE;
                i += 1;
            }
            b'=' => {
                i += 1;
                let (n, next) = number(b, i);
                fixed = n / 2;
                i = next;
            }
            _ => {
                let mut how = if count == 0 { MOVE } else { kind };
                if b[i] == b'h' {
                    how = ARC_H;
                    i += 1;
                } else if b[i] == b'v' {
                    how = ARC_V;
                    i += 1;
                }
                let (x, next) = number(b, i);
                assert!(b[next] == b',', "a point is x,y");
                let (y, next) = number(b, next + 1);
                i = next;
                if let Some((points, at)) = &mut out {
                    points[*at + count] = pack(how, x, y);
                }
                count += 1;
                if x > width {
                    width = x;
                }
                kind = LINE;
            }
        }
    }
    (count, if fixed >= 0 { fixed } else { width })
}

/// A number at `b[i..]` in half units (`3+` is 7), and where it ends.
const fn number(b: &[u8], mut i: usize) -> (i32, usize) {
    let negative = b[i] == b'-';
    if negative {
        i += 1;
    }
    let mut n = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        n = n * 10 + (b[i] - b'0') as i32;
        i += 1;
    }
    n *= 2;
    if i < b.len() && b[i] == b'+' {
        n += 1;
        i += 1;
    }
    (if negative { -n } else { n }, i)
}

const POINT_COUNT: usize = {
    let mut total = 0;
    let mut glyph = 0;
    while glyph < GLYPHS {
        total += parse(source(glyph), None).0;
        glyph += 1;
    }
    total
};

/// Every glyph's points, one after another.
static POINTS: [u16; POINT_COUNT] = {
    let mut points = [0; POINT_COUNT];
    let (mut glyph, mut at) = (0, 0);
    while glyph < GLYPHS {
        at += parse(source(glyph), Some((&mut points, at))).0;
        glyph += 1;
    }
    points
};

/// Where each glyph's points start in [`POINTS`] (bits 0–10; the next
/// glyph's start is where they end), and its width in half units (bits
/// 11–15).
static INDEX: [u16; GLYPHS + 1] = {
    let mut index = [0; GLYPHS + 1];
    let (mut glyph, mut at) = (0, 0);
    while glyph < GLYPHS {
        let (count, width) = parse(source(glyph), None);
        assert!(
            at < 1 << 11 && width < 1 << 5,
            "a glyph's start and width fit"
        );
        index[glyph] = at as u16 | (width as u16) << 11;
        at += count;
        glyph += 1;
    }
    index[GLYPHS] = at as u16;
    index
};

// ==============================================================================
// Which glyph a character is
// ==============================================================================

/// What a character is drawn as: a glyph, and an accent over it (or a
/// cedilla under it), if it has one.
#[derive(Clone, Copy)]
struct Glyph {
    base: usize,
    mark: Option<usize>,
    /// Whether the base is a capital, whose accent sits higher.
    capital: bool,
}

/// The glyph for `c`, in capitals if `caps`: as the pixel fonts have it
/// (see `font::decompose`), but keeping lowercase, and `?` for what the
/// face lacks.
fn glyph(c: char, caps: bool) -> Glyph {
    // Most text is ASCII, and a band of a frame looks up each of its
    // characters a few times.
    if let ' '..='~' = c {
        let c = if caps { c.to_ascii_uppercase() } else { c };
        return Glyph {
            base: c as usize - ' ' as usize,
            mark: None,
            capital: false,
        };
    }
    // As the pixel fonts' capitals, then lowercase again if it was:
    // Latin-1's lowercase is its capitals' 0x20 on (ÿ's base is the y).
    let (mut base, mut mark) = super::decompose(c);
    if !caps && matches!(c, 'à'..='ÿ') && c != '÷' {
        base = match base {
            'A'..='Z' => base.to_ascii_lowercase(),
            _ => char::from(base as u8 + 0x20),
        };
        // An accented i's has no dot.
        if base == 'i' {
            base = 'ı';
        }
    }
    // The cedilla, which the pixel fonts draw as part of the letter.
    if let 'Ç' | 'ç' = base {
        (base, mark) = (char::from(base as u8 - 0x84), b',');
    }
    let index = match base {
        ' '..='~' => base as usize - ' ' as usize,
        _ => match EXTRAS.chars().position(|e| e == base) {
            Some(i) => ASCII.len() + i,
            None => usize::from(b'?' - b' '),
        },
    };
    Glyph {
        base: index,
        mark: MARKS
            .iter()
            .position(|&m| m == mark)
            .map(|m| ASCII.len() + EXTRA.len() + m),
        // Only ASCII letters take marks.
        capital: base.is_ascii_uppercase(),
    }
}

/// Glyph `i`'s width, in half units.
fn width(i: usize) -> i32 {
    i32::from(INDEX[i] >> 11)
}

/// Glyph `i`'s points.
fn points(i: usize) -> &'static [u16] {
    let start = usize::from(INDEX[i] & 0x7FF);
    let end = usize::from(INDEX[i + 1] & 0x7FF);
    &POINTS[start..end]
}

// ==============================================================================
// Sizes and weights
// ==============================================================================

/// A size and weight of the face: what a layout measures in (as
/// `font::Face::Fine`) and `Canvas::fine_type` draws in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Style {
    /// The capitals' height, in the panel's pixels, from the middle of
    /// their top stroke to the middle of the baseline's: the ink is a
    /// stroke taller (see [`Style::height`]).
    pub cap: u8,
    /// The pen's width, in sixteenths of a pixel.
    pub stroke: u8,
    /// Lowercase drawn as capitals, as a sign's lettering is.
    pub caps: bool,
}

impl Style {
    /// The ink's height of a line's capitals, in pixels: what to centre.
    pub const fn height(self) -> i32 {
        self.cap as i32 + (self.stroke as i32 + 15) / 16
    }

    /// How far a line's ink reaches above `y`, the top of its capitals
    /// (an accent's top over a capital), and below it (a descender's
    /// end), in pixels.
    pub const fn reach(self) -> (i32, i32) {
        let pen = (self.stroke as i32 + 15) / 16;
        (self.cap as i32 / 2 + pen, self.cap as i32 * 13 / 10 + pen)
    }

    /// The clear space between two letters' ink, in pixels: about a
    /// seventh of the cap height, as a sign spaces its letters for
    /// reading from afar.
    pub const fn gap(self) -> i32 {
        (self.cap as i32 * 12 / 5 + 8) / 16
    }

    /// How far `c` moves the pen on, in pixels: its width, the pen's,
    /// and the gap.
    pub fn advance(self, c: char) -> i32 {
        let base = glyph(c, self.caps).base;
        (width(base) * self.cap as i32 * 4 / 5 + self.stroke as i32 + self.cap as i32 * 12 / 5 + 8)
            / 16
    }
}

// ==============================================================================
// Tracing
// ==============================================================================

/// The cosine of every 15° from 0° to 90°, in 256ths: a quarter arc is traced as six lines, which a round
/// pen can't be told from a curve at these sizes (a bowl 30 pixels across
/// strays from its ellipse by a tenth of a pixel).
const COS: [i32; 7] = [256, 247, 222, 181, 128, 66, 0];

/// Calls `segment` with each line of `c`'s strokes in `style`, in
/// sixteenths of the panel's pixels, the skeleton's top left (its
/// capitals' top, at the glyph's left) at `origin`: arcs as the lines
/// that trace them, and a dot as a line from a point to itself.
pub fn trace(
    c: char,
    style: Style,
    origin: (i32, i32),
    mut segment: impl FnMut((i32, i32), (i32, i32)),
) {
    let glyph = glyph(c, style.caps);
    let k = style.cap as i32 * 4;
    let at = |(ox, oy): (i32, i32), p: u16| {
        let x = i32::from(p & 63);
        let y = i32::from(p >> 6 & 63) - 4;
        (ox + x * k / 5, oy + y * k / 5)
    };
    // The accent over the letter's middle: half a unit above the x
    // height over lowercase, two above the capitals over a capital; the
    // cedilla where it's drawn, under the middle.
    let shift = (width(glyph.base) - 8) * k / 10;
    let (ox, oy) = origin;
    let mark = glyph.mark.map(|mark| {
        let rise = match MARKS[mark - ASCII.len() - EXTRA.len()] {
            b',' => 0,
            _ if glyph.capital => -8,
            _ => -1,
        };
        (mark, (ox + shift, oy + rise * k / 5))
    });
    for (i, origin) in [(glyph.base, origin)].into_iter().chain(mark) {
        let mut pen = (0, 0);
        let points = points(i);
        for (n, &p) in points.iter().enumerate() {
            let to = at(origin, p);
            match p >> 12 {
                MOVE => {
                    // A point the pen's lifted from at once is a dot.
                    if points.get(n + 1).is_none_or(|&next| next >> 12 == MOVE) {
                        segment(to, to);
                    }
                }
                LINE => segment(pen, to),
                kind => {
                    // A quarter of the ellipse centred level with one end
                    // and plumb with the other: leaving `pen`
                    // horizontally, its centre is under (or over) `pen`.
                    // In three lines rather than six if it's small (under
                    // 12 pixels round): it strays a fifth of a pixel at
                    // most, and the strokes cost half as much to draw.
                    let ((x0, y0), (x1, y1)) = (pen, to);
                    let small = (x1 - x0).abs().max((y1 - y0).abs()) < 12 * 16;
                    let mut from = pen;
                    for step in (1..COS.len()).filter(|step| !small || step % 2 == 0) {
                        let (cos, sin) = (COS[step], COS[COS.len() - 1 - step]);
                        let next = if kind == ARC_H {
                            (x0 + (x1 - x0) * sin / 256, y1 + (y0 - y1) * cos / 256)
                        } else {
                            (x1 + (x0 - x1) * cos / 256, y0 + (y1 - y0) * sin / 256)
                        };
                        segment(from, next);
                        from = next;
                    }
                }
            }
            pen = to;
        }
    }
}
