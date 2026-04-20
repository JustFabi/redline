use crate::board::board::Board;
use crate::board::piece::Color;
use crate::board::bitboard::{count_bits, bit, pop_lsb};
use crate::movegen::knight::get_knight_attacks;
use crate::movegen::pawn::get_pawn_attacks;

// PeSTO Base Piece Values (Midgame, Endgame)
pub const MG_VALUE: [i32; 6] = [82, 337, 365, 477, 1025, 0];
pub const EG_VALUE: [i32; 6] = [94, 281, 297, 512, 936, 0];

// PeSTO Piece-Square Tables
#[rustfmt::skip]
pub const MG_PAWN_TABLE: [i32; 64] = [
      0,   0,   0,   0,   0,   0,   0,   0,
     98, 134,  61,  95,  68, 126,  34, -11,
     -6,   7,  26,  31,  65,  56,  25, -20,
    -14,  13,   6,  21,  23,  12,  17, -23,
    -27,  -2,  -5,  12,  17,   6,  10, -25,
    -26,  -4,  -4, -10,   3,   3,  33, -12,
    -35,  -1, -20, -23, -15,  24,  38, -22,
      0,   0,   0,   0,   0,   0,   0,   0,
];

#[rustfmt::skip]
pub const EG_PAWN_TABLE: [i32; 64] = [
      0,   0,   0,   0,   0,   0,   0,   0,
    178, 173, 158, 134, 147, 132, 165, 187,
     94, 100,  85,  67,  56,  53,  82,  84,
     32,  24,  13,   5,  -2,   4,  17,  17,
     13,   9,  -3,  -7,  -7,  -8,   3,  -1,
      4,   7,  -6,   1,   0,  -5,  -1,  -8,
     13,   8,   8,  10,  13,   0,   2,  -7,
      0,   0,   0,   0,   0,   0,   0,   0,
];

#[rustfmt::skip]
pub const MG_KNIGHT_TABLE: [i32; 64] = [
    -167, -89, -34, -49,  61, -97, -15, -107,
     -73, -41,  72,  36,  23,  62,   7,  -17,
     -47,  60,  37,  65,  84, 129,  73,   44,
      -9,  17,  19,  53,  37,  69,  18,   22,
     -13,   4,  16,  13,  28,  19,  21,   -8,
     -23,  -9,  12,  10,  19,  17,  25,  -16,
     -29, -53, -12,  -3,  -1,  18, -14,  -19,
    -105, -21, -58, -33, -17, -28, -19,  -23,
];

#[rustfmt::skip]
pub const EG_KNIGHT_TABLE: [i32; 64] = [
     -58, -38, -13, -28, -31, -27, -63, -99,
     -25,  -8, -25,  -2,  -9, -25, -24, -52,
     -24, -20,  10,   9,  -1,  -9, -19, -41,
     -17,   3,  22,  22,  22,  11,   8, -18,
     -18,  -6,  16,  25,  16,  17,   4, -18,
     -23,  -3,  -1,  15,  10,  -3, -20, -22,
     -42, -20, -10,  -5,  -2, -20, -23, -44,
     -29, -51, -23, -15, -22, -18, -50, -64,
];

#[rustfmt::skip]
pub const MG_BISHOP_TABLE: [i32; 64] = [
     -29,   4, -82, -37, -25, -42,   7,  -8,
     -26,  16, -18, -13,  30,  59,  18, -47,
     -16,  37,  43,  40,  35,  50,  37,  -2,
      -4,   5,  19,  50,  37,  37,   7,  -2,
      -6,  13,  13,  26,  34,  12,  10,   4,
       0,  15,  15,  15,  14,  27,  18,  10,
       4,  15,  16,   0,   7,  21,  33,   1,
     -33,  -3, -14, -21, -13, -12, -39, -21,
];

#[rustfmt::skip]
pub const EG_BISHOP_TABLE: [i32; 64] = [
     -14, -21, -11,  -8,  -7,  -9, -17, -24,
      -8,  -4,   7, -12,  -3, -13,  -4, -14,
       2,  -8,   0,  -1,  -2,   6,   0,   4,
      -3,   9,  12,   9,  14,  10,   3,   2,
      -6,   3,  13,  19,   7,  10,  -3,  -9,
     -12,  -3,   8,  10,  13,   3,  -7, -15,
     -14, -18,  -7,  -1,   4,  -9, -15, -27,
     -23,  -9, -23,  -5,  -9, -16,  -5, -17,
];

