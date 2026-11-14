//! An 80×80 canvas of colour roles (see [`crate::palette`]), shown on the
//! 240×240 panel at 3×.
//!
//! The low resolution *is* the pixel-art look: every UI element is drawn on
//! this grid, so nothing can accidentally end up finer-grained than a "pixel".
//!
//! The UI redraws the whole canvas from scratch every frame (immediate-mode),
//! which keeps screen code simple. Pushing all of that to the panel would cost
//! ~115 ms per frame at 8 MHz SPI, so we keep a copy of what the panel
//! currently shows (`front`) and [`Canvas::flush`] only sends the rows that
//! differ from it. A typical clock tick touches a handful of rows.
//!
//! Living Tokyo isn't a retro theme, and draws some rows at the panel's
//! own resolution instead (see [`Canvas::fine_rect`]): those rows are
//! drawn again, a canvas row at a time, as they're sent ([`Canvas::refine`]),
//! as the panel's resolution has no room in RAM for a frame of its own.

#[cfg(feature = "theme-living_tokyo")]
use core::num::NonZeroU8;
use core::ops::Range;

use embassy_time::{Duration, Instant, Ticker};

pub use self::marquee::{MARQUEE_FRAME, marquee_cycle, marquee_offset};

use crate::display::Display;
use crate::font;
use crate::palette::Palette;
use crate::qr;

pub use crate::palette::Color;

pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 80;
/// Panel pixels per canvas pixel, in each direction.
pub const SCALE: usize = 3;

/// Two pixels per byte (4-bit colour roles), low nibble first.
const BYTES: usize = WIDTH * HEIGHT / 2;
/// One full-width canvas row of panel pixel bytes: the most [`stream`]
/// needs to send any rectangle whole rows at a time, and enough for a
/// transition's 8×8 blocks (1152 bytes) in one transfer. Two rows (as it
/// once was) halved the transfers of a full-width flush, but cost 1.4 KB of
/// RAM to save ~40 transfers' setup on the rare full-frame push.
const BATCH: usize = WIDTH * SCALE * SCALE * 2;

/// The QR code's modules, in panel pixels; its side, with the standard
/// quiet zone of four modules; and where it starts, centred on the panel
/// (see [`Canvas::qr`]).
const QR_MODULE: usize = 7;
const QR_SIDE: usize = (qr::SIZE + 2 * 4) * QR_MODULE;
const QR_ORIGIN: usize = (WIDTH * SCALE - QR_SIDE) / 2;

/// `repr(C)`, so `scripts/screenshot.py` can find `front` and `palette` (see
/// [`crate::debug`]).
#[repr(C)]
pub struct Canvas {
    /// The frame being drawn.
    back: [u8; BYTES],
    /// What the panel is currently showing.
    front: [u8; BYTES],
    /// False until the panel has received one full frame (or after a palette
    /// change); until then, `front` says nothing about the panel's contents.
    front_valid: bool,
    /// Resolves colour roles to pixel bytes when pushing.
    palette: &'static Palette,
    /// The bytes to show over the canvas as a QR code this frame (see
    /// [`Canvas::qr`]), if `qr_wanted`; and whether the panel shows it.
    /// Before `batch`, at a fixed place for `scripts/screenshot.py`.
    qr: [u8; 16],
    qr_wanted: bool,
    qr_shown: bool,
    /// Pixel bytes waiting to be sent; see [`stream`].
    batch: [u8; BATCH],
    /// While set, drawing only touches these columns (for [`Canvas::marquee`]).
    clip: Option<Range<i32>>,
    /// The rows drawn at the panel's resolution. Last, so that
    /// `scripts/screenshot.py` and the dev kit compare find the rest
    /// where they always were.
    #[cfg(feature = "theme-living_tokyo")]
    fine: Fine,
}

impl Canvas {
    /// A canvas showing in `palette` until the UI sets one (see
    /// [`Canvas::set_palette`]), which it does before the first frame.
    pub const fn new(palette: &'static Palette) -> Self {
        Self {
            back: [0; BYTES],
            front: [0; BYTES],
            front_valid: false,
            palette,
            qr: [0; 16],
            qr_wanted: false,
            qr_shown: false,
            batch: [0; BATCH],
            clip: None,
            #[cfg(feature = "theme-living_tokyo")]
            fine: Fine::NONE,
        }
    }

    /// Switches palette. The canvas contents (roles) stay as they are, but
    /// every pixel's colour changes, so the next flush repaints the whole
    /// panel.
    pub fn set_palette(&mut self, palette: &'static Palette) {
        if !core::ptr::eq(palette, self.palette) {
            self.palette = palette;
            self.front_valid = false;
        }
    }

    // ==========================================================================
    // Drawing
    // ==========================================================================

    /// Clears the frame, starting a new one: every screen starts here.
    pub fn clear(&mut self, color: Color) {
        #[cfg(feature = "theme-living_tokyo")]
        if self.fine.band.is_some() {
            self.back[..BAND_ROWS * BAND].fill(color.0 | (color.0 << 4));
            return;
        }
        self.back.fill(color.0 | (color.0 << 4));
        self.qr_wanted = false;
        #[cfg(feature = "theme-living_tokyo")]
        self.start_notes(color);
    }

    /// Shows `matrix` over this frame, at the panel's own resolution
    /// rather than the canvas's: a QR code drawn on the canvas would have
    /// modules of 3 or 6 panel pixels, where the panel fits 7, and every
    /// pixel of a module helps a phone's camera. Dark on white, whatever
    /// the theme, as scanners expect, with the standard quiet zone of four
    /// modules: 33 × 7 = 231 pixels, nearly the whole panel.
    ///
    /// It's sent after the canvas, once, while the frames that want it
    /// leave the canvas beneath it unchanged; whatever the canvas does send
    /// meanwhile (a banner, say) it's drawn again over. The first frame
    /// without it repaints the panel.
    ///
    /// What it encodes is `bytes` in hex (capitals, which QR's
    /// alphanumeric mode packs tighter than bytes): Home Assistant's key.
    /// The code itself is only worked out as it's sent (see `overlay`),
    /// rather than kept here, in static RAM, for the rest of the time.
    pub fn qr(&mut self, bytes: &[u8; 16]) {
        self.qr = *bytes;
        self.qr_wanted = true;
    }

    /// The panel pixels (in each direction) the QR code covers, while the
    /// panel shows it: for the simulator's dev kit compare, as the dev kit
    /// exports only its canvas, from beneath the code.
    #[cfg(not(target_os = "none"))]
    pub fn qr_area(&self) -> Option<Range<usize>> {
        self.qr_shown.then_some(QR_ORIGIN..QR_ORIGIN + QR_SIDE)
    }

    /// Sets one pixel. Out-of-bounds coordinates are silently clipped, so
    /// callers can draw shapes that hang off the edge.
    pub fn pixel(&mut self, x: i32, y: i32, color: Color) {
        #[cfg(feature = "theme-living_tokyo")]
        self.note(y, y + 1, &[note::PIXEL, x, y, color.0.into()], &[]);
        self.plot(x, y, color);
    }

    /// [`Canvas::pixel`], for the shapes made of them, which note their
    /// drawing whole (see `note`).
    fn plot(&mut self, x: i32, y: i32, color: Color) {
        if !(0..WIDTH as i32).contains(&x) || !(0..HEIGHT as i32).contains(&y) {
            return;
        }
        if self.clip.as_ref().is_some_and(|clip| !clip.contains(&x)) {
            return;
        }
        // Redrawing rows finely (see `refine`): a canvas pixel is
        // SCALE × SCALE of the band's.
        #[cfg(feature = "theme-living_tokyo")]
        if self.fine.band.is_some() {
            let scale = SCALE as i32;
            self.fine_rect(x * scale, y * scale, scale, scale, color);
            return;
        }
        set_nibble(&mut self.back, y as usize * WIDTH + x as usize, color.0);
    }

