use crate::board::board::Board;
use crate::board::piece::Color;
use crate::board::bitboard::{count_bits, bit, pop_lsb};
use crate::movegen::knight::get_knight_attacks;
use crate::magic::{get_bishop_attacks, get_rook_attacks};
use std::cmp;

// --- Constants & Tables ---
const GAME_PHASE_INC: [i32; 6] = [0, 1, 1, 2, 4, 0]; // Pawn, Knight, Bishop, Rook, Queen, King

// King Danger table (quadratic scaling)
const KING_DANGER_TABLE: [i32; 32] = [
    0, 0, 1, 2, 4, 7, 11, 16, 22, 29, 37, 46, 56, 67, 79, 92,
    106, 121, 137, 154, 172, 191, 211, 232, 254, 277, 301, 326, 352, 379, 407, 436
];

// Attack weights for king safety
const ATTACK_WEIGHT_KNIGHT: i32 = 2;
const ATTACK_WEIGHT_BISHOP: i32 = 2;
const ATTACK_WEIGHT_ROOK: i32 = 3;
const ATTACK_WEIGHT_QUEEN: i32 = 5;

// Mobility bonuses: (Midgame, Endgame) per safe move
const KNIGHT_MOBILITY: [(i32, i32); 9] = [
    (-15, -15), (-5, -5), (0, 0), (5, 5), (10, 10), (15, 15), (20, 20), (25, 25), (30, 30)
];
const BISHOP_MOBILITY: [(i32, i32); 14] = [
    (-25, -25), (-10, -10), (0, 0), (5, 5), (10, 10), (15, 15), (20, 20), (25, 25),
    (30, 30), (35, 35), (40, 40), (45, 45), (50, 50), (55, 55)
];
const ROOK_MOBILITY: [(i32, i32); 15] = [
    (-20, -20), (-10, -10), (0, 0), (5, 5), (10, 10), (15, 15), (20, 20), (25, 25),
    (30, 30), (35, 35), (40, 40), (45, 45), (50, 50), (55, 55), (60, 60)
];
const QUEEN_MOBILITY: [(i32, i32); 28] = [
    (-20, -20), (-15, -15), (-10, -10), (-5, -5), (0, 0), (2, 2), (4, 4), (6, 6),
    (8, 8), (10, 10), (12, 12), (14, 14), (16, 16), (18, 18), (20, 20), (22, 22),
    (24, 24), (26, 26), (28, 28), (30, 30), (32, 32), (34, 34), (36, 36), (38, 38),
    (40, 40), (42, 42), (44, 44), (46, 46)
];

// File masks
const FILE_MASKS: [u64; 8] = [
    0x0101010101010101, 0x0202020202020202, 0x0404040404040404, 0x0808080808080808,
    0x1010101010101010, 0x2020202020202020, 0x4040404040404040, 0x8080808080808080
];

static mut KING_ZONE_MASKS: [u64; 64] = [0; 64];
static mut KING_ZONE_KNIGHT_ATTACKS: [u64; 64] = [0; 64];
static mut KING_ZONE_BISHOP_RAYS: [u64; 64] = [0; 64];
static mut KING_ZONE_ROOK_RAYS: [u64; 64] = [0; 64];
static mut PASSED_PAWN_MASKS: [[u64; 64]; 2] = [[0; 64]; 2];
static mut ADJACENT_FILES_MASKS: [u64; 8] = [0; 8];
static mut OUTPOST_MASKS: [[u64; 64]; 2] = [[0; 64]; 2];

