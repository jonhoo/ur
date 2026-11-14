//! 2^, the hidden puzzle: sliding tiles on a 4×4 board, by 2048's rules.
//!
//! - A swipe slides every tile as far as it goes that way. Two equal
//!   tiles that meet merge into one of their sum, once a move: the pair
//!   nearest the wall the tiles slide towards first, so `2 2 2 _`
//!   slid left is `4 2 _ _`, and `2 2 2 2` is `4 4 _ _`, not `8`.
//! - A move that changes the board brings a new tile, on an empty cell
//!   chosen at random: a 2, nine times in ten, or a 4. A move that
//!   changes nothing brings none, and doesn't count.
//! - The score goes up by each merged tile's value; the best score is
//!   the highest score yet, kept with the game.
//! - A board with no move left is over ([`Game::over`]).
//!
//! What 2048 has and this leaves out, to cost the firmware as little as
//! it can: a win (making a 2048 is only a bigger tile here; play goes
//! on), and an undo. The UI (`crate::ui`, from its Info screen) opens it,
//! draws it, and hands it swipes; this module is only the rules and the
//! state, hardware-free, so `host-tests/game` checks it against a plain
//! implementation of the same rules.
//!
//! ## Packing
//!
//! RAM is tight (the `compare` build has only tens of bytes free), so a
//! cell is a 4-bit exponent (0 empty, 1 for a 2, … 11 for 2048), and a
//! row of four is a `u16`: the board is 8 bytes, and the whole [`Game`]
//! 16, with the score and the best. The cost is a ceiling: 15 is 32768,
//! and two of them don't merge, as 65536 would need a fifth bit.
//! Reaching two 32768s on a 4×4 board is the stuff of record attempts,
//! not of a watch.
//! TODO: 5-bit cells (2 bytes more) if anyone ever gets there.
//!
//! ## Randomness
//!
//! The watch's hardware RNG belongs to the Bluetooth controller, which
//! takes it at boot (see `main`), so the new tile comes from the move's
//! own swipe instead: when it started and how long the finger was down,
//! to the millisecond, and where, which no finger controls at that
//! resolution ([`entropy`]), mixed with the board and the score
//! ([`Game::roll`]). Unpredictable to whoever plays, and yet the same
//! for the same touches: the simulator's scripts give the same boards on
//! every run, and `ur-sim --devkit` follows a dev kit's game, as the
//! `compare` build exports each touch's timing anyway.
//!
//! ## Saving
//!
//! [`Game::to_bytes`] is the whole game, 16 bytes: whether it's over
//! comes back from the board. The UI saves it when the wearer leaves the
//! game, rather than after every move, which would be a flash write a
//! move and wear the storage pages (see `ui::App::take_game_save`).

/// How many cells a side.
pub const SIDE: usize = 4;
/// How many cells.
pub const CELLS: usize = SIDE * SIDE;
/// The highest exponent a cell holds (see the module docs).
pub const MAX: u8 = 15;

/// Which way a swipe slides the tiles: the way the finger moved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// The board: a row a `u16`, top to bottom; in each, a cell a nibble,
/// the leftmost the lowest. A nibble is the tile's exponent, 0 for none.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Board {
    rows: [u16; SIDE],
}

impl Board {
    pub const fn from_rows(rows: [u16; SIDE]) -> Self {
        Self { rows }
    }

    /// The exponent in cell `index` (`row * 4 + column`), 0 if empty.
    pub fn cell(self, index: usize) -> u8 {
        (self.rows[index / SIDE] >> (4 * (index % SIDE)) & 0xF) as u8
    }

    fn set(&mut self, index: usize, exponent: u8) {
        let shift = 4 * (index % SIDE);
        let row = &mut self.rows[index / SIDE];
        *row = *row & !(0xF << shift) | u16::from(exponent & 0xF) << shift;
    }