    /// Fills a rectangle.
    ///
    /// Unlike a single [`Canvas::pixel`], a rectangle is expected to lie on
    /// the canvas, and to have a size: every screen lays its bars and
    /// boxes out from constants, so one that hangs off an edge (or comes
    /// out negative, and so draws nothing) is arithmetic gone wrong
    /// upstream, which the clipping would otherwise hide.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        debug_assert!(w >= 0 && h >= 0, "a rectangle of a negative size");
        debug_assert!(
            w == 0 || h == 0 || self.on_canvas(x..x + w, y..y + h),
            "a rectangle off the canvas"
        );
        self.rect(x, y, w, h, color);
    }

    /// A rectangle, wherever it is: what's of it on the canvas. For the
    /// sprites' pixels, which may hang off an edge (see [`Canvas::pixel`]).
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        #[cfg(feature = "theme-living_tokyo")]
        self.note(y, y + h, &[note::RECT, x, y, w, h, color.0.into()], &[]);
        self.paint(x, y, w, h, color);
    }

    /// [`Canvas::rect`], for the shapes made of them, which note their
    /// drawing whole (see `note`).
    fn paint(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        // Redrawing rows finely (see `refine`), only those rows of it, a
        // run at a time, so that drawing the rest of the screen again
        // costs little.
        #[cfg(feature = "theme-living_tokyo")]
        if self.fine.band.is_some() {
            let scale = SCALE as i32;
            self.fine_rect(x * scale, y * scale, w * scale, h * scale, color);
            return;
        }
        for yy in y..y + h {
            for xx in x..x + w {
                self.plot(xx, yy, color);
            }
        }
    }

    /// Whether a box drawn over `columns` and `rows` lies wholly on the
    /// canvas, for the drawing functions' checks. While a clip is set
    /// ([`Canvas::marquee`]), what's drawn is meant to be cut, so anything
    /// goes.
    fn on_canvas(&self, columns: Range<i32>, rows: Range<i32>) -> bool {
        self.clip.is_some()
            || (columns.start >= 0
                && columns.end <= WIDTH as i32
                && rows.start >= 0
                && rows.end <= HEIGHT as i32)
    }

    /// Draws a 1-bit sprite: each row is a bitmask whose most significant of
    /// the `width` low bits is the leftmost pixel. Zero bits are transparent.
    pub fn sprite(&mut self, x: i32, y: i32, width: u32, rows: &[u8], scale: i32, color: Color) {
        #[cfg(feature = "theme-living_tokyo")]
        self.note(
            y,
            y + rows.len() as i32 * scale,
            &[note::SPRITE, x, y, width as i32, scale, color.0.into()],
            rows,
        );
        self.sprite_bits(x, y, width, rows, scale, color);
    }

    /// [`Canvas::sprite`], for the text drawn with it, which notes its
    /// drawing whole (see `note`).
    fn sprite_bits(&mut self, x: i32, y: i32, width: u32, rows: &[u8], scale: i32, color: Color) {
        for (dy, &row) in rows.iter().enumerate() {
            for dx in 0..width {
                if row & (1 << (width - 1 - dx)) != 0 {
                    let px = x + dx as i32 * scale;
                    let py = y + dy as i32 * scale;
                    self.paint(px, py, scale, scale, color);
                }
            }
        }
    }

    /// Like [`Canvas::sprite`], for the 16-wide turn arrows (see
    /// [`font::TURN_W`]), optionally mirrored left to right: the left turns
    /// are the right ones' mirror images.
    pub fn sprite16(
        &mut self,
        x: i32,
        y: i32,
        rows: &[u16],
        mirror: bool,
        scale: i32,
        color: Color,
    ) {
        for (dy, &row) in rows.iter().enumerate() {
            let row = if mirror { row.reverse_bits() } else { row };
            for dx in 0..16 {
                if row & (1 << (15 - dx)) != 0 {
                    self.rect(x + dx * scale, y + dy as i32 * scale, scale, scale, color);
                }
            }
        }
    }

    /// Draws text in the 3×5 font and returns the x just past its end.
    pub fn text(&mut self, x: i32, y: i32, text: &str, color: Color) -> i32 {
        self.text_scaled(x, y, text, color, 1)
    }

    /// Text in the small font at `scale`× (each font pixel a `scale`-sided
    /// square); returns the x just past its end. `y` is the top of the
    /// capitals; accents extend above it.
    ///
    /// The capitals must fit on the canvas (its last letter's spacing
    /// column, and accents, may hang off it): screens measure their text
    /// to place it, or cut it to fit with [`font::wrap`], so text running
    /// off an edge is a string longer than its layout allowed for. What's
    /// meant to be cut at the edge goes through [`Canvas::text_cut`], and
    /// what scrolls, through [`Canvas::marquee`].
    pub fn text_scaled(&mut self, x: i32, y: i32, text: &str, color: Color, scale: i32) -> i32 {
        let end = self.glyphs(x, y, text, color, scale);
        debug_assert!(
            text.is_empty() || self.on_canvas(x..end - scale, y..y + 5 * scale),
            "text off the canvas"
        );
        end
    }

    /// Like [`Canvas::text`], for text that may be too long for the
    /// canvas: it's cut at the right edge.
    pub fn text_cut(&mut self, x: i32, y: i32, text: &str, color: Color) {
        self.glyphs(x, y, text, color, 1);
    }

    /// Draws `text` as [`Canvas::text_scaled`] has it, wherever that puts
    /// it.
    fn glyphs(&mut self, x: i32, y: i32, text: &str, color: Color, scale: i32) -> i32 {
        let mut cursor = x;
        let top = y - font::ACCENT_ROWS * scale;
        // Rows a fine band doesn't take (see `refine`) only need its end.
        #[cfg(feature = "theme-living_tokyo")]
        if !self.in_band(top, y + 5 * scale) {
            let width = font::small_text_width(text);
            return if text.is_empty() {
                x
            } else {
                x + (width + 1) * scale
            };
        }
        // Whether any letter has an accent, which the rows above the
        // capitals are only drawn for.
        #[cfg(feature = "theme-living_tokyo")]
        let mut accented = false;
        for c in text.chars() {
            let glyph = font::glyph(c);
            self.sprite_bits(cursor, top, glyph.width, &glyph.rows, scale, color);
            cursor += (glyph.width as i32 + 1) * scale;
            #[cfg(feature = "theme-living_tokyo")]
            {
                accented |= glyph.rows[..font::ACCENT_ROWS as usize]
                    .iter()
                    .any(|&row| row != 0);
            }
        }
        // Noted whole, over the rows it drew: so that a ticking figure
        // under a fine row drawn over its accents' rows doesn't redraw
        // that row's band (see `note`).
        #[cfg(feature = "theme-living_tokyo")]
        self.note(
            if accented { top } else { y },
            y + 5 * scale,
            &[note::TEXT, x, y, scale, color.0.into()],
            text.as_bytes(),
        );
        cursor
    }

    /// Text in the medium face (see [`font::medium_glyph`]); returns the x
    /// just past its end. `y` is the top of the capitals; accents extend
    /// above it.
    pub fn text_medium(&mut self, x: i32, y: i32, text: &str, color: Color) -> i32 {
        let mut cursor = x;
        let top = y - font::ACCENT_ROWS;
        // As in `glyphs`.
        #[cfg(feature = "theme-living_tokyo")]
        if !self.in_band(top, y + font::MEDIUM_HEIGHT) {
            let width = font::Face::Medium.width(text);
            return if text.is_empty() { x } else { x + width + 1 };
        }
        #[cfg(feature = "theme-living_tokyo")]
        let mut accented = false;
        for c in text.chars() {
            let glyph = font::medium_glyph(c);
            self.sprite_bits(cursor, top, glyph.width, &glyph.rows, 1, color);
            cursor += glyph.width as i32 + 1;
            #[cfg(feature = "theme-living_tokyo")]
            {
                accented |= glyph.rows[..font::ACCENT_ROWS as usize]
                    .iter()
                    .any(|&row| row != 0);
            }
        }
        // As in `glyphs`.
        #[cfg(feature = "theme-living_tokyo")]
        self.note(
            if accented { top } else { y },
            y + font::MEDIUM_HEIGHT,
            &[note::MEDIUM, x, y, color.0.into()],
            text.as_bytes(),
        );
        // As for the small font (see `text_scaled`).
        debug_assert!(
            text.is_empty() || self.on_canvas(x..cursor - 1, y..y + font::MEDIUM_HEIGHT),
            "text off the canvas"
        );
        cursor
    }

    /// One laid-out [`font::Line`] of `text` (see [`font::wrap`]), in
    /// `face`, its tail (a hyphen or an ellipsis) after it.
    pub fn line(
        &mut self,
        x: i32,
        y: i32,
        text: &str,
        line: font::Line,
        face: font::Face,
        color: Color,
    ) {
        let mut draw = |x: i32, text: &str| match face {
            font::Face::Small => self.text(x, y, text, color),
            font::Face::Medium => self.text_medium(x, y, text, color),
            // In the panel's pixels, as it's measured.
            #[cfg(feature = "theme-living_tokyo")]
            font::Face::Fine(style) => self.fine_type(x, y, text, style, color, None),
        };
        let end = draw(x, line.of(text));
        // Right after the text, as one more letter of it.
        let x = if line.end > line.start { end } else { x };
        draw(x, line.tail.as_str());
    }

    /// Text confined to `columns`. If it doesn't fit, it scrolls:
    /// a pause at the start, a steady crawl to the end, a pause there, and
    /// round again. `elapsed` is how long the text has been showing.
    pub fn marquee(
        &mut self,
        columns: Range<i32>,
        y: i32,
        text: &str,
        color: Color,
        scale: i32,
        elapsed: Duration,
    ) {
        let overflow = font::small_text_width(text) * scale - columns.len() as i32;
        let offset = if overflow > 0 {
            marquee_offset(overflow, elapsed)
        } else {
            0
        };
        // Within what it has to travel, so the text's start and end both
        // come to rest in view.
        debug_assert!((0..=overflow.max(0)).contains(&offset));
        // One at a time: the clip isn't a stack.
        debug_assert!(self.clip.is_none(), "a marquee within a marquee");
        // Noted whole, here, as what it draws within its clip isn't.
        #[cfg(feature = "theme-living_tokyo")]
        self.note(
            y - font::ACCENT_ROWS * scale,
            y + 5 * scale,
            &[
                note::MARQUEE,
                columns.start,
                columns.end,
                y,
                scale,
                color.0.into(),
                offset,
            ],
            text.as_bytes(),
        );
        let left = columns.start;
        self.clip = Some(columns);
        self.text_scaled(left - offset, y, text, color, scale);
        self.clip = None;
    }

    /// Like [`Canvas::text`], but horizontally centred on `center_x`.
    pub fn text_centered(&mut self, center_x: i32, y: i32, text: &str, color: Color) {
        self.text(center_x - font::small_text_width(text) / 2, y, text, color);
    }

    /// Like [`Canvas::text`], but ending at `right_x` (exclusive).
    pub fn text_right(&mut self, right_x: i32, y: i32, text: &str, color: Color) {
        self.text(right_x - font::small_text_width(text), y, text, color);
    }

    // ==========================================================================
    // Pushing to the panel
    // ==========================================================================

    /// Sends every changed region of the canvas to the panel.
    pub async fn flush(&mut self, display: &mut Display) {
        // Whether anything was sent, which may have covered a QR code, and
        // whether everything was.
        let mut sent = false;
        let whole = !self.front_valid;
        #[cfg(feature = "theme-living_tokyo")]
        let stale = self.flushing(whole);
        // Consecutive changed rows are merged into one band whose horizontal
        // extent is the union of the rows' changed spans, so a band costs one
        // address-window setup plus its pixels.
        let mut band: Option<(usize, Range<usize>)> = None; // (first row, columns)
        for row in 0..HEIGHT {
            #[cfg(not(feature = "theme-living_tokyo"))]
            let span = self.changed_span(row);
            // Fine rows are `refine`'s to send; and a row the panel shows
            // finely that's no longer fine, the canvas's, whole.
            #[cfg(feature = "theme-living_tokyo")]
            let span = if self.fine.rows.has(row) {
                let bytes = row * WIDTH / 2..(row + 1) * WIDTH / 2;
                self.front[bytes.clone()].copy_from_slice(&self.back[bytes]);
                None
            } else if stale.has(row) {
                Some(0..WIDTH)
            } else {
                self.changed_span(row)
            };
            match (span, band.take()) {
                (Some(span), Some((start, cols))) => {
                    band = Some((start, cols.start.min(span.start)..cols.end.max(span.end)));
                }
                (Some(span), None) => band = Some((row, span)),
                (None, Some((start, cols))) => {
                    self.push(display, start..row, cols).await;
                    sent = true;
                }
                (None, None) => {}
            }
        }
        if let Some((start, cols)) = band {
            self.push(display, start..HEIGHT, cols).await;
            sent = true;
        }
        self.front_valid = true;
        // Every row that differed was pushed, over at least the columns
        // that did, and `push` records what it sends: the panel now shows
        // the frame. If `changed_span` ever missed a pixel, it would stay
        // stale on the panel until that pixel next changed.
        debug_assert!(self.back == self.front, "a flush left the panel stale");
        self.overlay(display, sent, whole).await;
    }

    /// Draws the QR code over the panel if this frame wants it and the
    /// panel doesn't show it whole (`covered`: the canvas was just sent
    /// over some of it); or, if the frame doesn't want it and the panel
    /// shows it, repaints the canvas over it (unless it just was, `whole`).
    async fn overlay(&mut self, display: &mut Display, covered: bool, whole: bool) {
        match (self.qr_wanted, self.qr_shown) {
            (true, shown) if !shown || covered => {
                // The code goes in the batch's tail, while the pixels go
                // out of its head: worked out on the stack (`qr_rows`),
                // never across an await, so it never takes static RAM
                // in a future. Two rows of pixels fit beside it.
                let (pixels, rows) = self.batch.split_at_mut(BATCH - 4 * qr::SIZE);
                if !qr_rows(&self.qr, rows) {
                    return;
                }
                let span = |at: usize| at as u16..(at + QR_SIDE) as u16;
                display.start_pixels(span(QR_ORIGIN), span(QR_ORIGIN)).await;
                let row_bytes = QR_SIDE * 2;
                let mut used = 0;
                for y in 0..QR_SIDE {
                    if used + row_bytes > pixels.len() {
                        display.pixels(&pixels[..used]).await;
                        used = 0;
                    }
                    // Modules from the quiet zone's corner.
                    let my = (y / QR_MODULE).wrapping_sub(4);
                    let row = rows
                        .as_chunks::<4>()
                        .0
                        .get(my)
                        .map_or(0, |r| u32::from_le_bytes(*r));
                    let line = &mut pixels[used..used + row_bytes];
                    for (x, px) in line.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                        let mx = (x / QR_MODULE).wrapping_sub(4);
                        let dark = mx < qr::SIZE && row >> mx & 1 != 0;
                        *px = if dark { [0x00, 0x00] } else { [0xFF, 0xFF] };
                    }
                    used += row_bytes;
                }
                display.pixels(&pixels[..used]).await;
                display.end_pixels();
                self.qr_shown = true;
            }
            (false, true) => {
                self.qr_shown = false;
                if !whole {
                    self.push(display, 0..HEIGHT, 0..WIDTH).await;
                }
            }
            _ => {}
        }
    }

    /// Replaces the panel contents with the canvas using a retro "block
    /// wipe" that sweeps in `direction`.
    ///
    /// The screen is cut into a 10×10 grid of 8×8-canvas-pixel blocks. A
    /// wavefront of dithered blocks sweeps across (or down, or up); each
    /// block shows the dither for one step, then the new screen. The lanes
    /// the front moves along (rows for a sideways sweep, columns for a
    /// vertical one) are staggered so the front is a chevron pointing the
    /// way the content is moving (middle lanes lead).
    ///
    /// Every block is sent twice (dither, then final), so this costs about two
    /// full frames of SPI time (~230 ms), spread over the steps. That's far
    /// cheaper than a true slide, which would resend the whole screen for
    /// every animation frame.
    pub async fn transition(&mut self, display: &mut Display, direction: Sweep) {
        const BLOCK: usize = 8;
        const BLOCKS: usize = WIDTH / BLOCK;
        /// How many steps behind the leading lanes each lane of blocks
        /// trails: 2 1 1 0 0 0 0 1 1 2 from one edge to the other, i.e. a
        /// shallow chevron. (A deeper one, 4…0…4, read nicely but added
        /// steps.)
        const fn lane_delay(lane: usize) -> usize {
            (2 * lane).abs_diff(BLOCKS - 1) / 4
        }
        const LAST_STEP: usize = (BLOCKS - 1) + lane_delay(0) + 1;
        /// A floor on each step's duration, so steps that send little data
        /// still read as animation frames rather than a blur.
        const STEP: Duration = Duration::from_millis(10);

        let started = Instant::now();
        let mut ticker = Ticker::every(STEP);
        // Every block is sent from the canvas, fine rows' too (as their
        // samples: see `fine_rect`), which `refine` then sharpens.
        #[cfg(feature = "theme-living_tokyo")]
        self.flushing(true);
        // The canvas rows and columns of the `n`th block along `lane`,
        // counted from the side the sweep starts on.
        let block = |lane: usize, n: usize| {
            let (row, column) = match direction {
                Sweep::LeftToRight => (lane, n),
                Sweep::RightToLeft => (lane, BLOCKS - 1 - n),
                Sweep::TopToBottom => (n, lane),
                Sweep::BottomToTop => (BLOCKS - 1 - n, lane),
            };
            (
                row * BLOCK..(row + 1) * BLOCK,
                column * BLOCK..(column + 1) * BLOCK,
            )
        };
        for step in 0..=LAST_STEP {
            for lane in 0..BLOCKS {
                // How far the dither front has got along this lane.
                let Some(reached) = step.checked_sub(lane_delay(lane)) else {
                    continue;
                };
                if reached < BLOCKS {
                    let (rows, columns) = block(lane, reached);
                    self.push_dither(display, rows, columns).await;
                }
                if let Some(revealed) = reached.checked_sub(1)
                    && revealed < BLOCKS
                {
                    let (rows, columns) = block(lane, revealed);
                    self.push(display, rows, columns).await;
                }
            }
            ticker.next().await;
        }
        defmt::debug!("transition took {}ms", started.elapsed().as_millis());
        // Every lane's every block was revealed (pushed) by the last
        // step, so the panel shows the whole frame, whatever it showed.
        debug_assert!(self.back == self.front, "a transition left a block");
        self.front_valid = true;
        // Every block was sent.
        self.overlay(display, true, true).await;
    }

    /// The changed columns in `row`, relative to what the panel shows.
    fn changed_span(&self, row: usize) -> Option<Range<usize>> {
        if !self.front_valid {
            return Some(0..WIDTH);
        }
        // Most rows don't change from one frame to the next: compare them
        // whole (a row is whole bytes, as `WIDTH` is even) before looking
        // for where they differ a pixel at a time. Built for size, the
        // per-pixel comparison is a function call per pixel, and scanning
        // every row that way made a flush take a third longer.
        let bytes = row * WIDTH / 2..(row + 1) * WIDTH / 2;
        if self.back[bytes.clone()] == self.front[bytes] {
            return None;
        }
        let differs =
            |x: &usize| nibble(&self.back, row * WIDTH + x) != nibble(&self.front, row * WIDTH + x);
        let first = (0..WIDTH).find(differs)?;
        let last = (0..WIDTH)
            .rev()
            .find(differs)
            .expect("first exists, so some column differs");
        Some(first..last + 1)
    }

    /// Sends a rectangle of the canvas to the panel, and records that the
    /// panel now shows it.
    async fn push(&mut self, display: &mut Display, rows: Range<usize>, columns: Range<usize>) {
        let Self {
            back,
            front,
            palette,
            batch,
            ..
        } = self;
        stream(display, batch, rows, columns, |row, x| {
            let i = row * WIDTH + x;
            let color = nibble(back, i);
            set_nibble(front, i, color);
            palette.columns(Color(color))
        })
        .await;
    }

    /// Fills a rectangle with a 1-canvas-pixel checkerboard of the palette's
    /// two dither colours: the classic way to fake an in-between shade on a
    /// limited palette.
    async fn push_dither(
        &mut self,
        display: &mut Display,
        rows: Range<usize>,
        columns: Range<usize>,
    ) {
        let (a, b) = (
            self.palette.columns(Color::DITHER_A),
            self.palette.columns(Color::DITHER_B),
        );
        stream(display, &mut self.batch, rows, columns, |row, x| {
            if (x + row).is_multiple_of(2) { a } else { b }
        })
        .await;
    }
}

