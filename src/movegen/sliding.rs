use crate::board::bitboard::*;
use crate::board::board::Board;
use crate::board::r#move::{Move, flags};

pub use crate::magic::{get_bishop_attacks, get_rook_attacks, get_queen_attacks};

pub fn generate_sliding_moves(board: &Board, moves: &mut Vec<Move>) {
    let side = board.side_to_move;
    let own_occ = board.occupancy[side.idx()];
    let enemy_occ = board.occupancy[side.opposite().idx()];
    let occ = board.all_occupancy;

    // Bishops
    let mut bishops = board.bishops[side.idx()];
    while bishops != 0 {
        let from = pop_lsb(&mut bishops);
        let attacks = get_bishop_attacks(from, occ) & !own_occ;
        let mut bb = attacks;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            let flag = if (bit(to) & enemy_occ) != 0 { flags::CAPTURE } else { flags::QUIET };
            moves.push(Move::new(from, to, flag));
        }
    }

    // Rooks
    let mut rooks = board.rooks[side.idx()];
    while rooks != 0 {
        let from = pop_lsb(&mut rooks);
        let attacks = get_rook_attacks(from, occ) & !own_occ;
        let mut bb = attacks;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            let flag = if (bit(to) & enemy_occ) != 0 { flags::CAPTURE } else { flags::QUIET };
            moves.push(Move::new(from, to, flag));
        }
    }

    // Queens
    let mut queens = board.queens[side.idx()];
    while queens != 0 {
        let from = pop_lsb(&mut queens);
        let attacks = get_queen_attacks(from, occ) & !own_occ;
        let mut bb = attacks;
        while bb != 0 {
            let to = pop_lsb(&mut bb);
            let flag = if (bit(to) & enemy_occ) != 0 { flags::CAPTURE } else { flags::QUIET };
            moves.push(Move::new(from, to, flag));
        }
    }
}