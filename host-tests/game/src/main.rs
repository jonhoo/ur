//! Host tests for the firmware's `game` module, 2^'s rules: against a
//! reference implementation of the same rules, written the plain way (a
//! 4×4 array of values, nothing packed), every row there is, and
//! Bolero's boards (`src/fuzz.rs`). `cargo test` runs them.
//!
//! No Kani proofs: a row is a `u16`, so `every_row_slides_as_the_reference`
//! tries all 65,536 of them, which is what a proof of the row's slide
//! would show; and a board's slide is four rows'.

// Used only by the tests.
#[path = "../../../src/game.rs"]
#[allow(dead_code)]
mod game;

fn main() {
    println!("run `cargo test`");
}

/// The rules, the plain way: values (0 empty), not exponents.
#[cfg(test)]
mod reference {
    pub type Grid = [[u32; 4]; 4];

    /// One line slid towards its start: the tiles packed there, each
    /// pair of equal neighbours merged once, the first pair first; with
    /// the points. Bar 32768s, which the firmware's 4 bits can't merge
    /// (see `game`).
    pub fn slide_line(line: [u32; 4]) -> ([u32; 4], u32) {
        let tiles: Vec<u32> = line.into_iter().filter(|&v| v != 0).collect();
        let mut out = Vec::new();
        let mut points = 0;
        let mut i = 0;
        while i < tiles.len() {
            if i + 1 < tiles.len() && tiles[i] == tiles[i + 1] && tiles[i] < 32768 {
                out.push(tiles[i] * 2);
                points += tiles[i] * 2;
                i += 2;
            } else {
                out.push(tiles[i]);
                i += 1;
            }
        }
        let mut line = [0; 4];
        line[..out.len()].copy_from_slice(&out);
        (line, points)
    }

    /// The grid slid one way: `dx`/`dy` the way the tiles go.
    pub fn slide(grid: Grid, dx: i32, dy: i32) -> (Grid, u32) {
        let mut out = [[0; 4]; 4];
        let mut points = 0;
        for k in 0..4 {
            // The line's places, from the wall the tiles go towards.
            let places: Vec<(usize, usize)> = (0..4)
                .map(|i| match (dx, dy) {
                    (-1, 0) => (k, i),
                    (1, 0) => (k, 3 - i),
                    (0, -1) => (i, k),
                    _ => (3 - i, k),
                })
                .collect();
            let line = [0, 1, 2, 3].map(|i| grid[places[i].0][places[i].1]);
            let (slid, p) = slide_line(line);
            points += p;
            for i in 0..4 {
                let (r, c) = places[i];
                out[r][c] = slid[i];
            }
        }
        (out, points)
    }

    pub const WAYS: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    /// Whether any way moves a tile.
    pub fn can_move(grid: Grid) -> bool {
        WAYS.iter().any(|&(dx, dy)| slide(grid, dx, dy).0 != grid)
    }
}

/// A board from values, row by row (0 empty).
#[cfg(test)]
fn board(values: [[u32; 4]; 4]) -> game::Board {
    let rows = values.map(|row| {
        row.iter().enumerate().fold(0u16, |packed, (i, &v)| {
            let exponent = if v == 0 { 0 } else { v.trailing_zeros() as u16 };
            packed | exponent << (4 * i)
        })
    });
    game::Board::from_rows(rows)
}

/// A board's values, row by row.
#[cfg(test)]
fn values(board: game::Board) -> [[u32; 4]; 4] {
    let mut out = [[0; 4]; 4];
    for (i, cell) in out.iter_mut().flatten().enumerate() {
        let e = board.cell(i);
        *cell = if e == 0 { 0 } else { 1 << e };
    }
    out
}

/// A direction, and the way its tiles go, for the reference.
#[cfg(test)]
const DIRECTIONS: [(game::Direction, (i32, i32)); 4] = [
    (game::Direction::Left, (-1, 0)),
    (game::Direction::Right, (1, 0)),
    (game::Direction::Up, (0, -1)),
    (game::Direction::Down, (0, 1)),
];

#[cfg(test)]
mod tests {
    use super::game::*;
    use super::{DIRECTIONS, board, reference, values};

    /// A board's slide, as the reference has it: `None` if nothing moved,
    /// else the values and the points.
    pub fn expected(b: Board, way: (i32, i32)) -> Option<([[u32; 4]; 4], u32)> {
        let (grid, points) = reference::slide(values(b), way.0, way.1);
        (grid != values(b)).then_some((grid, points))
    }