/// Streams a canvas rectangle to the panel at [`SCALE`]×, with `color(row, x)`
/// giving each canvas pixel's bytes on the panel's even and odd columns
/// (see [`Palette::columns`]).
///
/// Rows are accumulated in `batch` and sent in as few DMA transfers as
/// possible. Each transfer has a fixed cost (setup, the nRF52832 anomaly-109
/// workaround, an interrupt, a task wake-up), and sending block-sized
/// rectangles a 48-byte line at a time made the transition *slower* than the
/// blocking driver it replaced.
async fn stream(
    display: &mut Display,
    batch: &mut [u8; BATCH],
    rows: Range<usize>,
    columns: Range<usize>,
    mut color: impl FnMut(usize, usize) -> [[u8; 2]; 2],
) {
    // On the canvas, and not empty: the panel's window is given by its
    // last row and column, inclusive, so an empty rectangle would name a
    // window wrapping below zero, and one off the canvas would write
    // outside the panel (and index `front` out of range). A full-width
    // row is the most `batch` holds at once.
    debug_assert!(rows.start < rows.end && rows.end <= HEIGHT);
    debug_assert!(columns.start < columns.end && columns.end <= WIDTH);
    start_pixels(display, rows.clone(), columns.clone()).await;
    let line_len = columns.len() * SCALE * 2;
    let row_len = line_len * SCALE;
    let mut used = 0;
    for row in rows {
        if used + row_len > BATCH {
            display.pixels(&batch[..used]).await;
            used = 0;
        }
        let line = &mut batch[used..used + line_len];
        for x in columns.clone() {
            let bytes = color(row, x);
            let at = (x - columns.start) * SCALE * 2;
            for (i, px) in line[at..at + SCALE * 2]
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .enumerate()
            {
                *px = bytes[(x * SCALE + i) % 2];
            }
        }
        // The vertical half of the 3× scale: repeat the line.
        for copy in 1..SCALE {
            batch.copy_within(used..used + line_len, used + copy * line_len);
        }
        used += row_len;
    }
    if used > 0 {
        display.pixels(&batch[..used]).await;
    }
    display.end_pixels();
}

