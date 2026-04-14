use crate::board::bitboard::*;
use crate::board::board::{Board, castling};
use crate::board::piece::Color;
use crate::board::r#move::{Move, flags};

static mut KING_ATTACKS: [u64; 64] = [0; 64];

pub fn init_king_attacks() {
    for sq in 0..64 {
        unsafe {
            KING_ATTACKS[sq] = mask_king_attacks(sq as u8);
        }
    }
}

fn mask_king_attacks(square: u8) -> u64 {
    let bb = bit(square);
    let mut attacks = 0u64;

    let not_a_file = 0xfefefefefefefefe;
    let not_h_file = 0x7f7f7f7f7f7f7f7f;

    attacks |= bb << 8;
    attacks |= bb >> 8;
    attacks |= (bb << 1) & not_a_file;
    attacks |= (bb >> 1) & not_h_file;
    attacks |= (bb << 7) & not_h_file;
    attacks |= (bb << 9) & not_a_file;
    attacks |= (bb >> 7) & not_a_file;
    attacks |= (bb >> 9) & not_h_file;

    attacks
}

#[inline(always)]
pub fn get_king_attacks(square: u8) -> u64 {
    unsafe { KING_ATTACKS[square as usize] }
}

pub fn generate_king_moves(board: &Board, moves: &mut Vec<Move>) {
    let side = board.side_to_move;
    let own_occ = board.occupancy[side.idx()];
    let enemy_occ = board.occupancy[side.opposite().idx()];

    let from = board.kings[side.idx()].trailing_zeros() as u8;
    if from >= 64 { return; } // Should not happen in legal board

    let attacks = get_king_attacks(from);
    let mut bb = attacks & !own_occ;

    while bb != 0 {
        let to = pop_lsb(&mut bb);
        let flag = if (bit(to) & enemy_occ) != 0 { flags::CAPTURE } else { flags::QUIET };
        moves.push(Move::new(from, to, flag));
    }

    // Castling
    generate_castling_moves(board, moves);
}

fn generate_castling_moves(board: &Board, moves: &mut Vec<Move>) {
    let side = board.side_to_move;
    let occ = board.all_occupancy;

    if side == Color::White {
        // Kingside
        if (board.castling_rights & castling::WHITE_KING) != 0 {
            if (occ & (bit(5) | bit(6))) == 0 {
                // Check if squares are attacked
                if !board.is_square_attacked(4, Color::Black) &&
                   !board.is_square_attacked(5, Color::Black) &&
                   !board.is_square_attacked(6, Color::Black) {
                    moves.push(Move::new(4, 6, flags::KING_CASTLE));
                }
            }
        }
        // Queenside
        if (board.castling_rights & castling::WHITE_QUEEN) != 0 {
            if (occ & (bit(1) | bit(2) | bit(3))) == 0 {
                if !board.is_square_attacked(4, Color::Black) &&
                   !board.is_square_attacked(3, Color::Black) &&
                   !board.is_square_attacked(2, Color::Black) {
                    moves.push(Move::new(4, 2, flags::QUEEN_CASTLE));
                }
            }
        }
    } else {
        // Black Kingside
        if (board.castling_rights & castling::BLACK_KING) != 0 {
            if (occ & (bit(61) | bit(62))) == 0 {
                if !board.is_square_attacked(60, Color::White) &&
                   !board.is_square_attacked(61, Color::White) &&
                   !board.is_square_attacked(62, Color::White) {
                    moves.push(Move::new(60, 62, flags::KING_CASTLE));
                }
            }
        }
        // Black Queenside
        if (board.castling_rights & castling::BLACK_QUEEN) != 0 {
            if (occ & (bit(57) | bit(58) | bit(59))) == 0 {
                if !board.is_square_attacked(60, Color::White) &&
                   !board.is_square_attacked(59, Color::White) &&
                   !board.is_square_attacked(58, Color::White) {
                    moves.push(Move::new(60, 58, flags::QUEEN_CASTLE));
                }
            }
        }
    }
}