    /// How many cells are empty.
    pub fn empty(self) -> usize {
        (0..CELLS).filter(|&i| self.cell(i) == 0).count()
    }

    /// Rows become columns: the up and down slides are the left and right
    /// ones of the transposed board.
    fn transpose(self) -> Self {
        let mut out = Self::default();
        for row in 0..SIDE {
            for column in 0..SIDE {
                out.set(column * SIDE + row, self.cell(row * SIDE + column));
            }
        }
        out
    }

    /// The board slid `direction`, with the points its merges made;
    /// `None` if nothing moved, which isn't a move.
    ///
    /// Transposing (for up and down) and reversing rows (for right and
    /// down) around one left slide of each row, rather than reading each
    /// line out of the board by a per-direction index: that was simpler
    /// to follow, but ~220 bytes more, its index inlined at every cell.
    pub fn slide(self, direction: Direction) -> Option<(Board, u32)> {
        let transposed = matches!(direction, Direction::Up | Direction::Down);
        let towards_end = matches!(direction, Direction::Right | Direction::Down);
        let mut out = if transposed { self.transpose() } else { self };
        let mut points = 0;
        for row in &mut out.rows {
            // Sliding towards the end is sliding towards the start of the
            // row reversed.
            let flip = |row: u16| if towards_end { reverse(row) } else { row };
            let (slid, row_points) = slide_row(flip(*row));
            *row = flip(slid);
            points += row_points;
        }
        if transposed {
            out = out.transpose();
        }
        (out != self).then_some((out, points))
    }

    /// Whether any slide would change the board: an empty cell, or two
    /// equal tiles side by side (that can still merge).
    pub fn can_move(self) -> bool {
        (0..CELLS).any(|i| {
            let (row, column) = (i / SIDE, i % SIDE);
            let tile = self.cell(i);
            let right = column + 1 < SIDE && self.cell(i + 1) == tile;
            let below = row + 1 < SIDE && self.cell(i + SIDE) == tile;
            tile == 0 || tile < MAX && (right || below)
        })
    }

    /// The board with a new tile from `roll` (see [`Game::roll`]): on one
    /// of the empty cells, each as likely, from the roll's top half; a 4
    /// if its bottom half is a multiple of ten (6554 of 65536, 10.0%),
    /// else a 2. `None` if the board is full.
    pub fn spawn(self, roll: u32) -> Option<Board> {
        // Multiply and shift, rather than a remainder: as even a split as
        // 16 bits allow, and no division.
        let nth = ((roll >> 16) as usize * self.empty()) >> 16;
        let index = (0..CELLS).filter(|&i| self.cell(i) == 0).nth(nth)?;
        let exponent = if (roll & 0xFFFF).is_multiple_of(10) {
            2
        } else {
            1
        };
        let mut board = self;
        board.set(index, exponent);
        Some(board)
    }
}

/// One row slid towards its start (the low nibble): its tiles packed
/// there, each pair of equal neighbours merged once, the first pair
/// first. With the points the merges made.
pub fn slide_row(row: u16) -> (u16, u32) {
    let mut out = 0u16;
    let mut placed = 0;
    // The last tile placed, if it can still take a merge: not one that a
    // merge made this move.
    let mut open = None;
    let mut points = 0;
    for column in 0..SIDE {
        let tile = row >> (4 * column) & 0xF;
        if tile == 0 {
            continue;
        }
        if open == Some(tile) && tile < u16::from(MAX) {
            // One more on the last tile placed: its value doubled.
            out += 1 << (4 * (placed - 1));
            points += 1 << (tile + 1);
            open = None;
        } else {
            out |= tile << (4 * placed);
            placed += 1;
            open = Some(tile);
        }
    }
    (out, points)
}

/// A row's cells in the other order.
fn reverse(row: u16) -> u16 {
    (row & 0xF) << 12 | (row >> 4 & 0xF) << 8 | (row >> 8 & 0xF) << 4 | row >> 12
}