/// Opens a panel window covering the given canvas rectangle.
async fn start_pixels(display: &mut Display, rows: Range<usize>, columns: Range<usize>) {
    let panel = |r: Range<usize>| (r.start * SCALE) as u16..(r.end * SCALE) as u16;
    display.start_pixels(panel(columns), panel(rows)).await;
}

/// Direction a [`Canvas::transition`] sweeps across the panel: the way the
/// content moves, so the new screen comes in from where the sweep starts.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Sweep {
    LeftToRight,
    RightToLeft,
    TopToBottom,
    BottomToTop,
}

/// Pixel `i` of a packed 4-bit buffer.
fn nibble(buffer: &[u8; BYTES], i: usize) -> u8 {
    if i.is_multiple_of(2) {
        buffer[i / 2] & 0x0F
    } else {
        buffer[i / 2] >> 4
    }
}

fn set_nibble(buffer: &mut [u8; BYTES], i: usize, value: u8) {
    // A colour role is 4 bits (`palette::Color::COUNT` is checked against
    // that); more would spill into the pixel beside it.
    debug_assert!(value < 16, "a colour role over 4 bits");
    let byte = &mut buffer[i / 2];
    *byte = if i.is_multiple_of(2) {
        (*byte & 0xF0) | value
    } else {
        (*byte & 0x0F) | (value << 4)
    };
}

// ==============================================================================
// Drawing at the panel's resolution
// ==============================================================================
//
// Living Tokyo isn't pixel art: it draws its scenes' detail (a noodle bar's
// noren, its stools and its steam) at the panel's own resolution, three
// times the canvas's each way. A frame of that would be 28.8 KB, which the
// nRF52832's RAM hasn't room for, beside the canvas or in place of it. So
// the rows a frame draws finely are drawn twice:
//
// - With the rest of the frame, onto the canvas, where each panel pixel at
//   the centre of a canvas pixel stands for it: the canvas then holds what
//   the panel shows at those centres, which is what `scripts/screenshot.py`
//   and the dev kit compare read, and what a transition sweeps in. The
//   rows touched are noted (`Fine::rows`).
// - After the flush has sent the rest, those rows again (`refine`): the
//   whole frame is drawn again with only eight canvas rows' panel rows
//   taking any of it (`Fine::band`), into `back`, which the flush has
//   just made a copy of `front` and so is free until it's put back from
//   it, and the band's fine rows are sent.
//
// Drawing the frame again for a band costs 4–7 ms on the watch, so it's
// only done for a band whose fine rows may have changed. Whether they have
// is known from what drew them rather than from the pixels, which there's
// no copy of: as the frame's own pass draws, each call's arguments (a
// rectangle's corners and colour, a line of text's place and letters) are
// hashed into each canvas row it touches (`note`). A fine row's pixels are
// all the calls over it, in order, and nothing else, so two frames whose
// calls over a band's fine rows hash the same draw them the same. Those
// hashes are kept in `batch` while the frame is drawn, as nothing is sent
// until it's done; the flush folds each band's into a hash of 15 bits, and
// a band is drawn again only if that differs from the last frame's, or the
// canvas's samples of its fine rows do, or which of them are fine, or the
// panel was sent the frame whole (`Canvas::flushing`). A still screen then
// draws no band at all after its first frame, and a scrolling marquee none
// of the fine scene beside it.
//
// The hash is small to keep the RAM small (`Fine`, 32 bytes): a frame that
// changed a band's fine rows yet hashes the same (one in 32768) and leaves
// the canvas's samples of them alone would leave the band as the last
// frame had it, until it next changes. Most changes move a sample (text, a
// figure, a shape's edge); this is for the fine detail between them. And
// Living Tokyo's scenes go by the clock, so a band drawn again after a
// flush that crossed a second (a transition takes 0.4 s) shows the next
// second's, which the hash, of the frame's own pass, doesn't know of: the
// next frame's hash changes with that second, and draws it again.
//
// A band drawn again sends its fine rows whole, as there's no copy to find
// where it changed: each a full-width row, a transfer of its own. Only
// Living Tokyo draws finely, so only its builds have any of this: Retro's
// are as they were.
//
// TODO: a hash a row, or a row's parts, so that a fine row that changed
// in a part (steam rising) is sent alone, in part, as the canvas's rows
// are: it needs RAM that the dev kit's Living Tokyo `compare` build
// hasn't got.