pub fn init_eval() {
    unsafe {
        for sq in 0..64 {
            let zone = generate_king_zone(sq);
            KING_ZONE_MASKS[sq as usize] = zone;
            let mut knight_attacks = 0;
            for z_sq in 0..64 {
                if (zone & bit(z_sq)) != 0 {
                    knight_attacks |= get_knight_attacks(z_sq);
                }
            }
            KING_ZONE_KNIGHT_ATTACKS[sq as usize] = knight_attacks;
            // Precompute bishop/rook ray projections from the king zone.
            // These are the union of all bishop/rook attacks from every square
            // in the king zone, using an empty board. An enemy slider can attack
            // the king zone only if it sits on one of these rays.
            let mut bishop_rays = 0u64;
            let mut rook_rays = 0u64;
            for z_sq in 0..64u8 {
                if (zone & bit(z_sq)) != 0 {
                    bishop_rays |= get_bishop_attacks(z_sq, 0);
                    rook_rays |= get_rook_attacks(z_sq, 0);
                }
            }
            KING_ZONE_BISHOP_RAYS[sq as usize] = bishop_rays;
            KING_ZONE_ROOK_RAYS[sq as usize] = rook_rays;
            PASSED_PAWN_MASKS[0][sq as usize] = generate_passed_pawn_mask(Color::White, sq);
            PASSED_PAWN_MASKS[1][sq as usize] = generate_passed_pawn_mask(Color::Black, sq);
            OUTPOST_MASKS[0][sq as usize] = generate_outpost_mask(Color::White, sq);
            OUTPOST_MASKS[1][sq as usize] = generate_outpost_mask(Color::Black, sq);
        }
        for file in 0..8 {
            ADJACENT_FILES_MASKS[file as usize] = generate_adjacent_files_mask(file);
        }
    }
}

// Bitboard shifting helpers
fn shift_up(bb: u64) -> u64 { bb << 8 }
fn shift_down(bb: u64) -> u64 { bb >> 8 }
fn shift_up_left(bb: u64) -> u64 { (bb & !FILE_MASKS[0]) << 7 }
fn shift_up_right(bb: u64) -> u64 { (bb & !FILE_MASKS[7]) << 9 }
fn shift_down_left(bb: u64) -> u64 { (bb & !FILE_MASKS[0]) >> 9 }
fn shift_down_right(bb: u64) -> u64 { (bb & !FILE_MASKS[7]) >> 7 }

fn distance(sq1: u8, sq2: u8) -> u8 {
    let r1 = sq1 / 8;
    let f1 = sq1 % 8;
    let r2 = sq2 / 8;
    let f2 = sq2 % 8;
    cmp::max(r1.abs_diff(r2), f1.abs_diff(f2))
}

#[derive(Clone, Copy)]
pub struct PawnEntry {
    pub key: u64,
    pub score: EvalScore,
    pub passed_pawns: [u64; 2],
    pub pawn_attacks: [u64; 2],
}

#[derive(Clone, Copy, Default)]
pub struct EvalScore {
    pub mg: i32,
    pub eg: i32,
}

impl EvalScore {
    fn add(&mut self, mg: i32, eg: i32) {
        self.mg += mg;
        self.eg += eg;
    }
    fn sub(&mut self, mg: i32, eg: i32) {
        self.mg -= mg;
        self.eg -= eg;
    }
}

pub struct PawnTable {
    pub table: Vec<PawnEntry>,
    pub mask: usize,
}

impl PawnTable {
    pub fn new(size: usize) -> Self {
        let size = size.next_power_of_two();
        Self {
            table: vec![PawnEntry { key: 0, score: EvalScore::default(), passed_pawns: [0; 2], pawn_attacks: [0; 2] }; size],
            mask: size - 1,
        }
    }

    pub fn probe(&self, key: u64) -> Option<PawnEntry> {
        let entry = self.table[(key as usize) & self.mask];
        if entry.key == key {
            Some(entry)
        } else {
            None
        }
    }

    pub fn store(&mut self, key: u64, entry: PawnEntry) {
        self.table[(key as usize) & self.mask] = entry;
    }
}

pub fn evaluate(board: &Board, pawn_table: Option<&mut PawnTable>) -> i32 {
    let mut phase = 0;
    phase += count_bits(board.knights[0] | board.knights[1]) as i32 * GAME_PHASE_INC[1];
    phase += count_bits(board.bishops[0] | board.bishops[1]) as i32 * GAME_PHASE_INC[2];
    phase += count_bits(board.rooks[0] | board.rooks[1]) as i32 * GAME_PHASE_INC[3];
    phase += count_bits(board.queens[0] | board.queens[1]) as i32 * GAME_PHASE_INC[4];
    
    let phase = cmp::min(phase, 24);
    let phase_weight = (phase * 256 + 12) / 24;

    let mut score = EvalScore { mg: board.mg_pst, eg: board.eg_pst };
    
    let pawn_entry = evaluate_pawns(board, pawn_table);
    score.add(pawn_entry.score.mg, pawn_entry.score.eg);

    let ks = evaluate_king(board, &pawn_entry);
    score.add(ks.mg, ks.eg);

    let pieces = evaluate_pieces(board, &pawn_entry);
    score.add(pieces.mg, pieces.eg);

    let asym = evaluate_asymmetries(board);
    score.add(asym.mg, asym.eg);

    let space = evaluate_space(board, &pawn_entry, phase);
    score.add(space.mg, space.eg);

    let mut total = ((score.mg * phase_weight) + (score.eg * (256 - phase_weight))) / 256;

    total = evaluate_scale(board, total, phase);

    let tempo = 12;
    if board.side_to_move == Color::White { total + tempo } else { -total + tempo }
}

