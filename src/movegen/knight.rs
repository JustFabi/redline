// Knight move generation using precomputed attack masks.

use crate::board::bitboard::*;
use crate::board::board::Board;
use crate::board::r#move::{Move, flags};
use crate::movegen::move_list::MoveList;

/// Precomputed knight attack table
/// Each index = square
/// Value = bitboard of all attacked squares
static mut KNIGHT_ATTACKS: [u64; 64] = [0; 64];

/// Initialize knight attack table (call once at startup)
pub fn init_knight_attacks() {
    for sq in 0..64 {
        unsafe {
            KNIGHT_ATTACKS[sq] = mask_knight_attacks(sq as u8);
        }
    }
}

/// Generate knight attack mask for a single square
///
/// This computes all valid knight jumps from a square
fn mask_knight_attacks(square: u8) -> u64 {
    let bb = bit(square);
    let mut attacks = 0u64;

    // These masks prevent wrapping around the board
    let not_a_file = 0xfefefefefefefefe;
    let not_h_file = 0x7f7f7f7f7f7f7f7f;
    let not_ab_file = 0xfcfcfcfcfcfcfcfc;
    let not_gh_file = 0x3f3f3f3f3f3f3f3f;

    // Knight moves (bit shifts)
    attacks |= (bb << 17) & not_a_file;
    attacks |= (bb << 15) & not_h_file;
    attacks |= (bb << 10) & not_ab_file;
    attacks |= (bb << 6)  & not_gh_file;

    attacks |= (bb >> 17) & not_h_file;
    attacks |= (bb >> 15) & not_a_file;
    attacks |= (bb >> 10) & not_gh_file;
    attacks |= (bb >> 6)  & not_ab_file;

    attacks
}

/// Get knight attacks for a square
#[inline(always)]
pub fn get_knight_attacks(square: u8) -> u64 {
    unsafe { KNIGHT_ATTACKS[square as usize] }
}

/// Generate all knight moves for current side
pub fn generate_knight_moves(board: &Board, moves: &mut MoveList, captures_only: bool) {
    let side = board.side_to_move;
    let own_occ = board.occupancy[side.idx()];
    let enemy_occ = board.occupancy[side.opposite().idx()];

    let mut knights = board.knights[side.idx()];

    // Loop through all knights using bit tricks
    while knights != 0 {
        let from = pop_lsb(&mut knights);

        // Get all attack squares for this knight
        let attacks = get_knight_attacks(from);

        // Remove squares occupied by own pieces
        let mut bb = attacks & !own_occ;

        if captures_only {
            bb &= enemy_occ;
        }

        while bb != 0 {
            let to = pop_lsb(&mut bb);

            // Determine if capture or quiet move
            let flag = if (bit(to) & enemy_occ) != 0 {
                flags::CAPTURE
            } else {
                flags::QUIET
            };

            moves.push(Move::new(from, to, flag));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;

    #[test]
    fn test_knight_attacks_center() {
        init_knight_attacks();

        let attacks = get_knight_attacks(27); // d4
        assert_eq!(attacks.count_ones(), 8);
    }

    #[test]
    fn test_knight_moves_startpos() {
        init_knight_attacks();

        let board = Board::startpos();
        let mut moves = MoveList::new();

        generate_knight_moves(&board, &mut moves, false);

        // In starting position: 2 knights → 4 moves total
        assert_eq!(moves.len(), 4);
    }
}