/// The panel's side, in its own pixels: where [`Canvas::fine_rect`] draws.
#[cfg(feature = "theme-living_tokyo")]
pub const FINE: i32 = FINE_W as i32;
#[cfg(feature = "theme-living_tokyo")]
const FINE_W: usize = WIDTH * SCALE;

/// One canvas row of the panel's pixels, as colour roles: 4 bits each, as
/// on the canvas. Drawn in `back`, a band of them at a time (see
/// [`BAND_ROWS`]), and sent a row at a time from the tail of `batch`,
/// where it's turned into the panel's bytes in place (see [`expand`]).
#[cfg(feature = "theme-living_tokyo")]
const BAND: usize = FINE_W * SCALE / 2;
#[cfg(feature = "theme-living_tokyo")]
const _: () = assert!(BATCH == 4 * BAND, "a band's pixels fill the batch");
/// How many canvas rows [`Canvas::refine`] draws finely at once: as many
/// as `back` holds (eight), so a byte of a [`Rows`] each. The canvas's
/// rows are in bands of these from the top.
#[cfg(feature = "theme-living_tokyo")]
const BAND_ROWS: usize = BYTES / BAND;
#[cfg(feature = "theme-living_tokyo")]
const BANDS: usize = HEIGHT / BAND_ROWS;
#[cfg(feature = "theme-living_tokyo")]
const _: () = assert!(
    BAND_ROWS == 8 && HEIGHT.is_multiple_of(BAND_ROWS),
    "a band a byte of `Rows`"
);

/// Some of the canvas's rows, a bit each, a band (see [`BAND_ROWS`]) a
/// byte.
#[cfg(feature = "theme-living_tokyo")]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Rows([u8; BANDS]);

#[cfg(feature = "theme-living_tokyo")]
impl Rows {
    const NONE: Self = Self([0; BANDS]);

    fn has(&self, row: usize) -> bool {
        self.0[row / BAND_ROWS] >> (row % BAND_ROWS) & 1 != 0
    }
}

/// What a frame's drawing leaves in `batch` while it's drawn (see
/// [`Canvas::note`]), until the flush takes it: a hash of each canvas
/// row's drawing, a `u32` each from the start, then the rows drawn finely,
/// a [`Rows`].
#[cfg(feature = "theme-living_tokyo")]
mod note {
    use super::{BANDS, BATCH, HEIGHT};

    /// Where the rows drawn finely are.
    pub const ROWS: usize = 4 * HEIGHT;
    const _: () = assert!(ROWS + BANDS <= BATCH, "the notes fit the batch");

    /// What each kind of call hashes first, so that one's arguments can't
    /// be taken for another's.
    pub const PIXEL: i32 = 1;
    pub const RECT: i32 = 2;
    pub const SPRITE: i32 = 3;
    pub const TEXT: i32 = 4;
    pub const MEDIUM: i32 = 5;
    pub const MARQUEE: i32 = 6;
    pub const TONE: i32 = 7;
    pub const TYPE: i32 = 8;

    /// Folds `word` into `hash`: for a given hash, a different word always
    /// gives a different one (xor, a rotation, and multiplying by an odd
    /// number are each one-to-one), so a frame whose calls differ from
    /// the last in one argument always hashes differently, before the
    /// fold to 15 bits.
    pub fn mix(hash: u32, word: u32) -> u32 {
        (hash.rotate_left(5) ^ word).wrapping_mul(0x9E37_79B9)
    }
}

/// What the canvas knows of the rows drawn at the panel's resolution:
/// kept small, as it's in RAM in every Living Tokyo build, the dev kit's
/// `compare` build included, which has a few dozen bytes to spare. So the
/// panel's fine rows are known by a short hash a band of what drew them
/// (see above), rather than of their pixels, and a change in a band sends
/// its every fine row.
#[cfg(feature = "theme-living_tokyo")]
struct Fine {
    /// While a band is drawn again finely (see [`Canvas::refine`]), which,
    /// counted from 1 (so that it fits a byte with `None`).
    band: Option<NonZeroU8>,
    /// Whether the frame being drawn has its drawing noted in `batch`
    /// (see [`Canvas::note`]): since its `clear`, until its flush.
    notes: Notes,
    /// The rows this frame draws finely (the last flushed frame's, while
    /// the next is drawn).
    rows: Rows,
    /// Each band's drawing's hash, as of the last flushed frame, in the
    /// low 15 bits; and [`DRAW`], if [`Canvas::refine`] must draw it.
    drawn: [u16; BANDS],
    /// Fine rows a flush left for [`Canvas::refine`] to send: a frame
    /// that draws them must be refined before the next.
    #[cfg(debug_assertions)]
    pending: bool,
    /// In the simulator, each row as last sent, so that a screenshot in
    /// another scheme's colours needn't draw it again (see
    /// [`Canvas::refine_kept`]).
    #[cfg(not(target_os = "none"))]
    kept: Vec<u8>,
    /// In the simulator, the second (UTC) the frame being drawn was drawn
    /// in, and that each band was last drawn again in, if its frame was
    /// drawn in the same second: for `check_unchanged`.
    #[cfg(not(target_os = "none"))]
    seconds: (Option<i64>, [Option<i64>; BANDS]),
}

/// Whether a frame's drawing is being noted (see [`Canvas::note`]).
#[cfg(feature = "theme-living_tokyo")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Notes {
    /// No frame is being drawn (since its flush, while `refine` draws its
    /// bands again).
    Off,
    On,
    /// Within a shape noted whole ([`Canvas::fine_shape`]), whose parts
    /// aren't.
    Whole,
}

/// In [`Fine::drawn`], a band [`Canvas::refine`] must draw again.
#[cfg(feature = "theme-living_tokyo")]
const DRAW: u16 = 1 << 15;

/// How [`Canvas::fine_type`] draws a stroke: in `color` where a pixel's
/// centre is within `core` of the stroke's middle, and in `edge` (a
/// shade) where it's within `outer`, in sixteenths of a pixel.
#[cfg(feature = "theme-living_tokyo")]
#[derive(Clone, Copy)]
struct Pen {
    color: Color,
    edge: Color,
    core: i32,
    outer: i32,
}

#[cfg(feature = "theme-living_tokyo")]
impl Fine {
    const NONE: Self = Self {
        band: None,
        notes: Notes::Off,
        rows: Rows::NONE,
        drawn: [DRAW; BANDS],
        #[cfg(debug_assertions)]
        pending: false,
        #[cfg(not(target_os = "none"))]
        kept: Vec::new(),
        #[cfg(not(target_os = "none"))]
        seconds: (None, [None; BANDS]),
    };

    /// The canvas rows of the band being drawn again, if one is: the
    /// first, and the one past the last.
    fn band_rows(&self) -> Option<(i32, i32)> {
        let first = i32::from(self.band?.get() - 1) * BAND_ROWS as i32;
        Some((first, first + BAND_ROWS as i32))
    }
}

#[cfg(feature = "theme-living_tokyo")]
impl Canvas {
    /// A frame's `clear`, in its own pass: starts noting its drawing (see
    /// `note`), every row's hash from `color`'s, no row fine.
    fn start_notes(&mut self, color: Color) {
        #[cfg(debug_assertions)]
        debug_assert!(!self.fine.pending, "fine rows drawn but never sent");
        let (hashes, rows) = self.batch.split_at_mut(note::ROWS);
        let seed = note::mix(0x811c_9dc5, color.0.into()).to_ne_bytes();
        for hash in hashes.as_chunks_mut::<4>().0 {
            *hash = seed;
        }
        rows[..BANDS].fill(0);
        self.fine.notes = Notes::On;
        #[cfg(not(target_os = "none"))]
        {
            self.fine.seconds.0 = crate::clock::now_utc_seconds();
        }
    }

