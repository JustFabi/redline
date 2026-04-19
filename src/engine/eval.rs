use crate::board::board::Board;
use crate::board::piece::Color;
use crate::board::bitboard::count_bits;

/// Basic evaluation scores (in centipawns)
pub const PAWN_VALUE: i32 = 100;
pub const KNIGHT_VALUE: i32 = 320;
pub const BISHOP_VALUE: i32 = 330;
pub const ROOK_VALUE: i32 = 500;
pub const QUEEN_VALUE: i32 = 900;
pub const KING_VALUE: i32 = 20000;

/// Piece-Square Tables (PST) - Scores from White's perspective
/// For Black, squares are flipped.
/// Values are in centipawns.
#[rustfmt::skip]
pub const PAWN_PST: [i32; 64] = [
    0,  0,  0,  0,  0,  0,  0,  0,
    50, 50, 50, 50, 50, 50, 50, 50,
    10, 10, 20, 30, 30, 20, 10, 10,
    5,  5, 10, 25, 25, 10,  5,  5,
    0,  0,  0, 20, 20,  0,  0,  0,
    5, -5,-10,  0,  0,-10, -5,  5,
    5, 10, 10,-20,-20, 10, 10,  5,
    0,  0,  0,  0,  0,  0,  0,  0
];

#[rustfmt::skip]
pub const KNIGHT_PST: [i32; 64] = [
    -50,-40,-30,-30,-30,-30,-40,-50,
    -40,-20,  0,  0,  0,  0,-20,-40,
    -30,  0, 10, 15, 15, 10,  0,-30,
    -30,  5, 15, 20, 20, 15,  5,-30,
    -30,  0, 15, 20, 20, 15,  0,-30,
    -30,  5, 10, 15, 15, 10,  5,-30,
    -40,-20,  0,  5,  5,  0,-20,-40,
    -50,-40,-30,-30,-30,-30,-40,-50,
];

#[rustfmt::skip]
pub const BISHOP_PST: [i32; 64] = [
    -20,-10,-10,-10,-10,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5, 10, 10,  5,  0,-10,
    -10,  5,  5, 10, 10,  5,  5,-10,
    -10,  0, 10, 10, 10, 10,  0,-10,
    -10, 10, 10, 10, 10, 10, 10,-10,
    -10,  5,  0,  0,  0,  0,  5,-10,
    -20,-10,-10,-10,-10,-10,-10,-20,
];

#[rustfmt::skip]
pub const ROOK_PST: [i32; 64] = [
    0,  0,  0,  0,  0,  0,  0,  0,
    5, 10, 10, 10, 10, 10, 10,  5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    0,  0,  0,  5,  5,  0,  0,  0
];

#[rustfmt::skip]
pub const QUEEN_PST: [i32; 64] = [
    -20,-10,-10, -5, -5,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5,  5,  5,  5,  0,-10,
    -5,  0,  5,  5,  5,  5,  0, -5,
    0,  0,  5,  5,  5,  5,  0, -5,
    -10,  5,  5,  5,  5,  5,  0,-10,
    -10,  0,  5,  0,  0,  0,  0,-10,
    -20,-10,-10, -5, -5,-10,-10,-20
];

#[rustfmt::skip]
pub const KING_MIDGAME_PST: [i32; 64] = [
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -20,-30,-30,-40,-40,-30,-30,-20,
    -10,-20,-20,-20,-20,-20,-20,-10,
    20, 20,  0,  0,  0,  0, 20, 20,
    20, 30, 10,  0,  0, 10, 30, 20
];

#[rustfmt::skip]
pub const KING_ENDGAME_PST: [i32; 64] = [
    -50,-40,-30,-20,-20,-30,-40,-50,
    -30,-20,-10,  0,  0,-10,-20,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-30,  0,  0,  0,  0,-30,-30,
    -50,-30,-30,-30,-30,-30,-30,-50
];

const PHASE_VALUES: [i32; 6] = [0, 1, 1, 2, 4, 0]; // Pawn, Knight, Bishop, Rook, Queen, King

pub fn evaluate(board: &Board) -> i32 {
    let mut midgame = 0;
    let mut endgame = 0;
    let mut phase = 0;

    // Material and PST for both sides
    for color in [Color::White, Color::Black] {
        let side = color.idx();
        let multiplier = if color == Color::White { 1 } else { -1 };

        // Material values (using simple centipawns)
        let w_pawns = board.pawns[side];
        let w_knights = board.knights[side];
        let w_bishops = board.bishops[side];
        let w_rooks = board.rooks[side];
        let w_queens = board.queens[side];
        let w_kings = board.kings[side];

        phase += count_bits(w_knights) as i32 * PHASE_VALUES[1];
        phase += count_bits(w_bishops) as i32 * PHASE_VALUES[2];
        phase += count_bits(w_rooks) as i32 * PHASE_VALUES[3];
        phase += count_bits(w_queens) as i32 * PHASE_VALUES[4];

        let mut eval_pieces = |mut bb: u64, val: i32, pst: &[i32; 64]| {
            while bb != 0 {
                let sq = crate::board::bitboard::pop_lsb(&mut bb);
                let pst_sq = if color == Color::White { ((7 - (sq / 8)) * 8 + (sq % 8)) as usize } else { sq as usize };
                midgame += multiplier * (val + pst[pst_sq]);
                endgame += multiplier * (val + pst[pst_sq]);
            }
        };

        eval_pieces(w_pawns, PAWN_VALUE, &PAWN_PST);
        eval_pieces(w_knights, KNIGHT_VALUE, &KNIGHT_PST);
        eval_pieces(w_bishops, BISHOP_VALUE, &BISHOP_PST);
        eval_pieces(w_rooks, ROOK_VALUE, &ROOK_PST);
        eval_pieces(w_queens, QUEEN_VALUE, &QUEEN_PST);

        // King PST (Midgame/Endgame split)
        let mut king_bb = w_kings;
        while king_bb != 0 {
            let sq = crate::board::bitboard::pop_lsb(&mut king_bb);
            let pst_sq = if color == Color::White { ((7 - (sq / 8)) * 8 + (sq % 8)) as usize } else { sq as usize };
            midgame += multiplier * (KING_VALUE + KING_MIDGAME_PST[pst_sq]);
            endgame += multiplier * (KING_VALUE + KING_ENDGAME_PST[pst_sq]);
        }
    }

    // Phase calculation (24 is starting material phase)
    let phase = (phase * 256 + 12) / 24;
    let score = ((midgame * (256 - phase)) + (endgame * phase)) / 256;

    // Return score from side to move's perspective
    if board.side_to_move == Color::White { score } else { -score }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;

    #[test]
    fn test_initial_eval() {
        let board = Board::startpos();
        // Startpos should be equal (0)
        assert_eq!(evaluate(&board), 0);
    }

    #[test]
    fn test_material_advantage() {
        let mut board = Board::startpos();
        // Remove a black pawn
        board.pawns[1] &= !crate::board::bitboard::bit(52); // e7
        board.update_occupancy();
        
        let score = evaluate(&board);
        assert!(score > 0); // White should be better
    }
}