    pub fn actual(b: Board, direction: Direction) -> Option<([[u32; 4]; 4], u32)> {
        b.slide(direction)
            .map(|(slid, points)| (values(slid), points))
    }

    /// The cells two boards differ in.
    fn changed(a: Board, b: Board) -> Vec<usize> {
        (0..CELLS).filter(|&i| a.cell(i) != b.cell(i)).collect()
    }

    #[test]
    fn every_row_slides_as_the_reference() {
        for row in 0..=u16::MAX {
            let b = Board::from_rows([row, 0, 0, 0]);
            for (direction, way) in DIRECTIONS {
                assert_eq!(
                    actual(b, direction),
                    expected(b, way),
                    "{row:04x} {direction:?}"
                );
            }
        }
    }

    #[test]
    fn pairs_merge_once_nearest_the_wall_first() {
        let cases = [
            ([2, 2, 2, 0], [4, 2, 0, 0], 4),
            ([2, 2, 2, 2], [4, 4, 0, 0], 8),
            ([4, 2, 2, 0], [4, 4, 0, 0], 4),
            ([2, 0, 0, 2], [4, 0, 0, 0], 4),
            ([8, 8, 16, 16], [16, 32, 0, 0], 48),
            ([2, 4, 8, 16], [2, 4, 8, 16], 0),
        ];
        for (row, after, points) in cases {
            let b = board([row, [0; 4], [0; 4], [0; 4]]);
            match b.slide(Direction::Left) {
                Some((slid, got)) => {
                    assert_eq!(values(slid)[0], after, "{row:?}");
                    assert_eq!(got, points, "{row:?}");
                }
                None => assert_eq!(row, after, "{row:?} moved nothing, yet should have"),
            }
        }
        // Rightwards, the pair nearest the right wall first.
        let b = board([[2, 2, 2, 0], [0; 4], [0; 4], [0; 4]]);
        let (slid, _) = b.slide(Direction::Right).expect("it moves");
        assert_eq!(values(slid)[0], [0, 0, 2, 4]);
    }

    #[test]
    fn two_32768s_stay_apart() {
        let b = board([[32768, 32768, 0, 0], [0; 4], [0; 4], [0; 4]]);
        assert!(b.slide(Direction::Left).is_none());
        let full = board([
            [32768, 32768, 2, 4],
            [4, 2, 4, 2],
            [2, 4, 2, 4],
            [4, 2, 4, 2],
        ]);
        assert!(!full.can_move());
    }