#[rustfmt::skip]
pub const MG_ROOK_TABLE: [i32; 64] = [
      32,  42,  32,  51,  63,   9,  31,  43,
      27,  32,  58,  62,  80,  67,  26,  44,
      -5,  19,  26,  36,  17,  45,  61,  16,
     -24, -11,   7,  26,  24,  35,  -8, -20,
     -36, -26, -12,  -1,   9,  -7,   6, -23,
     -45, -25, -16, -17,   3,   0,  -5, -33,
     -44, -16, -20,  -9,  -1,  11,  -6, -71,
     -19, -13,   1,  17,  16,   7, -37, -26,
];

#[rustfmt::skip]
pub const EG_ROOK_TABLE: [i32; 64] = [
      13,  10,  18,  15,  12,  12,   8,   5,
      11,  13,  13,  11,  -3,   3,   8,   3,
       7,   7,   7,   5,   4,  -3,  -5,  -3,
       4,   3,  13,   1,   2,   1,  -1,   2,
       3,   5,   8,   4,  -5,  -6,  -8, -11,
      -4,   0,  -5,  -1,  -7, -12,  -8, -16,
      -6,  -6,   0,   2,  -9,  -9, -11,  -3,
      -9,   2,   3,  -1,  -5, -13,   4, -20,
];

#[rustfmt::skip]
pub const MG_QUEEN_TABLE: [i32; 64] = [
     -28,   0,  29,  12,  59,  44,  43,  45,
     -24, -39,  -5,   1, -16,  57,  28,  54,
     -13, -17,   7,   8,  29,  56,  47,  57,
     -27, -27, -16, -16,  -1,  17,  -2,   1,
      -9, -26,  -9, -10,  -2,  -4,   3,  -3,
     -14,   2, -11,  -2,  -5,   2,  14,   5,
     -35,  -8,  11,   2,   8,  15,  -3,   1,
      -1, -18,  -9,  10, -15, -25, -31, -50,
];

#[rustfmt::skip]
pub const EG_QUEEN_TABLE: [i32; 64] = [
      -9,  22,  22,  27,  27,  19,  10,  20,
     -17,  20,  32,  41,  58,  25,  30,   0,
     -20,   6,   9,  49,  47,  35,  19,   9,
       3,  22,  24,  45,  57,  40,  57,  36,
     -18,  28,  19,  47,  31,  34,  39,  23,
     -16, -27,  15,   6,   9,  17,  10,   5,
     -22, -23, -30, -16, -16, -23, -36, -32,
     -33, -28, -22, -43,  -5, -32, -20, -41,
];

#[rustfmt::skip]
pub const MG_KING_TABLE: [i32; 64] = [
     -65,  23,  16, -15, -56, -34,   2,  13,
      29,  -1, -20,  -7,  -8,  -4, -38, -29,
      -9,  24,   2, -16, -20,   6,  22, -22,
     -17, -20, -12, -27, -30, -25, -14, -36,
     -49,  -1, -27, -39, -46, -44, -33, -51,
     -14, -14, -22, -46, -44, -30, -15, -27,
       1,   7,  -8, -64, -43, -16,   9,   8,
     -15,  36,  12, -54,   8, -28,  24,  14,
];

#[rustfmt::skip]
pub const EG_KING_TABLE: [i32; 64] = [
     -74, -35, -18, -18, -11,  15,   4, -17,
     -12,  17,  14,  17,  17,  38,  23,  11,
      10,  17,  23,  15,  20,  45,  44,  13,
      -8,  22,  24,  27,  26,  33,  26,   3,
     -18,  -4,  21,  24,  27,  23,   9, -11,
     -19,  -3,  11,  21,  23,  16,   7,  -9,
     -27, -11,   4,  13,  14,   4,  -5, -17,
     -53, -34, -21, -11, -28, -14, -24, -43,
];

const GAME_PHASE_INC: [i32; 6] = [0, 1, 1, 2, 4, 0]; // Pawn, Knight, Bishop, Rook, Queen, King

