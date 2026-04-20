// Pawn move generation using bitboards.

use crate::board::bitboard::*;
use crate::board::board::Board;
use crate::board::piece::Color;
use crate::board::r#move::{Move, flags};
use crate::movegen::move_list::MoveList;
use crate::movegen::GenType;

static mut PAWN_ATTACKS: [[u64; 64]; 2] = [[0; 64]; 2];

pub fn init_pawn_attacks() {
    for sq in 0..64 {
        let b = bit(sq as u8);
        unsafe {
            PAWN_ATTACKS[Color::White.idx()][sq] = ((b & !0x0101010101010101) << 7) | ((b & !0x8080808080808080) << 9);
            PAWN_ATTACKS[Color::Black.idx()][sq] = ((b & !0x0101010101010101) >> 9) | ((b & !0x8080808080808080) >> 7);
        }
    }
}

#[inline(always)]
pub fn get_pawn_attacks(sq: u8, color: Color) -> u64 {
    unsafe { PAWN_ATTACKS[color.idx()][sq as usize] }
}

/// Generates all pawn moves for the current side
///
/// Moves are pushed into `moves` vector
pub fn generate_pawn_moves(board: &Board, moves: &mut MoveList, gen_type: GenType) {
    match board.side_to_move {
        Color::White => generate_white_pawns(board, moves, gen_type),
        Color::Black => generate_black_pawns(board, moves, gen_type),
        Color::None => {},
    }
}

/// =======================
/// WHITE PAWNS
/// =======================
fn generate_white_pawns(board: &Board, moves: &mut MoveList, gen_type: GenType) {
    let pawns = board.pawns[Color::White.idx()];
    let empty = !board.all_occupancy;
    let enemy = board.occupancy[Color::Black.idx()];

    let rank8 = 0xFF00000000000000;
    let rank2 = 0x000000000000FF00;

    let single_push = (pawns << 8) & empty;

    if gen_type != GenType::Captures {
        // =========================
        // Single pushes (1 square forward)
        // =========================
        let mut bb = single_push & !rank8; // not promotion
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            moves.push(Move::new(to - 8, to, flags::QUIET));
        }

        // =========================
        // Double pushes (from rank 2)
        // =========================
        let double_push = ((pawns & rank2) << 16) & empty & (empty << 8);

        let mut bb = double_push;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            moves.push(Move::new(to - 16, to, flags::DOUBLE_PAWN));
        }
    }

    let mut bb = single_push & rank8; // promotions
    while bb != 0 {
        let to = pop_lsb(&mut bb);
        let from = to - 8;
        moves.push(Move::new(from, to, flags::PROMOTE_QUEEN));
        if gen_type != GenType::Captures {
            moves.push(Move::new(from, to, flags::PROMOTE_KNIGHT));
            moves.push(Move::new(from, to, flags::PROMOTE_BISHOP));
            moves.push(Move::new(from, to, flags::PROMOTE_ROOK));
        }
    }

    if gen_type != GenType::Quiets {
        // =========================
        // Captures (left)
        // =========================
        let capture_left = (pawns << 7) & enemy & !0x8080808080808080;

        let mut bb = capture_left & !rank8;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            moves.push(Move::new(to - 7, to, flags::CAPTURE));
        }

        let mut bb = capture_left & rank8;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            let from = to - 7;
            moves.push(Move::new(from, to, flags::PROMOTE_KNIGHT_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_BISHOP_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_ROOK_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_QUEEN_CAPTURE));
        }

        // =========================
        // Captures (right)
        // =========================
        let capture_right = (pawns << 9) & enemy & !0x0101010101010101;

        let mut bb = capture_right & !rank8;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            moves.push(Move::new(to - 9, to, flags::CAPTURE));
        }

        let mut bb = capture_right & rank8;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            let from = to - 9;
            moves.push(Move::new(from, to, flags::PROMOTE_KNIGHT_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_BISHOP_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_ROOK_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_QUEEN_CAPTURE));
        }

        // =========================
        // En Passant
        // =========================
        if let Some(to) = board.en_passant_square {
            let bit = bit(to);
            let from_left = bit >> 7;
            if (from_left & pawns & !0x0101010101010101) != 0 {
                moves.push(Move::new(to - 7, to, flags::EN_PASSANT));
            }

            let from_right = bit >> 9;
            if (from_right & pawns & !0x8080808080808080) != 0 {
                moves.push(Move::new(to - 9, to, flags::EN_PASSANT));
            }
        }
    }
}