/// A touch's contribution to the next roll: when it started and how long
/// it lasted, in milliseconds, and where, in canvas pixels (see the
/// module docs). Folded, rather than mixed: [`Game::roll`] mixes.
pub fn entropy(start_ms: u32, held_ms: u16, x: i32, y: i32) -> u32 {
    start_ms ^ u32::from(held_ms).rotate_left(16) ^ (x as u32) << 8 ^ (y as u32) << 24
}

/// murmur3's 32-bit finaliser: every bit of the input flips each bit of
/// the output with even odds, so neighbouring inputs (two swipes a
/// millisecond apart) give unrelated rolls.
fn mix(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ h >> 16
}

/// A game, and the best score: 16 bytes (see the module docs).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Game {
    pub board: Board,
    pub score: u32,
    pub best: u32,
}

impl Game {
    /// What [`Game::to_bytes`] makes.
    pub const BYTES: usize = 16;

    /// No game yet: an empty board, and no best score.
    pub const fn new() -> Self {
        Self {
            board: Board { rows: [0; SIDE] },
            score: 0,
            best: 0,
        }
    }

    /// A game as saved by [`Game::to_bytes`]. Any 16 bytes are a game:
    /// any nibble is a tile, and the best is at least the score.
    pub fn from_bytes(bytes: [u8; Self::BYTES]) -> Self {
        let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        let u32_at =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let score = u32_at(8);
        Self {
            board: Board::from_rows([u16_at(0), u16_at(2), u16_at(4), u16_at(6)]),
            score,
            best: u32_at(12).max(score),
        }
    }

    /// The board, the score and the best, to save.
    pub fn to_bytes(self) -> [u8; Self::BYTES] {
        let mut bytes = [0; Self::BYTES];
        for (i, row) in self.board.rows.iter().enumerate() {
            bytes[2 * i..2 * i + 2].copy_from_slice(&row.to_le_bytes());
        }
        bytes[8..12].copy_from_slice(&self.score.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.best.to_le_bytes());
        bytes
    }

    /// Whether the game is over: no move is left. (Never so of an empty
    /// board, no game yet: its empty cells are moves.)
    pub fn over(&self) -> bool {
        !self.board.can_move()
    }

    /// The next roll: the touch's `entropy` with the board and the score
    /// (see the module docs).
    fn roll(&self, entropy: u32) -> u32 {
        let [a, b, c, d] = self.board.rows.map(u32::from);
        mix(entropy ^ mix(a | b << 16) ^ mix(c | d << 16 ^ self.score))
    }

    /// The wearer opened the game, with a touch: the one there was, as it
    /// was, or a new one if there's none yet (when it says so, `true`).
    pub fn open(&mut self, entropy: u32) -> bool {
        let new = self.board == Board::default();
        if new {
            self.restart(entropy);
        }
        new
    }

    /// A new game, from the touch that asked for it: two tiles, and the
    /// score back to 0 (the best stays).
    pub fn restart(&mut self, entropy: u32) {
        self.board = Board::default();
        self.score = 0;
        for _ in 0..2 {
            // The board changes between the two, so their rolls differ.
            if let Some(board) = self.board.spawn(self.roll(entropy)) {
                self.board = board;
            }
        }
    }

    /// A swipe `direction`: a move, if it changes the board. Returns
    /// whether it did.
    pub fn swipe(&mut self, direction: Direction, entropy: u32) -> bool {
        let Some((slid, points)) = self.board.slide(direction) else {
            return false;
        };
        // Never `None`: a slide that moved left a cell empty, either by a
        // merge or as a row that moved had one (host-tests/game checks).
        // Were it full, the move would stand without a new tile.
        self.board = slid.spawn(self.roll(entropy)).unwrap_or(slid);
        self.score = self.score.saturating_add(points);
        self.best = self.best.max(self.score);
        true
    }
}