fn evaluate_space(board: &Board, pawn_entry: &PawnEntry, phase: i32) -> EvalScore {
    let mut score = EvalScore::default();
    
    // Space is only relevant when many pieces are on the board
    if phase < 16 { return score; }

    for color in [Color::White, Color::Black] {
        let us = color.idx();
        let them = color.opposite().idx();
        let mult = if color == Color::White { 1 } else { -1 };

        let space_mask = if color == Color::White {
            0x0000003C3C3C0000u64 // C-F, Ranks 2-4
        } else {
            0x00003C3C3C000000u64 // C-F, Ranks 5-7
        };

        let safe = space_mask & !board.pawns[us] & !pawn_entry.pawn_attacks[them];
        
        let behind = if color == Color::White {
            let p = board.pawns[us];
            p | shift_down(p) | shift_down(shift_down(p)) | shift_down(shift_down(shift_down(p)))
        } else {
            let p = board.pawns[us];
            p | shift_up(p) | shift_up(shift_up(p)) | shift_up(shift_up(shift_up(p)))
        };

        let bonus = count_bits(safe) as i32 + count_bits(behind & safe) as i32;
        let weight = count_bits(board.occupancy[us] & !board.pawns[us] & !board.kings[us]) as i32;
        
        // Stockfish space formula: (bonus * weight * weight) / 16
        let space_score = (bonus * weight * weight) / 16;
        score.add(mult * space_score, 0);
    }
    score
}

fn evaluate_pawns(board: &Board, mut pawn_table: Option<&mut PawnTable>) -> PawnEntry {
    if let Some(pt) = &mut pawn_table {
        if let Some(entry) = pt.probe(board.pawn_hash) {
            return entry;
        }
    }

    let mut entry = PawnEntry { key: board.pawn_hash, score: EvalScore::default(), passed_pawns: [0; 2], pawn_attacks: [0; 2] };

    for color in [Color::White, Color::Black] {
        let us = color.idx();
        let them = color.opposite().idx();
        let our_pawns = board.pawns[us];
        let their_pawns = board.pawns[them];
        
        // Calculate pawn attacks
        let attacks_left = if color == Color::White { shift_up_left(our_pawns) } else { shift_down_right(our_pawns) };
        let attacks_right = if color == Color::White { shift_up_right(our_pawns) } else { shift_down_left(our_pawns) };
        entry.pawn_attacks[us] = attacks_left | attacks_right;

        let mut pawns = our_pawns;
        while pawns != 0 {
            let sq = pop_lsb(&mut pawns);
            let file = sq % 8;
            let rank = sq / 8;
            let relative_rank = if color == Color::White { rank } else { 7 - rank };

            let file_mask = FILE_MASKS[file as usize];
            let adj_mask = unsafe { ADJACENT_FILES_MASKS[file as usize] };

            let is_supported = (entry.pawn_attacks[us] & bit(sq)) != 0;
            if is_supported {
                let bonus = 5 + (relative_rank as i32 * 2);
                if color == Color::White { entry.score.add(bonus, bonus); } else { entry.score.sub(bonus, bonus); }
            }

            let is_isolated = (our_pawns & adj_mask) == 0;
            if is_isolated {
                if color == Color::White { entry.score.sub(15, 20); } else { entry.score.add(15, 20); }
            }

            let is_doubled = count_bits(our_pawns & file_mask) > 1;
            if is_doubled {
                if color == Color::White { entry.score.sub(10, 15); } else { entry.score.add(10, 15); }
            }

            if !is_isolated && !is_supported {
                let stop_sq = if color == Color::White { sq + 8 } else { sq.wrapping_sub(8) };
                let behind_and_same_rank = if color == Color::White {
                    !((0xFFFFFFFFFFFFFFFFu64) << (rank * 8))
                } else {
                    !((0xFFFFFFFFFFFFFFFFu64) >> ((7 - rank) * 8))
                };
                let has_pawn_behind_or_beside = (our_pawns & adj_mask & behind_and_same_rank) != 0;
                
                if !has_pawn_behind_or_beside {
                    let enemy_attacks = if color == Color::White {
                        shift_down_left(their_pawns) | shift_down_right(their_pawns)
                    } else {
                        shift_up_left(their_pawns) | shift_up_right(their_pawns)
                    };
                    if (enemy_attacks & bit(stop_sq)) != 0 {
                        if color == Color::White { entry.score.sub(12, 16); } else { entry.score.add(12, 16); }
                    }
                }
            }

            // Passed pawn
            let ahead_mask = unsafe { PASSED_PAWN_MASKS[us][sq as usize] };
            if (their_pawns & ahead_mask) == 0 {
                entry.passed_pawns[us] |= bit(sq);
                
                let block_sq = if color == Color::White { sq + 8 } else { sq.wrapping_sub(8) };
                let passed_mg = match relative_rank {
                    6 => 80, 5 => 40, 4 => 20, 3 => 10, 2 => 5, _ => 0,
                };
                let mut passed_eg = match relative_rank {
                    6 => 120, 5 => 70, 4 => 35, 3 => 15, 2 => 5, _ => 0,
                };

                // Adjust by King proximity to block square (Stockfish 11 concept)
                if relative_rank >= 4 && board.kings[us] != 0 && board.kings[them] != 0 {
                    let our_king_sq = board.kings[us].trailing_zeros() as u8;
                    let their_king_sq = board.kings[them].trailing_zeros() as u8;
                    let our_dist = distance(our_king_sq, block_sq) as i32;
                    let their_dist = distance(their_king_sq, block_sq) as i32;
                    
                    let weight = (relative_rank as i32 * 5) - 13; // 4->7, 5->12, 6->17
                    let bonus = (their_dist * 4 - our_dist * 2) * weight;
                    passed_eg += bonus;
                }

                if color == Color::White { entry.score.add(passed_mg, passed_eg); } else { entry.score.sub(passed_mg, passed_eg); }
            }
        }
    }

    if let Some(pt) = pawn_table {
        pt.store(board.pawn_hash, entry);
    }
    entry
}

