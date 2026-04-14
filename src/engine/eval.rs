use crate::board::board::Board;
use crate::board::piece::Color;

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

pub fn evaluate(board: &Board) -> i32 {
    let mut score = 0;

    score += evaluate_color(board, Color::White);
    score -= evaluate_color(board, Color::Black);

    // Return score from side to move's perspective
    if board.side_to_move == Color::White {
        score
    } else {
        -score
    }
}

fn evaluate_color(board: &Board, color: Color) -> i32 {
    let mut score = 0;
    let side = color.idx();

    // Material and PST
    score += count_material_and_pst(board.pawns[side], PAWN_VALUE, &PAWN_PST, color);
    score += count_material_and_pst(board.knights[side], KNIGHT_VALUE, &KNIGHT_PST, color);
    score += count_material_and_pst(board.bishops[side], BISHOP_VALUE, &BISHOP_PST, color);
    score += count_material_and_pst(board.rooks[side], ROOK_VALUE, &ROOK_PST, color);
    score += count_material_and_pst(board.queens[side], QUEEN_VALUE, &QUEEN_PST, color);

    // King safety (simplified: use midgame or endgame PST based on material)
    let is_endgame = is_endgame(board);
    let king_pst = if is_endgame { &KING_ENDGAME_PST } else { &KING_MIDGAME_PST };
    score += count_material_and_pst(board.kings[side], KING_VALUE, king_pst, color);

    score
}

fn count_material_and_pst(mut bb: u64, piece_value: i32, pst: &[i32; 64], color: Color) -> i32 {
    let mut score = 0;
    while bb != 0 {
        let sq = crate::board::bitboard::pop_lsb(&mut bb);
        score += piece_value;
        
        // PST square mapping
        let pst_sq = if color == Color::White {
            ((7 - (sq / 8)) * 8 + (sq % 8)) as usize
        } else {
            sq as usize
        };
        score += pst[pst_sq];
    }
    score
}

fn is_endgame(board: &Board) -> bool {
    // Very simple endgame detection: no queens, or one queen and no other pieces
    let w_queens = crate::board::bitboard::count_bits(board.queens[0]);
    let b_queens = crate::board::bitboard::count_bits(board.queens[1]);
    
    if w_queens == 0 && b_queens == 0 {
        return true;
    }
    
    // More accurate would be counting total material
    false
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