    /// Notes a call drawing over canvas rows `top..bottom`, with `words`
    /// and `bytes` its arguments, in each of those rows' hashes in
    /// `batch` (see "Drawing at the panel's resolution"), in the frame's
    /// own pass. Not while a band is drawn again, which notes nothing; nor
    /// within a marquee's clip or a [`Canvas::fine_shape`], which note
    /// what they draw whole.
    fn note(&mut self, top: i32, bottom: i32, words: &[i32], bytes: &[u8]) {
        if self.fine.notes != Notes::On || self.clip.is_some() {
            return;
        }
        let (top, bottom) = (top.max(0), bottom.min(HEIGHT as i32));
        if top >= bottom {
            return;
        }
        let mut call = 0;
        for &word in words {
            call = note::mix(call, word as u32);
        }
        for &byte in bytes {
            call = note::mix(call, byte.into());
        }
        let hashes = &mut self.batch.as_chunks_mut::<4>().0[top as usize..bottom as usize];
        for hash in hashes {
            *hash = note::mix(u32::from_ne_bytes(*hash), call).to_ne_bytes();
        }
    }

    /// Draws a shape with `draw`, noted whole (see `note`) by `words`, its
    /// arguments, the first saying which shape it is, over the panel's
    /// rows `rows`, rather than by its every part: for a shape drawn a row
    /// or a pixel at a time, whose many parts would cost the frame's own
    /// pass more to note than to draw.
    pub fn fine_shape(&mut self, rows: Range<i32>, words: &[i32], draw: impl FnOnce(&mut Canvas)) {
        let scale = SCALE as i32;
        self.note(
            rows.start.div_euclid(scale),
            (rows.end + scale - 1).div_euclid(scale),
            words,
            &[],
        );
        let notes = self.fine.notes;
        if notes == Notes::On {
            self.fine.notes = Notes::Whole;
        }
        draw(self);
        self.fine.notes = notes;
    }

    /// Notes canvas rows `top..=last` as drawn finely, in the frame's own
    /// pass (see `note`).
    fn note_fine(&mut self, top: i32, last: i32) {
        for row in top as usize..=last as usize {
            self.batch[note::ROWS + row / BAND_ROWS] |= 1 << (row % BAND_ROWS);
        }
    }

    /// A flush (`whole`: of every row) or a transition is about to send the
    /// canvas, leaving the fine rows to `refine`: takes this frame's fine
    /// rows and its drawing's hashes from `batch` before the sending
    /// overwrites them, and marks the bands `refine` must draw again (see
    /// "Drawing at the panel's resolution"). Returns the rows the panel
    /// shows finely that this frame doesn't draw so, which the canvas must
    /// send whole, as what it holds of them is only their samples.
    fn flushing(&mut self, whole: bool) -> Rows {
        let fine = &mut self.fine;
        // No frame drawn since the last flush (the one that switches the
        // watch off; in the simulator, a screenshot in another scheme's
        // colours): the panel's fine rows are as the last refine left
        // them, unless they're all to be sent again.
        if fine.notes == Notes::Off {
            if whole {
                for drawn in &mut fine.drawn {
                    *drawn |= DRAW;
                }
            }
            return Rows::NONE;
        }
        fine.notes = Notes::Off;
        let (hashes, rows) = self.batch.split_at(note::ROWS);
        let hashes = hashes.as_chunks::<4>().0;
        let mut stale = Rows::NONE;
        let bands = fine
            .rows
            .0
            .iter_mut()
            .zip(&mut fine.drawn)
            .zip(&rows[..BANDS]);
        for (band, ((was, drawn), &now)) in bands.enumerate() {
            stale.0[band] = *was & !now;
            // The band's fine rows' hashes, folded, and whether the canvas
            // (their samples) differs from what the panel was last sent.
            let mut hash = note::mix(0, now.into());
            let mut moved = false;
            for i in (0..BAND_ROWS).filter(|&i| now >> i & 1 != 0) {
                let row = band * BAND_ROWS + i;
                hash = note::mix(hash, u32::from_ne_bytes(hashes[row]));
                let bytes = row * WIDTH / 2..(row + 1) * WIDTH / 2;
                moved |= self.back[bytes.clone()] != self.front[bytes];
            }
            let hash = (hash ^ hash >> 15 ^ hash >> 30) as u16 & !DRAW;
            let owed = *drawn & DRAW != 0;
            let changed = whole || owed || moved || *was != now || hash != *drawn & !DRAW;
            *drawn = hash | if changed && now != 0 { DRAW } else { 0 };
            *was = now;
        }
        #[cfg(debug_assertions)]
        {
            fine.pending = fine.rows != Rows::NONE;
        }
        stale
    }

    /// The panel rows being drawn: all of them, as the frame's drawn, or
    /// the three of the row [`Canvas::refine`] is drawing again. A shape
    /// drawn a pixel at a time can skip the rest.
    pub fn fine_rows(&self) -> Range<i32> {
        match self.fine.band_rows() {
            Some((first, end)) => first * SCALE as i32..end * SCALE as i32,
            None => 0..FINE,
        }
    }

    /// Whether canvas rows `top..bottom` are being drawn: always, but
    /// while [`Canvas::refine`] draws a band of them.
    fn in_band(&self, top: i32, bottom: i32) -> bool {
        self.fine
            .band_rows()
            .is_none_or(|(first, end)| top < end && bottom > first)
    }