/// =======================
/// BLACK PAWNS
/// =======================
fn generate_black_pawns(board: &Board, moves: &mut MoveList, gen_type: GenType) {
    let pawns = board.pawns[Color::Black.idx()];
    let empty = !board.all_occupancy;
    let enemy = board.occupancy[Color::White.idx()];

    let rank1 = 0x00000000000000FF;
    let rank7 = 0x00FF000000000000;

    let single_push = (pawns >> 8) & empty;

    if gen_type != GenType::Captures {
        // =========================
        // Single pushes
        // =========================
        let mut bb = single_push & !rank1;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            moves.push(Move::new(to + 8, to, flags::QUIET));
        }

        // =========================
        // Double pushes (from rank 7)
        // =========================
        let double_push = ((pawns & rank7) >> 16) & empty & (empty >> 8);

        let mut bb = double_push;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            moves.push(Move::new(to + 16, to, flags::DOUBLE_PAWN));
        }
    }

    let mut bb = single_push & rank1;
    while bb != 0 {
        let to = pop_lsb(&mut bb);
        let from = to + 8;
        moves.push(Move::new(from, to, flags::PROMOTE_QUEEN));
        if gen_type != GenType::Captures {
            moves.push(Move::new(from, to, flags::PROMOTE_KNIGHT));
            moves.push(Move::new(from, to, flags::PROMOTE_BISHOP));
            moves.push(Move::new(from, to, flags::PROMOTE_ROOK));
        }
    }

    if gen_type != GenType::Quiets {
        // =========================
        // Captures (left)
        // =========================
        let capture_left = (pawns >> 9) & enemy & !0x8080808080808080;

        let mut bb = capture_left & !rank1;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            moves.push(Move::new(to + 9, to, flags::CAPTURE));
        }

        let mut bb = capture_left & rank1;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            let from = to + 9;
            moves.push(Move::new(from, to, flags::PROMOTE_KNIGHT_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_BISHOP_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_ROOK_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_QUEEN_CAPTURE));
        }

        // =========================
        // Captures (right)
        // =========================
        let capture_right = (pawns >> 7) & enemy & !0x0101010101010101;

        let mut bb = capture_right & !rank1;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            moves.push(Move::new(to + 7, to, flags::CAPTURE));
        }

        let mut bb = capture_right & rank1;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            let from = to + 7;
            moves.push(Move::new(from, to, flags::PROMOTE_KNIGHT_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_BISHOP_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_ROOK_CAPTURE));
            moves.push(Move::new(from, to, flags::PROMOTE_QUEEN_CAPTURE));
        }

        // =========================
        // En Passant
        // =========================
        if let Some(to) = board.en_passant_square {
            let bit = bit(to);

            let from_left = bit << 9;
            if (from_left & pawns & !0x0101010101010101) != 0 {
                moves.push(Move::new(to + 9, to, flags::EN_PASSANT));
            }

            let from_right = bit << 7;
            if (from_right & pawns & !0x8080808080808080) != 0 {
                moves.push(Move::new(to + 7, to, flags::EN_PASSANT));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;

    #[test]
    fn test_white_pawn_moves_startpos() {
        let board = Board::startpos();
        let mut moves = MoveList::new();

        generate_pawn_moves(&board, &mut moves, GenType::All);

        // In starting position:
        // 8 single pushes + 8 double pushes = 16 moves
        assert_eq!(moves.len(), 16);
    }

    #[test]
    fn test_black_pawn_moves_startpos() {
        let mut board = Board::startpos();
        board.side_to_move = Color::Black;

        let mut moves = MoveList::new();
        generate_pawn_moves(&board, &mut moves, GenType::All);

        assert_eq!(moves.len(), 16);
    }
}