fn evaluate_king(board: &Board, _pawn_entry: &PawnEntry) -> EvalScore {
    let mut score = EvalScore::default();

    for color in [Color::White, Color::Black] {
        let us = color.idx();
        let them = color.opposite().idx();
        let king_bb = board.kings[us];
        if king_bb == 0 { continue; }
        let king_sq = king_bb.trailing_zeros() as u8;
        let king_ring = unsafe { KING_ZONE_MASKS[king_sq as usize] };

        // ===== King Safety (Midgame) =====
        let mut attack_weight = 0;
        let mut attackers_count = 0;

        let enemy_knights = board.knights[them];
        let knight_attackers = enemy_knights & unsafe { KING_ZONE_KNIGHT_ATTACKS[king_sq as usize] };
        let count = count_bits(knight_attackers) as i32;
        if count > 0 {
            attackers_count += count;
            attack_weight += count * ATTACK_WEIGHT_KNIGHT;
        }

        // Bishops: use precomputed ray mask as a fast pre-filter.
        // Only bishops that sit on a ray from the king zone can possibly
        // attack it. We still need per-piece sliding checks because of
        // occupancy blocking, but this reduces the loop body count.
        let bishop_ray_mask = unsafe { KING_ZONE_BISHOP_RAYS[king_sq as usize] };
        let candidate_bishops = board.bishops[them] & bishop_ray_mask;
        let mut bi = candidate_bishops;
        while bi != 0 {
            let sq = pop_lsb(&mut bi);
            if (get_bishop_attacks(sq, board.all_occupancy) & king_ring) != 0 {
                attackers_count += 1;
                attack_weight += ATTACK_WEIGHT_BISHOP;
            }
        }

        // Rooks: same pre-filter approach.
        let rook_ray_mask = unsafe { KING_ZONE_ROOK_RAYS[king_sq as usize] };
        let candidate_rooks = board.rooks[them] & rook_ray_mask;
        let mut ro = candidate_rooks;
        while ro != 0 {
            let sq = pop_lsb(&mut ro);
            if (get_rook_attacks(sq, board.all_occupancy) & king_ring) != 0 {
                attackers_count += 1;
                attack_weight += ATTACK_WEIGHT_ROOK;
            }
        }

        // Queens: can attack on both diagonals and ranks/files.
        let queen_ray_mask = bishop_ray_mask | rook_ray_mask;
        let candidate_queens = board.queens[them] & queen_ray_mask;
        let mut qu = candidate_queens;
        while qu != 0 {
            let sq = pop_lsb(&mut qu);
            let atk = get_bishop_attacks(sq, board.all_occupancy) | get_rook_attacks(sq, board.all_occupancy);
            if (atk & king_ring) != 0 {
                attackers_count += 1;
                attack_weight += ATTACK_WEIGHT_QUEEN;
            }
        }

        let mut king_danger = 0;
        if attackers_count >= 2 {
            let idx = cmp::min(attack_weight as usize, 31);
            king_danger = KING_DANGER_TABLE[idx];
        }

        let mut shelter_penalty = 0;
        let file = king_sq % 8;
        for f in (file.saturating_sub(1))..=(file + 1).min(7) {
            let file_mask = FILE_MASKS[f as usize];
            let our_pawns = board.pawns[us] & file_mask;
            let their_pawns = board.pawns[them] & file_mask;
            
            if our_pawns == 0 {
                shelter_penalty += if their_pawns == 0 { 20 } else { 10 };
            } else {
                let pawn_sq = if color == Color::White { our_pawns.trailing_zeros() } else { 63 - our_pawns.leading_zeros() } as u8;
                let p_rank = pawn_sq / 8;
                let rel_rank = if color == Color::White { p_rank } else { 7 - p_rank };
                if rel_rank > 2 {
                    shelter_penalty += (rel_rank as i32 - 2) * 5;
                }
            }

            if their_pawns != 0 {
                let pawn_sq = if color == Color::White { their_pawns.trailing_zeros() } else { 63 - their_pawns.leading_zeros() } as u8;
                let p_rank = pawn_sq / 8;
                let rel_dist_to_king = if color == Color::White { p_rank as i32 - (king_sq/8) as i32 } else { (king_sq/8) as i32 - p_rank as i32 };
                if rel_dist_to_king > 0 && rel_dist_to_king <= 3 {
                    shelter_penalty += (4 - rel_dist_to_king) * 10;
                }
            }
        }

        let total_mg_penalty = king_danger + shelter_penalty;
        
        if color == Color::White {
            score.sub(total_mg_penalty, 0);
        } else {
            score.add(total_mg_penalty, 0);
        }

        // ===== King Activity (Endgame) =====
        let mut eg_bonus = 0;
        let k_rank = king_sq / 8;
        let k_file = king_sq % 8;
        let dist_x = cmp::min(k_file, 7 - k_file);
        let dist_y = cmp::min(k_rank, 7 - k_rank);
        eg_bonus += (dist_x as i32 + dist_y as i32) * 5;

        if color == Color::White {
            score.add(0, eg_bonus);
        } else {
            score.sub(0, eg_bonus);
        }
    }

    score
}