    /// Fills a rectangle of the panel's own pixels ([`FINE`] a side),
    /// over whatever the canvas has drawn there, and under what it draws
    /// after. What's off the panel is cut, so shapes may hang off it.
    pub fn fine_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        self.fine_tone(x, y, w, h, 4, color);
    }

    /// Like [`Canvas::fine_rect`], lighting `level` of every four pixels
    /// (in each 2×2): 1, one; 2, a checkerboard; 3, all but one; 4, all.
    /// A pixel is a tenth of a millimetre: these read as shades of
    /// `color` over what's under them, not as dots, so a scene has more
    /// than the palette's sixteen (amber over the dark is wood).
    pub fn fine_tone(&mut self, x: i32, y: i32, w: i32, h: i32, level: u8, color: Color) {
        debug_assert!(w >= 0 && h >= 0, "a rectangle of a negative size");
        debug_assert!((1..=4).contains(&level), "a tone of 1 to 4 in 4");
        let rows = self.fine_rows();
        let (top, bottom) = (y.max(rows.start), (y + h).min(rows.end));
        let (mut left, mut right) = (x.max(0), (x + w).min(FINE));
        if let Some(clip) = &self.clip {
            let scale = SCALE as i32;
            left = left.max(clip.start * scale);
            right = right.min(clip.end * scale);
        }
        if top >= bottom || left >= right {
            return;
        }
        // Which pixels of a row are lit: all, or every other one from the
        // first column of `parity`; or none.
        let lit = |y: i32| -> Option<Option<i32>> {
            let odd = y & 1 == 1;
            match level {
                4 => Some(None),
                3 if !odd => Some(None),
                1 if odd => None,
                2 => Some(Some(y & 1)),
                _ => Some(Some(0)),
            }
        };
        match self.fine.band_rows() {
            Some((first, _)) => {
                for y in top..bottom {
                    let row = (y - first * SCALE as i32) as usize * FINE_W;
                    match lit(y) {
                        Some(None) => {
                            self.band_run(row + left as usize, row + right as usize, color)
                        }
                        Some(Some(parity)) => {
                            let start = left + ((left ^ parity) & 1);
                            for x in (start..right).step_by(2) {
                                self.set_band(row + x as usize, color);
                            }
                        }
                        None => {}
                    }
                }
            }
            None => {
                let scale = SCALE as i32;
                let (first, last) = (top / scale, (bottom - 1) / scale);
                // Packed, as the many small rectangles a scene is made of
                // make this the most noted call: each of these is under
                // 256 (on the panel, as cut to it), and the level and
                // colour under 16.
                self.note(
                    first,
                    last + 1,
                    &[
                        top | bottom << 8 | left << 16 | right << 24,
                        note::TONE << 8 | i32::from(level) << 4 | i32::from(color.0),
                    ],
                    &[],
                );
                self.note_fine(first, last);
                // The canvas pixels whose centres (3c + 1) it lights.
                let centres = |from: i32, to: i32| (from + 1) / scale..(to + 1) / scale;
                for cy in centres(top, bottom) {
                    let Some(every) = lit(cy * scale + 1) else {
                        continue;
                    };
                    for cx in centres(left, right) {
                        if every.is_none_or(|parity| (cx * scale + 1) & 1 == parity) {
                            set_nibble(&mut self.back, cy as usize * WIDTH + cx as usize, color.0);
                        }
                    }
                }
            }
        }
    }

    /// One of the panel's pixels (see [`Canvas::fine_rect`]).
    pub fn fine_pixel(&mut self, x: i32, y: i32, color: Color) {
        self.fine_rect(x, y, 1, 1, color);
    }

    /// `text` in Living Tokyo's lettering (see [`font::fine`]), in
    /// `style`, from `x` and with its capitals' ink from `y`, in the
    /// panel's pixels; and if `edge` is given, its edges smoothed in
    /// that colour, a shade between `color` and what's under the text
    /// (`TEXT_FAINT` round `TEXT` on the night's dark; nothing in the
    /// dim look, whose eight colours have no such shade). Returns where
    /// the next letter would start: the x past the ink and a gap.
    pub fn fine_type(
        &mut self,
        x: i32,
        y: i32,
        text: &str,
        style: font::fine::Style,
        color: Color,
        edge: Option<Color>,
    ) -> i32 {
        // A pixel is lit if its centre is within `core` of a stroke's
        // middle (in sixteenths of a pixel), and shaded if within `outer`:
        // smoothed, a shade from about half a pixel in to half out (less
        // a little inside, so the stroke keeps its weight); or lit to
        // the stroke's edge.
        let radius = i32::from(style.stroke) / 2;
        let pen = match edge {
            Some(edge) => Pen {
                color,
                edge,
                core: radius - 4,
                outer: radius + 6,
            },
            None => Pen {
                color,
                edge: color,
                core: radius,
                outer: radius,
            },
        };
        // A line whose ink is wholly outside the rows being drawn (see
        // `refine`) only needs its end.
        let (above, below) = style.reach();
        let rows = self.fine_rows();
        let drawn = y - above < rows.end && y + below > rows.start;
        // Its ink's rows, and a pixel either side for its shading.
        let scale = SCALE as i32;
        self.note(
            (y - above - 1).div_euclid(scale),
            (y + below + 1).div_euclid(scale) + 1,
            &[
                note::TYPE,
                x,
                y,
                style.cap.into(),
                style.stroke.into(),
                style.caps.into(),
                color.0.into(),
                edge.map_or(-1, |edge| edge.0.into()),
            ],
            text.as_bytes(),
        );
        let mut cursor = x;
        for c in text.chars() {
            let advance = style.advance(c);
            if drawn && cursor < FINE && cursor + advance > 0 {
                let origin = (cursor * 16 + radius, y * 16 + radius);
                font::fine::trace(c, style, origin, |from, to| {
                    self.fine_stroke(from, to, &rows, pen);
                });
            }
            cursor += advance;
        }
        cursor
    }

    /// Draws the line `from`-`to` (in sixteenths of the panel's pixels)
    /// with `pen`, a round one, in `rows`, the panel rows being drawn
    /// (see [`Canvas::fine_type`]).
    fn fine_stroke(&mut self, from: (i32, i32), to: (i32, i32), rows: &Range<i32>, pen: Pen) {
        let ((ax, ay), (bx, by)) = (from, to);
        let reach = pen.outer;
        let top = ((ay.min(by) - reach) >> 4).max(rows.start);
        let bottom = (((ay.max(by) + reach) >> 4) + 1).min(rows.end);
        if top >= bottom {
            return;
        }
        let (mut left, mut right) = (0, FINE);
        if let Some(clip) = &self.clip {
            let scale = SCALE as i32;
            left = clip.start * scale;
            right = clip.end * scale;
        }
        // How near a pixel's centre is: 2 within `core`, 1 within
        // `outer`, else 0. Past either end, from that end, by distances
        // squared; else from the line, the cross product over the line's
        // length (its square root, once a line, so it all stays in 32
        // bits: the cross product of a long stroke's would need 64).
        let (dx, dy) = (bx - ax, by - ay);
        let length = dx * dx + dy * dy;
        let span = length.isqrt();
        let (core, outer) = (pen.core, pen.outer);
        let near = |x: i32, y: i32| -> u8 {
            let (px, py) = (x * 16 + 8 - ax, y * 16 + 8 - ay);
            let along = px * dx + py * dy;
            let (d, (core, outer)) = if along <= 0 || along >= length {
                let (qx, qy) = if along <= 0 {
                    (px, py)
                } else {
                    (px - dx, py - dy)
                };
                (qx * qx + qy * qy, (core * core, outer * outer))
            } else {
                ((px * dy - py * dx).abs(), (core * span, outer * span))
            };
            u8::from(d <= outer) + u8::from(d <= core)
        };
        // Where the line crosses row `y`'s middle (or its nearer end, if
        // it doesn't): lit, if any of the row is, as a stroke is convex.
        // So each row is walked from there out to where it stops, rather
        // than across the line's box.
        let crossing = |y: i32| -> i32 {
            let cy = y * 16 + 8;
            let x = if dy == 0 || (cy - ay) * dy <= 0 {
                ax
            } else if (cy - by) * dy >= 0 {
                bx
            } else {
                ax + (cy - ay) * dx / dy
            };
            x >> 4
        };
        let band = self.fine.band_rows();
        let scale = SCALE as i32;
        // The pixels to walk: a band's every one, or in the frame's own
        // pass only the canvas pixels' centres, as `fine_tone` has them,
        // whose rows it notes as fine.
        let step = if band.is_some() { 1 } else { scale };
        let first_row = match band {
            Some(_) => top,
            None => {
                self.note_fine(top / scale, (bottom - 1) / scale);
                (top + 1) / scale * scale + 1
            }
        };
        for y in (first_row..bottom).step_by(step as usize) {
            // Pixel `x` of this row in `back` (see `refine`), or the
            // canvas pixel it's the centre of.
            let at = |x: i32| match band {
                Some((first, _)) => (y - first * scale) as usize * FINE_W + x as usize,
                None => (y / scale) as usize * WIDTH + (x / scale) as usize,
            };
            // On a centre column, in the frame's own pass.
            let mut start = crossing(y);
            if step > 1 {
                start = start / scale * scale + 1;
            }
            for direction in [-step, step] {
                let mut x = if direction < 0 { start } else { start + step };
                let mut lit = false;
                while x >= left && x < right {
                    let how = near(x, y);
                    // Past the stroke; or the first pixel walked wasn't
                    // in it (its centre to the far side of a thin one),
                    // nor the next.
                    if how == 0 {
                        if lit || (x - start).abs() >= step {
                            break;
                        }
                        x += direction;
                        continue;
                    }
                    lit = true;
                    let i = at(x);
                    // The shade never covers what another of the
                    // letter's strokes has lit.
                    let (byte, shift) = (&mut self.back[i / 2], 4 * (i % 2));
                    if how == 2 || *byte >> shift & 0xF != pen.color.0 {
                        let color = if how == 2 { pen.color } else { pen.edge };
                        *byte = *byte & !(0xF << shift) | color.0 << shift;
                    }
                    x += direction;
                }
            }
        }
    }

    /// Sends the rows this frame drew finely (see [`Canvas::fine_rect`]),
    /// after the flush or transition that sent the rest: each band of
    /// them whose drawing may have changed (see `flushing`) drawn again
    /// with `render`, which draws the frame as it was drawn, and sent.
    ///
    /// They're drawn into `back`, which the flush has just made the same
    /// as `front`, and which is put back from it after: a band of
    /// [`BAND_ROWS`] at once, so the frame is drawn again once for every
    /// eight rows with fine ones rather than for each.
    pub async fn refine(&mut self, display: &mut Display, render: &mut dyn FnMut(&mut Canvas)) {
        #[cfg(debug_assertions)]
        {
            self.fine.pending = false;
        }
        let rows = self.fine.rows;
        if rows == Rows::NONE {
            return;
        }
        // For a dev kit's log (at `debug`), what it cost: the bands drawn
        // again, and the rows sent.
        let started = Instant::now();
        let (mut drawn, mut sent) = (0u8, 0u8);
        for band in 0..BANDS {
            let fine = rows.0[band];
            if fine == 0 {
                continue;
            }
            if self.fine.drawn[band] & DRAW == 0 {
                #[cfg(all(debug_assertions, not(target_os = "none")))]
                self.check_unchanged(band, render);
                continue;
            }
            self.fine.drawn[band] &= !DRAW;
            self.fine.band = NonZeroU8::new(band as u8 + 1);
            render(self);
            drawn += 1;
            self.fine.band = None;
            #[cfg(not(target_os = "none"))]
            {
                let (frame, bands) = &mut self.fine.seconds;
                let now = crate::clock::now_utc_seconds();
                bands[band] = now.filter(|_| now == *frame);
            }
            for i in (0..BAND_ROWS).filter(|&i| fine >> i & 1 != 0) {
                let row = band * BAND_ROWS + i;
                self.batch[BATCH - BAND..].copy_from_slice(&self.back[i * BAND..(i + 1) * BAND]);
                #[cfg(not(target_os = "none"))]
                {
                    let kept = &mut self.fine.kept;
                    kept.resize(HEIGHT * BAND, 0);
                    kept[row * BAND..(row + 1) * BAND].copy_from_slice(&self.batch[BATCH - BAND..]);
                }
                self.send_band(display, row).await;
                sent += 1;
            }
        }
        if drawn > 0 {
            self.back = self.front;
        }
        defmt::debug!(
            "refine: {} bands drawn, {} rows sent, {}us",
            drawn,
            sent,
            started.elapsed().as_micros()
        );
    }

    /// In the simulator's debug builds (its tests), draws a band that
    /// `refine` skips as unchanged anyway, and checks that the panel
    /// already shows its fine rows so: that what `note` hashes is all
    /// that draws them.
    ///
    /// Only if this frame, and the one the band was last drawn for, were
    /// each drawn again within the second they were drawn in: Living
    /// Tokyo's scenes go by the clock (`street::Night::now`), which a
    /// band drawn again after a flush that crossed a second (a transition
    /// takes 0.4 s) reads as the next second's. Then the panel shows that
    /// second's band a frame early, which `refine` can't tell from its
    /// hash; it's drawn again with the next frame that changes it.
    #[cfg(all(debug_assertions, not(target_os = "none")))]
    fn check_unchanged(&mut self, band: usize, render: &mut dyn FnMut(&mut Canvas)) {
        let (frame, bands) = self.fine.seconds;
        let now = crate::clock::now_utc_seconds();
        if now.is_none() || now != frame || bands[band] != frame {
            return;
        }
        let fine = self.fine.rows.0[band];
        self.fine.band = NonZeroU8::new(band as u8 + 1);
        render(self);
        self.fine.band = None;
        for i in (0..BAND_ROWS).filter(|&i| fine >> i & 1 != 0) {
            let row = band * BAND_ROWS + i;
            let kept = self.fine.kept.get(row * BAND..(row + 1) * BAND);
            assert!(
                kept == Some(&self.back[i * BAND..(i + 1) * BAND]),
                "fine row {row} changed, but refine skipped its band"
            );
        }
        self.back = self.front;
    }

    /// In the simulator, sends the fine rows again as they were last
    /// drawn: after a flush in another scheme's colours, for a screenshot
    /// in them, which (as on the dev kit) doesn't draw the frame again.
    #[cfg(not(target_os = "none"))]
    pub async fn refine_kept(&mut self, display: &mut Display) {
        #[cfg(debug_assertions)]
        {
            self.fine.pending = false;
        }
        if self.fine.kept.is_empty() {
            return;
        }
        // The panel shows them all as last drawn, which `refine` would
        // draw them as again.
        for drawn in &mut self.fine.drawn {
            *drawn &= !DRAW;
        }
        let rows = self.fine.rows;
        for row in (0..HEIGHT).filter(|&row| rows.has(row)) {
            let kept = &self.fine.kept[row * BAND..(row + 1) * BAND];
            self.batch[BATCH - BAND..].copy_from_slice(kept);
            self.send_band(display, row).await;
        }
    }

    /// Sends the band, a row of the panel's pixels from the left edge to
    /// the right.
    async fn send_band(&mut self, display: &mut Display, row: usize) {
        expand(&mut self.batch, self.palette);
        start_pixels(display, row..row + 1, 0..WIDTH).await;
        display.pixels(&self.batch).await;
        display.end_pixels();
    }

    /// Sets the band's pixels `from..to` (counted row by row): the
    /// whole bytes between its ends at once.
    fn band_run(&mut self, mut from: usize, to: usize, color: Color) {
        if from < to && from % 2 == 1 {
            self.set_band(from, color);
            from += 1;
        }
        let pairs = (to - from.min(to)) / 2;
        self.back[from / 2..from / 2 + pairs].fill(color.0 | (color.0 << 4));
        if from + 2 * pairs < to {
            self.set_band(to - 1, color);
        }
    }

    /// Sets pixel `at` of the band (row by row of the panel's, in `back`:
    /// see [`Canvas::refine`]).
    fn set_band(&mut self, at: usize, color: Color) {
        debug_assert!(color.0 < 16, "a colour role over 4 bits");
        let byte = &mut self.back[at / 2];
        *byte = if at.is_multiple_of(2) {
            (*byte & 0xF0) | color.0
        } else {
            (*byte & 0x0F) | (color.0 << 4)
        };
    }
}