pub fn evaluate(board: &Board) -> i32 {
    let mut midgame = 0;
    let mut endgame = 0;
    let mut phase = 0;

    // Material and PST for both sides
    for color in [Color::White, Color::Black] {
        let side = color.idx();
        let multiplier = if color == Color::White { 1 } else { -1 };

        let w_pawns = board.pawns[side];
        let w_knights = board.knights[side];
        let w_bishops = board.bishops[side];
        let w_rooks = board.rooks[side];
        let w_queens = board.queens[side];
        let w_kings = board.kings[side];

        phase += count_bits(w_knights) as i32 * GAME_PHASE_INC[1];
        phase += count_bits(w_bishops) as i32 * GAME_PHASE_INC[2];
        phase += count_bits(w_rooks) as i32 * GAME_PHASE_INC[3];
        phase += count_bits(w_queens) as i32 * GAME_PHASE_INC[4];

        let mut eval_pieces = |mut bb: u64, pt: usize, mg_pst: &[i32; 64], eg_pst: &[i32; 64]| {
            let mg_val = MG_VALUE[pt];
            let eg_val = EG_VALUE[pt];
            while bb != 0 {
                let sq = crate::board::bitboard::pop_lsb(&mut bb);
                let pst_sq = if color == Color::White { ((7 - (sq / 8)) * 8 + (sq % 8)) as usize } else { sq as usize };
                midgame += multiplier * (mg_val + mg_pst[pst_sq]);
                endgame += multiplier * (eg_val + eg_pst[pst_sq]);
            }
        };

        eval_pieces(w_pawns, 0, &MG_PAWN_TABLE, &EG_PAWN_TABLE);
        eval_pieces(w_knights, 1, &MG_KNIGHT_TABLE, &EG_KNIGHT_TABLE);
        eval_pieces(w_bishops, 2, &MG_BISHOP_TABLE, &EG_BISHOP_TABLE);
        eval_pieces(w_rooks, 3, &MG_ROOK_TABLE, &EG_ROOK_TABLE);
        eval_pieces(w_queens, 4, &MG_QUEEN_TABLE, &EG_QUEEN_TABLE);
        eval_pieces(w_kings, 5, &MG_KING_TABLE, &EG_KING_TABLE);
    }

    let phase = phase.min(24);
    let phase_weight = (phase * 256 + 12) / 24;

    // ===== King Safety =====
    let mut king_safety = [0i32; 2];
    for color in [Color::White, Color::Black] {
        let us = color.idx();
        let them = color.opposite().idx();
        let king_bb = board.kings[us];
        if king_bb == 0 { continue; }
        let king_sq = king_bb.trailing_zeros() as u8;
        let king_ring = king_zone(king_sq);

        let mut attackers = 0i32;
        let mut attack_weight = 0i32;

        // Enemy knights attacking king ring
        let mut knights = board.knights[them];
        while knights != 0 {
            let sq = pop_lsb(&mut knights);
            let attacks = get_knight_attacks(sq);
            if (attacks & king_ring) != 0 {
                attackers += 1;
                attack_weight += 40;
            }
        }

        // Enemy bishops/queens on diagonals near king
        let bishop_queen = board.bishops[them] | board.queens[them];
        if (bishop_queen & king_ring) != 0 {
            attackers += 1;
            attack_weight += 60;
        }

        // Enemy rooks/queens on files near king
        let rook_queen = board.rooks[them] | board.queens[them];
        let king_file_mask = 0x0101010101010101u64 << (king_sq % 8);
        if (rook_queen & king_file_mask) != 0 {
            attackers += 1;
            attack_weight += 40;
        }

        // Enemy pawns attacking king ring
        let mut enemy_pawns = board.pawns[them];
        while enemy_pawns != 0 {
            let sq = pop_lsb(&mut enemy_pawns);
            let atk = get_pawn_attacks(sq, color.opposite());
            if (atk & king_ring) != 0 {
                attackers += 1;
                attack_weight += 20;
            }
        }

        // Scale by attacker count: more attackers = disproportionately more danger
        let safety_penalty = match attackers {
            0 => 0,
            1 => attack_weight / 4,
            2 => attack_weight / 2,
            _ => attack_weight,
        };
        king_safety[us] = -safety_penalty;
    }

    // ===== Pawn Structure =====
    let mut pawn_bonus = [0i32; 2];
    for color in [Color::White, Color::Black] {
        let us = color.idx();
        let them = color.opposite().idx();
        let our_pawns = board.pawns[us];
        let their_pawns = board.pawns[them];

        let mut pawns = our_pawns;
        while pawns != 0 {
            let sq = pop_lsb(&mut pawns);
            let file = sq % 8;
            let rank = sq / 8;

            // Passed pawn: no enemy pawns on same or adjacent files ahead
            let ahead_mask = passed_pawn_mask(color, sq);
            if (their_pawns & ahead_mask) == 0 {
                // Bonus scales with how far advanced
                let advance = if color == Color::White { rank } else { 7 - rank };
                let passed_bonus = match advance {
                    6 => 120,
                    5 => 80,
                    4 => 50,
                    3 => 30,
                    2 => 15,
                    _ => 5,
                };
                pawn_bonus[us] += passed_bonus;
            }

            // Isolated pawn: no friendly pawns on adjacent files
            let adj_mask = adjacent_files_mask(file);
            if (our_pawns & adj_mask) == 0 {
                pawn_bonus[us] -= 15;
            }

            // Doubled pawn: another friendly pawn on the same file
            let file_mask = 0x0101010101010101u64 << file;
            if count_bits(our_pawns & file_mask) > 1 {
                pawn_bonus[us] -= 10;
            }
        }
    }

    // ===== Mobility (simple: count legal knight + bishop moves) =====
    let mut mobility_score = 0i32;
    for color in [Color::White, Color::Black] {
        let us = color.idx();
        let multiplier = if color == Color::White { 1 } else { -1 };
        let own_occ = board.occupancy[us];

        // Knight mobility
        let mut knights = board.knights[us];
        while knights != 0 {
            let sq = pop_lsb(&mut knights);
            let moves = get_knight_attacks(sq) & !own_occ;
            mobility_score += multiplier * (count_bits(moves) as i32 - 4) * 4; // 4cp per move above/below 4
        }

        // Bishop mobility (use magic sliders)
        let mut bishops = board.bishops[us];
        while bishops != 0 {
            let sq = pop_lsb(&mut bishops);
            let moves = crate::magic::get_bishop_attacks(sq, board.all_occupancy) & !own_occ;
            mobility_score += multiplier * (count_bits(moves) as i32 - 7) * 5; // 5cp per move above/below 7
        }

        // Rook mobility
        let mut rooks = board.rooks[us];
        while rooks != 0 {
            let sq = pop_lsb(&mut rooks);
            let moves = crate::magic::get_rook_attacks(sq, board.all_occupancy) & !own_occ;
            mobility_score += multiplier * (count_bits(moves) as i32 - 7) * 3;
        }
    }

    // ===== Bishop Pair Bonus =====
    let mut bishop_pair = 0i32;
    if count_bits(board.bishops[0]) >= 2 { bishop_pair += 30; }
    if count_bits(board.bishops[1]) >= 2 { bishop_pair -= 30; }

    // ===== Combine =====
    let pst_score = ((midgame * phase_weight) + (endgame * (256 - phase_weight))) / 256;

    let ks_white = king_safety[0];
    let ks_black = king_safety[1];
    let king_safety_total = ks_white - ks_black;

    let pawn_total = pawn_bonus[0] as i32 - pawn_bonus[1] as i32;

    let score = pst_score + king_safety_total + pawn_total + mobility_score + bishop_pair;

    if board.side_to_move == Color::White { score } else { -score }
}