fn evaluate_pieces(board: &Board, pawn_entry: &PawnEntry) -> EvalScore {
    let mut score = EvalScore::default();

    for color in [Color::White, Color::Black] {
        let us = color.idx();
        let them = color.opposite().idx();
        let mult = if color == Color::White { 1 } else { -1 };
        
        let own_occ = board.occupancy[us];
        let enemy_pawn_attacks = pawn_entry.pawn_attacks[them];
        let our_pawns = board.pawns[us];
        
        // Safe squares are squares not occupied by our own pieces and not attacked by enemy pawns
        let safe_squares = !own_occ & !enemy_pawn_attacks;

        // ThreatBySafePawn (Stockfish 11 concept)
        let safe_pawn_attacks = pawn_entry.pawn_attacks[us] & safe_squares;
        let safe_pawn_threats = safe_pawn_attacks & (board.knights[them] | board.bishops[them] | board.rooks[them] | board.queens[them]);
        let safe_pawn_threat_count = count_bits(safe_pawn_threats) as i32;
        score.add(mult * safe_pawn_threat_count * 40, mult * safe_pawn_threat_count * 30);

        // Knights
        let mut knights = board.knights[us];
        while knights != 0 {
            let sq = pop_lsb(&mut knights);
            let attacks = get_knight_attacks(sq);
            let safe_moves = count_bits(attacks & safe_squares) as usize;
            
            let mob = KNIGHT_MOBILITY[cmp::min(safe_moves, 8)];
            score.add(mult * mob.0, mult * mob.1);

            // Outpost
            let outpost_mask = unsafe { OUTPOST_MASKS[us][sq as usize] };
            if (outpost_mask & board.pawns[them]) == 0 && (pawn_entry.pawn_attacks[us] & bit(sq)) != 0 {
                let rank = sq / 8;
                let rel_rank = if color == Color::White { rank } else { 7 - rank };
                if rel_rank >= 3 && rel_rank <= 5 {
                    let outpost_bonus = (rel_rank as i32 - 2) * 15;
                    score.add(mult * outpost_bonus, mult * outpost_bonus);
                }
            }

            // MinorBehindPawn
            let directly_ahead = if color == Color::White { shift_up(bit(sq)) } else { shift_down(bit(sq)) };
            if (directly_ahead & our_pawns) != 0 {
                score.add(mult * 18, mult * 3);
            }
        }

        // Bishops
        let mut bishops = board.bishops[us];
        while bishops != 0 {
            let sq = pop_lsb(&mut bishops);
            let attacks = get_bishop_attacks(sq, board.all_occupancy);
            let safe_moves = count_bits(attacks & safe_squares) as usize;
            
            let mob = BISHOP_MOBILITY[cmp::min(safe_moves, 13)];
            score.add(mult * mob.0, mult * mob.1);

            // Outpost
            let outpost_mask = unsafe { OUTPOST_MASKS[us][sq as usize] };
            if (outpost_mask & board.pawns[them]) == 0 && (pawn_entry.pawn_attacks[us] & bit(sq)) != 0 {
                let rank = sq / 8;
                let rel_rank = if color == Color::White { rank } else { 7 - rank };
                if rel_rank >= 3 && rel_rank <= 5 {
                    let outpost_bonus = (rel_rank as i32 - 2) * 10;
                    score.add(mult * outpost_bonus, mult * outpost_bonus);
                }
            }

            // MinorBehindPawn
            let directly_ahead = if color == Color::White { shift_up(bit(sq)) } else { shift_down(bit(sq)) };
            if (directly_ahead & our_pawns) != 0 {
                score.add(mult * 18, mult * 3);
            }

            // BishopPawns (bad bishop penalty)
            let is_light_square = ((sq / 8) + (sq % 8)) % 2 != 0;
            let same_color_pawns = if is_light_square {
                our_pawns & 0xAA55AA55AA55AA55u64
            } else {
                our_pawns & 0x55AA55AA55AA55AAu64
            };
            let pawns_on_same_color = count_bits(same_color_pawns) as i32;
            score.sub(mult * (pawns_on_same_color * 3), mult * (pawns_on_same_color * 7));
        }

        // Rooks
        let mut rooks = board.rooks[us];
        while rooks != 0 {
            let sq = pop_lsb(&mut rooks);
            let attacks = get_rook_attacks(sq, board.all_occupancy);
            let safe_moves = count_bits(attacks & safe_squares) as usize;
            
            let mob = ROOK_MOBILITY[cmp::min(safe_moves, 14)];
            score.add(mult * mob.0, mult * mob.1);
        }

        // Queens
        let mut queens = board.queens[us];
        while queens != 0 {
            let sq = pop_lsb(&mut queens);
            let attacks = get_bishop_attacks(sq, board.all_occupancy) | get_rook_attacks(sq, board.all_occupancy);
            let safe_moves = count_bits(attacks & safe_squares) as usize;
            
            let mob = QUEEN_MOBILITY[cmp::min(safe_moves, 27)];
            score.add(mult * mob.0, mult * mob.1);
        }
    }

    score
}

