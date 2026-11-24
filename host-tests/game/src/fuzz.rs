//! Fuzzing, with Bolero: where the tests try every row on its own and a
//! few boards, these try any board, and any game of any moves, against
//! the reference and the rules' promises.
//!
//! `cargo test` runs each harness over random inputs (and any saved in a
//! corpus), so they're regression tests too. To fuzz one properly
//! (nightly, for the sanitizer):
//!
//! ```text
//! cargo +nightly bolero test fuzz::any_board_slides_as_the_reference --engine libfuzzer -T 10min
//! ```

use super::game::{Board, CELLS, Direction, Game};
use super::tests::{actual, expected};
use super::{DIRECTIONS, reference, values};

/// The sum of a board's tiles' values.
fn total(b: Board) -> u64 {
    values(b).iter().flatten().map(|&v| u64::from(v)).sum()
}

#[test]
fn any_board_slides_as_the_reference() {
    bolero::check!().with_type::<[u16; 4]>().for_each(|&rows| {
        let b = Board::from_rows(rows);
        for (direction, way) in DIRECTIONS {
            assert_eq!(
                actual(b, direction),
                expected(b, way),
                "{rows:04x?} {direction:?}"
            );
            if let Some((slid, _)) = b.slide(direction) {
                // Merging keeps what's on the board.
                assert_eq!(total(slid), total(b));
                // A move leaves room for its new tile.
                assert!(slid.empty() > 0, "{rows:04x?} {direction:?}");
            }
        }
        // The empty board aside: no slide changes it, so the reference
        // has no move, but its empty cells are moves to `can_move`, on
        // purpose (an empty board, no game yet, isn't over).
        let moves = b == Board::default() || reference::can_move(values(b));
        assert_eq!(b.can_move(), moves, "{rows:04x?}");
    });
}

#[test]
fn any_saved_game_loads() {
    bolero::check!().with_type::<[u8; 16]>().for_each(|&bytes| {
        let game = Game::from_bytes(bytes);
        assert!(game.best >= game.score);
        let over = game.board != Board::default() && !game.board.can_move();
        assert_eq!(game.over(), over);
        // What it saves is what it loaded, bar the best raised.
        assert_eq!(Game::from_bytes(game.to_bytes()), game);
    });
}

#[test]
fn any_game_keeps_the_rules() {
    bolero::check!()
        .with_type::<(u32, Vec<(u8, u32)>)>()
        .for_each(|(seed, touches)| {
            let mut game = Game::new();
            game.restart(*seed);
            for &(kind, entropy) in touches {
                let (before, score, over) = (game.board, game.score, game.over());
                let direction = [
                    Direction::Left,
                    Direction::Right,
                    Direction::Up,
                    Direction::Down,
                ][usize::from(kind % 4)];
                if kind % 8 < 7 {
                    let moved = game.swipe(direction, entropy);
                    assert_eq!(moved, game.board != before);
                    if moved {
                        // The merges' values, and one more tile: a 2 or
                        // a 4.
                        let added = total(game.board) - total(before);
                        assert!(matches!(added, 2 | 4), "{added}");
                        assert!(game.score >= score);
                    }
                } else {
                    game.restart(entropy);
                    assert_eq!(CELLS - game.board.empty(), 2);
                    assert_eq!(game.score, 0);
                }
                assert!(game.best >= game.score);
                assert_ne!(game.board, Board::default(), "a game never empties");
                assert_eq!(game.over(), !game.board.can_move());
                if over && game.over() {
                    assert_eq!(game.board, before, "nothing moves once it's over");
                }
            }
        });
}