/// Generate a king zone mask: the 3x3 area around the king + 3 squares in front
fn king_zone(king_sq: u8) -> u64 {
    let mut zone = 0u64;
    let rank = (king_sq / 8) as i32;
    let file = (king_sq % 8) as i32;
    for dr in -1..=2 {
        for df in -1..=1 {
            let r = rank + dr;
            let f = file + df;
            if r >= 0 && r < 8 && f >= 0 && f < 8 {
                zone |= bit((r * 8 + f) as u8);
            }
        }
    }
    zone
}

/// Generate a passed pawn mask: all squares on same and adjacent files ahead of sq
fn passed_pawn_mask(color: Color, sq: u8) -> u64 {
    let file = sq % 8;
    let rank = sq / 8;
    let mut mask = 0u64;

    let files = if file == 0 { vec![0, 1] }
                else if file == 7 { vec![6, 7] }
                else { vec![file - 1, file, file + 1] };

    for &f in &files {
        for r in 0..8u8 {
            let ahead = if color == Color::White { r > rank } else { r < rank };
            if ahead {
                mask |= bit(r * 8 + f);
            }
        }
    }
    mask
}

/// Mask of all squares on files adjacent to `file` (not including `file` itself)
fn adjacent_files_mask(file: u8) -> u64 {
    let mut mask = 0u64;
    if file > 0 {
        mask |= 0x0101010101010101u64 << (file - 1);
    }
    if file < 7 {
        mask |= 0x0101010101010101u64 << (file + 1);
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;

    #[test]
    fn test_initial_eval() {
        let board = Board::startpos();
        assert_eq!(evaluate(&board), 0);
    }

    #[test]
    fn test_material_advantage() {
        let mut board = Board::startpos();
        board.pawns[1] &= !crate::board::bitboard::bit(52); // e7
        board.update_occupancy();
        let score = evaluate(&board);
        assert!(score > 0);
    }
}