fn evaluate_asymmetries(board: &Board) -> EvalScore {
    let mut score = EvalScore::default();

    // Bishop Pair
    if count_bits(board.bishops[0]) >= 2 { score.add(30, 40); }
    if count_bits(board.bishops[1]) >= 2 { score.sub(30, 40); }

    // Rooks on Open files
    for color in [Color::White, Color::Black] {
        let us = color.idx();
        let them = color.opposite().idx();
        let mult = if color == Color::White { 1 } else { -1 };
        
        let mut rooks = board.rooks[us];
        while rooks != 0 {
            let sq = pop_lsb(&mut rooks);
            let file = sq % 8;
            let file_mask = FILE_MASKS[file as usize];
            
            if (board.pawns[us] & file_mask) == 0 {
                if (board.pawns[them] & file_mask) == 0 {
                    score.add(mult * 25, mult * 15); // Open
                } else {
                    score.add(mult * 10, mult * 5); // Semi-open
                }
            }
        }
    }

    score
}

fn evaluate_scale(board: &Board, mut score: i32, phase: i32) -> i32 {
    // Opposite Colored Bishops Endgame scaling
    if phase <= 2 && board.knights[0] == 0 && board.knights[1] == 0 && board.rooks[0] == 0 && board.rooks[1] == 0 && board.queens[0] == 0 && board.queens[1] == 0 {
        if count_bits(board.bishops[0]) == 1 && count_bits(board.bishops[1]) == 1 {
            let b0_sq = board.bishops[0].trailing_zeros() as u8;
            let b1_sq = board.bishops[1].trailing_zeros() as u8;
            let is_white_sq_0 = ((b0_sq / 8) + (b0_sq % 8)) % 2 != 0;
            let is_white_sq_1 = ((b1_sq / 8) + (b1_sq % 8)) % 2 != 0;
            if is_white_sq_0 != is_white_sq_1 {
                // Scale down score heavily for OCB endgames
                score /= 2;
            }
        }
    }
    score
}

