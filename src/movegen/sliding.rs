use crate::board::bitboard::*;
use crate::board::board::Board;
use crate::board::r#move::{Move, flags};

pub fn get_bishop_attacks(sq: u8, occ: u64) -> u64 {
    let mut attacks = 0u64;
    let r = (sq / 8) as i8;
    let c = (sq % 8) as i8;

    // Directions: (dr, dc)
    let dirs = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
    for &(dr, dc) in &dirs {
        let mut nr = r + dr;
        let mut nc = c + dc;
        while nr >= 0 && nr < 8 && nc >= 0 && nc < 8 {
            let nsq = (nr * 8 + nc) as u8;
            attacks |= bit(nsq);
            if (bit(nsq) & occ) != 0 {
                break;
            }
            nr += dr;
            nc += dc;
        }
    }
    attacks
}

pub fn get_rook_attacks(sq: u8, occ: u64) -> u64 {
    let mut attacks = 0u64;
    let r = (sq / 8) as i8;
    let c = (sq % 8) as i8;

    let dirs = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    for &(dr, dc) in &dirs {
        let mut nr = r + dr;
        let mut nc = c + dc;
        while nr >= 0 && nr < 8 && nc >= 0 && nc < 8 {
            let nsq = (nr * 8 + nc) as u8;
            attacks |= bit(nsq);
            if (bit(nsq) & occ) != 0 {
                break;
            }
            nr += dr;
            nc += dc;
        }
    }
    attacks
}

pub fn get_queen_attacks(sq: u8, occ: u64) -> u64 {
    get_bishop_attacks(sq, occ) | get_rook_attacks(sq, occ)
}

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
