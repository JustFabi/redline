use crate::board::board::Board;
use crate::board::piece::Color;
use crate::board::bitboard::{count_bits, bit, pop_lsb};
use crate::movegen::knight::get_knight_attacks;
use crate::movegen::pawn::get_pawn_attacks;

const KING_SAFETY_TABLE: [i32; 32] = [
    0,  0,   1,   2,   3,   5,   7,   9,  12,  15,  18,  22,  26,  30,  35,  40,
    45, 50,  55,  60,  65,  70,  75,  80,  85,  90,  95, 100, 105, 110, 115, 120
];
const GAME_PHASE_INC: [i32; 6] = [0, 1, 1, 2, 4, 0]; // Pawn, Knight, Bishop, Rook, Queen, King

static mut KING_ZONE_MASKS: [u64; 64] = [0; 64];
static mut PASSED_PAWN_MASKS: [[u64; 64]; 2] = [[0; 64]; 2];
static mut ADJACENT_FILES_MASKS: [u64; 8] = [0; 8];

pub fn init_eval() {
    unsafe {
        for sq in 0..64 {
            KING_ZONE_MASKS[sq as usize] = generate_king_zone(sq);
            PASSED_PAWN_MASKS[0][sq as usize] = generate_passed_pawn_mask(Color::White, sq);
            PASSED_PAWN_MASKS[1][sq as usize] = generate_passed_pawn_mask(Color::Black, sq);
        }
        for file in 0..8 {
            ADJACENT_FILES_MASKS[file as usize] = generate_adjacent_files_mask(file);
        }
    }
}

#[derive(Clone, Copy)]
pub struct PawnEntry {
    pub key: u64,
    pub score: i32,
}

pub struct PawnTable {
    pub table: Vec<PawnEntry>,
    pub mask: usize,
}

impl PawnTable {
    pub fn new(size: usize) -> Self {
        let size = size.next_power_of_two();
        Self {
            table: vec![PawnEntry { key: 0, score: 0 }; size],
            mask: size - 1,
        }
    }

    pub fn probe(&self, key: u64) -> Option<i32> {
        let entry = &self.table[(key as usize) & self.mask];
        if entry.key == key {
            Some(entry.score)
        } else {
            None
        }
    }

    pub fn store(&mut self, key: u64, score: i32) {
        let entry = &mut self.table[(key as usize) & self.mask];
        entry.key = key;
        entry.score = score;
    }
}

pub fn evaluate(board: &Board, pawn_table: Option<&mut PawnTable>) -> i32 {
    let midgame = board.mg_pst;
    let endgame = board.eg_pst;
    let mut phase = 0;

    // Phase calculation
    phase += count_bits(board.knights[0] | board.knights[1]) as i32 * GAME_PHASE_INC[1];
    phase += count_bits(board.bishops[0] | board.bishops[1]) as i32 * GAME_PHASE_INC[2];
    phase += count_bits(board.rooks[0] | board.rooks[1]) as i32 * GAME_PHASE_INC[3];
    phase += count_bits(board.queens[0] | board.queens[1]) as i32 * GAME_PHASE_INC[4];

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
        let king_ring = unsafe { KING_ZONE_MASKS[king_sq as usize] };

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
    let pawn_score = if let Some(pt) = pawn_table {
        if let Some(s) = pt.probe(board.pawn_hash) {
            s
        } else {
            let s = evaluate_pawns(board);
            pt.store(board.pawn_hash, s);
            s
        }
    } else {
        evaluate_pawns(board)
    };

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

    let score = pst_score + king_safety_total + pawn_score + mobility_score + bishop_pair;

    if board.side_to_move == Color::White { score } else { -score }
}

fn evaluate_pawns(board: &Board) -> i32 {
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
            let ahead_mask = unsafe { PASSED_PAWN_MASKS[us][sq as usize] };
            if (their_pawns & ahead_mask) == 0 {
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
            let adj_mask = unsafe { ADJACENT_FILES_MASKS[file as usize] };
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
    pawn_bonus[0] - pawn_bonus[1]
}

fn generate_king_zone(king_sq: u8) -> u64 {
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

fn generate_passed_pawn_mask(color: Color, sq: u8) -> u64 {
    let file = sq % 8;
    let rank = sq / 8;
    let mut mask = 0u64;

    for f in (file.saturating_sub(1))..=(file + 1).min(7) {
        for r in 0..8u8 {
            let ahead = if color == Color::White { r > rank } else { r < rank };
            if ahead {
                mask |= bit(r * 8 + f);
            }
        }
    }
    mask
}

fn generate_adjacent_files_mask(file: u8) -> u64 {
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
        init_eval();
        let board = Board::startpos();
        assert_eq!(evaluate(&board, None), 0);
    }

    #[test]
    fn test_material_advantage() {
        init_eval();
        let mut board = Board::startpos();
        board.remove_piece(52, crate::board::piece::PieceType::Pawn, crate::board::piece::Color::Black);
        let score = evaluate(&board, None);
        assert!(score > 0);
    }

    #[test]
    fn test_from_fen_vs_startpos_pst() {
        init_eval();
        crate::movegen::init_all();
        let startpos = Board::startpos();
        let from_fen = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        
        eprintln!("startpos  mg_pst={} eg_pst={}", startpos.mg_pst, startpos.eg_pst);
        eprintln!("from_fen  mg_pst={} eg_pst={}", from_fen.mg_pst, from_fen.eg_pst);
        
        assert_eq!(startpos.mg_pst, from_fen.mg_pst, "mg_pst mismatch between startpos and from_fen");
        assert_eq!(startpos.eg_pst, from_fen.eg_pst, "eg_pst mismatch between startpos and from_fen");
        
        let eval_startpos = evaluate(&startpos, None);
        let eval_from_fen = evaluate(&from_fen, None);
        eprintln!("eval startpos={} from_fen={}", eval_startpos, eval_from_fen);
        assert_eq!(eval_startpos, eval_from_fen, "eval mismatch");
    }

    #[test]
    fn test_from_fen_eval_reasonable() {
        init_eval();
        crate::movegen::init_all();
        // After 1.d4 - White should have a small advantage
        let board = Board::from_fen("rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq d3 0 1").unwrap();
        let score = evaluate(&board, None);
        eprintln!("After 1.d4 (Black to move): score = {} (mg_pst={}, eg_pst={})", score, board.mg_pst, board.eg_pst);
        // Score is from side-to-move perspective. Black to move, so negative = White advantage.
        assert!(score.abs() < 100, "Score {} is unreasonable for 1.d4", score);
    }

    #[test]
    fn test_pst_consistency_after_moves() {
        init_eval();
        crate::movegen::init_all();
        // Start from FEN, play some moves, verify PST is consistent
        let mut board = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        
        // Record initial eval
        let initial_mg = board.mg_pst;
        let initial_eg = board.eg_pst;
        eprintln!("Initial: mg_pst={}, eg_pst={}", initial_mg, initial_eg);
        
        // Play e2e4
        let m = board.parse_move("e2e4").unwrap();
        let state = board.make_move(m);
        eprintln!("After e2e4: mg_pst={}, eg_pst={}", board.mg_pst, board.eg_pst);
        
        // Unmake and verify we get back to initial
        board.unmake_move(m, state);
        assert_eq!(board.mg_pst, initial_mg, "mg_pst not restored after unmake");
        assert_eq!(board.eg_pst, initial_eg, "eg_pst not restored after unmake");
    }
}