fn generate_king_zone(king_sq: u8) -> u64 {
    let rank = (king_sq / 8) as i32;
    let file = (king_sq % 8) as i32;
    let mut clean_zone = 0;
    for dr in -1..=1 {
        for df in -1..=1 {
            let r = rank + dr;
            let f = file + df;
            if r >= 0 && r < 8 && f >= 0 && f < 8 {
                clean_zone |= bit((r * 8 + f) as u8);
            }
        }
    }
    clean_zone
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
    if file > 0 { mask |= FILE_MASKS[(file - 1) as usize]; }
    if file < 7 { mask |= FILE_MASKS[(file + 1) as usize]; }
    mask
}

fn generate_outpost_mask(color: Color, sq: u8) -> u64 {
    let file = sq % 8;
    let rank = sq / 8;
    let mut mask = 0u64;

    for f in (file.saturating_sub(1))..=(file + 1).min(7) {
        if f == file { continue; } 
        for r in 0..8u8 {
            let ahead = if color == Color::White { r >= rank } else { r <= rank };
            if ahead {
                mask |= bit(r * 8 + f);
            }
        }
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
        assert_eq!(evaluate(&board, None), 12); // 0 + 12cp tempo bonus
    }

    #[test]
    fn test_material_advantage() {
        init_eval();
        let mut board = Board::startpos();
        board.remove_piece(52, crate::board::piece::PieceType::Pawn, crate::board::piece::Color::Black);
        let score = evaluate(&board, None);
        assert!(score > 12);
    }

    #[test]
    fn test_from_fen_vs_startpos_pst() {
        init_eval();
        crate::movegen::init_all();
        let startpos = Board::startpos();
        let from_fen = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert_eq!(startpos.mg_pst, from_fen.mg_pst);
        assert_eq!(startpos.eg_pst, from_fen.eg_pst);
        assert_eq!(evaluate(&startpos, None), evaluate(&from_fen, None));
    }

    #[test]
    fn test_tempo_bonus() {
        init_eval();
        let mut board = Board::startpos();
        let score_white = evaluate(&board, None);
        board.side_to_move = crate::board::piece::Color::Black;
        let score_black = evaluate(&board, None);
        assert_eq!(score_white + score_black, 24);
    }

    #[test]
    fn test_eval_symmetry() {
        init_eval();
        crate::movegen::init_all();
        let fen = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
        let board = Board::from_fen(fen).unwrap();
        let eval_w = evaluate(&board, None);
        
        let fen_b = "r3k2r/pppbbppp/2n2q1P/1P2p3/3pn3/BN2PNP1/P1PPQPB1/R3K2R b KQkq - 0 1";
        let board_b = Board::from_fen(fen_b).unwrap();
        let eval_b = evaluate(&board_b, None);
        
        assert_eq!(eval_w, eval_b, "Eval is not symmetric!");
    }
}