    #[test]
    fn a_move_brings_a_tile_on_an_empty_cell() {
        let b = board([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        for roll in [0, 1, 0xFFFF_FFFF, 0x1234_5678, 0x8000_0000] {
            let next = b.spawn(roll).expect("there's room");
            let [cell] = changed(b, next)[..] else {
                panic!("one new tile")
            };
            assert_eq!(b.cell(cell), 0);
            assert!(matches!(next.cell(cell), 1 | 2));
        }
        let full = board([[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]]);
        assert!(full.spawn(7).is_none());
    }

    /// Every empty cell as likely, and one in ten a 4, over every roll's
    /// top and bottom halves.
    #[test]
    fn spawns_are_fair() {
        let b = board([[2, 0, 0, 0], [0, 4, 0, 0], [0, 0, 0, 0], [0, 0, 0, 8]]);
        let mut cells = [0u32; CELLS];
        let mut fours = 0u32;
        for high in 0..=u16::MAX as u32 {
            let next = b.spawn(high << 16 | 1).expect("room");
            cells[changed(b, next)[0]] += 1;
        }
        for low in 0..=u16::MAX as u32 {
            let next = b.spawn(low).expect("room");
            fours += u32::from(next.cell(changed(b, next)[0]) == 2);
        }
        let empty: Vec<u32> = (0..CELLS)
            .filter(|&i| b.cell(i) == 0)
            .map(|i| cells[i])
            .collect();
        assert_eq!(empty.len(), 13);
        let (min, max) = (empty.iter().min().unwrap(), empty.iter().max().unwrap());
        assert!(max - min <= 1, "{empty:?}");
        assert_eq!(fours, 6554, "a 4 for one roll in ten (rounded up)");
    }

    #[test]
    fn a_new_game_has_two_tiles_and_no_score() {
        for entropy in [0, 1, 42, 0xDEAD_BEEF] {
            let mut g = Game::from_bytes(bytes(board([[8, 16, 0, 0]; 4]), 500, 1000));
            g.restart(entropy);
            assert_eq!(CELLS - g.board.empty(), 2);
            assert!((0..CELLS).all(|i| matches!(g.board.cell(i), 0..=2)));
            assert_eq!((g.score, g.best), (0, 1000));
            assert!(!g.over());
        }
    }

    #[test]
    fn a_move_scores_its_merges_and_keeps_the_best() {
        let start = board([[4, 4, 0, 0], [2, 2, 0, 0], [0; 4], [0; 4]]);
        let mut g = Game::from_bytes(bytes(start, 100, 105));
        assert!(g.swipe(Direction::Left, 7));
        assert_eq!(g.score, 112);
        assert_eq!(g.best, 112);
        assert_eq!(values(g.board)[0][0], 8);
        assert_eq!(values(g.board)[1][0], 4);
        // The two merged, and one new tile.
        assert_eq!(CELLS - g.board.empty(), 3);
        // Nothing moves: nothing happens.
        let wall = board([[8, 0, 0, 0], [4, 0, 0, 0], [0; 4], [0; 4]]);
        let mut g = Game::from_bytes(bytes(wall, 0, 0));
        assert!(!g.swipe(Direction::Left, 7));
        assert_eq!(g.board, wall);
    }

    #[test]
    fn a_2048_is_only_a_bigger_tile() {
        let start = board([[1024, 1024, 0, 0], [0; 4], [0; 4], [0; 4]]);
        let mut g = Game::from_bytes(bytes(start, 0, 0));
        assert!(g.swipe(Direction::Left, 3));
        assert_eq!(g.score, 2048);
        assert!(!g.over());
        assert!(g.swipe(Direction::Right, 4));
    }

    #[test]
    fn no_move_left_is_over() {
        // One move left (the top row, right), whose new tile (a 2 or a
        // 4, top left) fills the board for good, whichever comes.
        let start = board([
            [8, 16, 32, 0],
            [64, 128, 256, 512],
            [1024, 8, 16, 32],
            [2, 4, 2, 4],
        ]);
        for entropy in 0..50 {
            let mut g = Game::from_bytes(bytes(start, 50, 60));
            assert!(!g.over());
            assert!(g.swipe(Direction::Right, entropy));
            assert!(g.over(), "{:?}", values(g.board));
            for (direction, _) in DIRECTIONS {
                assert!(!g.swipe(direction, entropy));
            }
            g.restart(entropy);
            assert!(!g.over());
            assert_eq!((g.score, g.best), (0, 60));
        }
        // Loaded with no move left: over. No game yet isn't.
        let full = board([[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]]);
        assert!(Game::from_bytes(bytes(full, 9, 9)).over());
        assert!(!Game::new().over());
    }

    fn bytes(b: Board, score: u32, best: u32) -> [u8; Game::BYTES] {
        Game {
            board: b,
            score,
            best,
        }
        .to_bytes()
    }

    #[test]
    fn saving_keeps_the_board_and_the_scores() {
        let mut g = Game::new();
        g.restart(77);
        for (i, (direction, _)) in DIRECTIONS.iter().cycle().take(30).enumerate() {
            g.swipe(*direction, i as u32);
        }
        assert_eq!(Game::from_bytes(g.to_bytes()), g);
        // The layout: the rows, then the score and the best, little-endian.
        let b = board([[2, 4, 0, 0], [0; 4], [0; 4], [0; 4]]);
        let saved = bytes(b, 0x0102_0304, 0x0506_0708);
        assert_eq!(saved, [0x21, 0, 0, 0, 0, 0, 0, 0, 4, 3, 2, 1, 8, 7, 6, 5]);
        // A best lower than the score (never saved so) is raised.
        assert_eq!(Game::from_bytes(bytes(b, 10, 3)).best, 10);
    }

    /// The same touches, the same game: what the simulator and
    /// `ur-sim --devkit` rely on.
    #[test]
    fn the_same_touches_give_the_same_game() {
        let play = || {
            let mut g = Game::new();
            g.restart(entropy(1000, 80, 40, 40));
            for i in 0..50u32 {
                let (direction, _) = DIRECTIONS[(i % 4) as usize];
                g.swipe(direction, entropy(2000 + i * 467, 80, 30, 50));
            }
            g
        };
        assert_eq!(play(), play());
    }
}

#[cfg(test)]
mod fuzz;