/// Turns the band in `batch`'s tail into the panel's bytes, filling
/// `batch`, in place: pixel `i`'s two bytes go at `2i`, from its role at
/// `TAIL + i / 2`, so going left to right, each pixel's bytes only ever
/// overwrite roles already read (`2i + 1 < TAIL + (i + 1) / 2` for every
/// pixel but the last; a byte, two pixels, at a time, as checked below).
/// That's what lets the band sent cost no RAM of its own.
#[cfg(feature = "theme-living_tokyo")]
fn expand(batch: &mut [u8; BATCH], palette: &Palette) {
    const TAIL: usize = BATCH - BAND;
    // Byte `i`'s four go at `4i`, beyond none still to be read.
    const _: () = assert!(4 * (BAND - 2) + 3 < TAIL + (BAND - 1));
    // Each role's bytes on even and odd columns, looked up once rather
    // than for each of the 720 pixels: half-toned by the panel's column,
    // as `stream` has them. A byte holds a pixel on an even column (low
    // nibble) and the odd one after it, as a row is an even 240 wide.
    let colors: [[[u8; 2]; 2]; Color::COUNT] =
        core::array::from_fn(|role| palette.columns(Color(role as u8)));
    for i in 0..BAND {
        let byte = batch[TAIL + i];
        let (even, odd) = (
            colors[usize::from(byte & 0x0F)][0],
            colors[usize::from(byte >> 4)][1],
        );
        batch[4 * i..4 * i + 4].copy_from_slice(&[even[0], even[1], odd[0], odd[1]]);
    }
}

// ==============================================================================
// Scrolling text
// ==============================================================================

mod marquee {
    use embassy_time::Duration;

    /// Scroll speed, in canvas pixels per second (about 2.5 characters).
    const SPEED: u64 = 10;
    /// Hold still this long at each end, so the start and end can be read.
    const PAUSE: Duration = Duration::from_millis(1200);
    /// How often to redraw while text scrolls: one pixel per frame.
    pub const MARQUEE_FRAME: Duration = Duration::from_millis(1000 / SPEED);

    /// How long one full pause–scroll–pause cycle takes for text that
    /// overflows its space by `overflow` pixels.
    pub fn marquee_cycle(overflow: i32) -> Duration {
        PAUSE * 2 + Duration::from_millis(overflow.max(0) as u64 * 1000 / SPEED)
    }

    /// How far to have scrolled after `elapsed`.
    pub fn marquee_offset(overflow: i32, elapsed: Duration) -> i32 {
        let travel = marquee_cycle(overflow) - PAUSE * 2;
        let t = Duration::from_ticks(elapsed.as_ticks() % marquee_cycle(overflow).as_ticks());
        if t < PAUSE {
            0
        } else if t < PAUSE + travel {
            ((t - PAUSE).as_millis() * SPEED / 1000) as i32
        } else {
            overflow
        }
    }
}

/// Encodes `bytes` in hex as a QR code, into `rows` (a little-endian
/// `u32` a row; see [`qr::Matrix`]). Not async, so the encoding's
/// working (and the matrix) stay on the stack. False if it can't, which
/// hex always can.
#[inline(never)]
fn qr_rows(bytes: &[u8; 16], rows: &mut [u8]) -> bool {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut text = [0u8; 32];
    for (pair, &byte) in text.chunks_mut(2).zip(bytes) {
        pair[0] = HEX[usize::from(byte >> 4)];
        pair[1] = HEX[usize::from(byte & 15)];
    }
    let Some(matrix) = qr::encode(&text) else {
        return false;
    };
    for (out, row) in rows.as_chunks_mut::<4>().0.iter_mut().zip(&matrix) {
        *out = row.to_le_bytes();
    }
    true
}
