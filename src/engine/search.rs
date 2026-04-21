use crate::board::board::Board;
use crate::board::r#move::{Move, flags};
use crate::board::piece::PieceType;
use crate::movegen;
use crate::engine::eval::{evaluate, PawnTable};
use crate::engine::tt::{TranspositionTable, NodeType};
use std::time::{Instant, Duration};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const INFINITY: i32 = 1000000;
const MATE_VALUE: i32 = 100000;
const MAX_PLY: usize = 256;
const DELTA_MARGIN: i32 = 200; // Safety margin for delta pruning
const SEE_PIECE_VALUES: [i32; 7] = [100, 325, 335, 500, 975, 0, 0]; // P, N, B, R, Q, K, Empty

// Precomputed LMR table to avoid per-node floating-point ln() calls.
// Indexed as LMR_TABLE[depth][move_index], capped at 64 each.
fn build_lmr_table() -> [[u32; 64]; 64] {
    let mut table = [[0u32; 64]; 64];
    for depth in 1..64usize {
        for moves in 1..64usize {
            table[depth][moves] =
                (0.5 + (depth as f64).ln() * (moves as f64).ln() / 2.2) as u32;
        }
    }
    table
}

pub struct SearchResult {
    pub best_move: Option<Move>,
    pub score: i32,
    pub depth: u32,
    pub info_lines: Vec<String>,
    pub pv: Vec<Move>,
}

pub struct Searcher {
    pub nodes: Arc<AtomicU64>,
    pub seldepth: u32,
    pub start_time: Instant,
    pub soft_time_limit: Option<Duration>,
    pub hard_time_limit: Option<Duration>,
    pub stop: Arc<AtomicBool>,
    pub is_main_thread: bool,
    pub tt: Arc<TranspositionTable>,
    pub killer_moves: [[Option<Move>; 2]; MAX_PLY],
    pub history: [[[i32; 64]; 64]; 2],
    pub counter_moves: [[Option<Move>; 4096]; 2],
    pub age: u8,
    pub eval_history: [i32; MAX_PLY],
    pub pv: Vec<Move>,
    pub pawn_table: PawnTable,
    lmr_table: [[u32; 64]; 64],
}

impl Searcher {
    pub fn new(tt: Arc<TranspositionTable>) -> Self {
            Self {
                nodes: Arc::new(AtomicU64::new(0)),
                seldepth: 0,
                start_time: Instant::now(),
                soft_time_limit: None,
                hard_time_limit: None,
                stop: Arc::new(AtomicBool::new(false)),
                is_main_thread: true,
                tt,
                killer_moves: [[None; 2]; MAX_PLY],
                history: [[[0; 64]; 64]; 2],
                counter_moves: [[None; 4096]; 2],
                age: 0,
                eval_history: [0; MAX_PLY],
                pv: Vec::new(),
                pawn_table: PawnTable::new(8192),
                lmr_table: build_lmr_table(),
            }
        }

    pub fn format_score(&self, score: i32) -> String {
        if score > MATE_VALUE - 1000 {
            let mate_in = (MATE_VALUE - score + 1) / 2;
            format!("mate {}", mate_in)
        } else if score < -MATE_VALUE + 1000 {
            let mate_in = -(MATE_VALUE + score) / 2;
            format!("mate {}", mate_in)
        } else {
            format!("cp {}", score)
        }
    }

    pub fn search(
            &mut self,
            board: &mut Board,
            depth: u32,
            soft_time_limit: Option<Duration>,
            hard_time_limit: Option<Duration>,
            num_threads: usize,
        ) -> SearchResult {
            self.stop.store(false, Ordering::SeqCst);
            self.start_time = Instant::now();
            self.soft_time_limit = soft_time_limit;
            self.hard_time_limit = hard_time_limit;
            self.nodes.store(0, Ordering::SeqCst);
            self.age = self.age.wrapping_add(1);

            if num_threads <= 1 {
                return self.internal_search(board, depth);
            }

            crossbeam::scope(|s| {
                // Helper threads: Local searchers sharing TT and Stop flag
                for i in 1..num_threads {
                    let mut helper = Searcher::new(Arc::clone(&self.tt));
                    helper.nodes = Arc::clone(&self.nodes);
                    helper.stop = Arc::clone(&self.stop);
                    helper.soft_time_limit = self.soft_time_limit;
                    helper.hard_time_limit = self.hard_time_limit;
                    helper.start_time = self.start_time;
                    helper.age = self.age;
                    helper.is_main_thread = false;
                    let mut helper_board = board.clone();
                    // Helpers search slightly different depths to broaden TT coverage (Lazy SMP)
                    let helper_depth = depth + (i % 2) as u32;
                    s.spawn(move |_| {
                        helper.internal_search(&mut helper_board, helper_depth);
                    });
                }
                let result = self.internal_search(board, depth);
                self.stop.store(true, Ordering::SeqCst);
                result
            })
            .unwrap()
        }

    fn internal_search(&mut self, board: &mut Board, depth: u32) -> SearchResult {
        let mut best_move = None;
        let mut best_score = -INFINITY;
        let mut previous_best_move = None;
        let mut previous_score = -INFINITY;
        let mut last_completed_depth = 0;
        let mut info_lines = Vec::new();

        for d in 1..=depth {
            self.seldepth = 0;
            let mut alpha = -INFINITY;
            let mut beta = INFINITY;
            let mut delta = 20;

            if d >= 3 && best_score.abs() < MATE_VALUE - 1000 {
                alpha = (best_score - delta).max(-INFINITY);
                beta = (best_score + delta).min(INFINITY);
            }

            loop {
                let (_, score) = self.negamax(board, d, alpha, beta, 0, None);

                if self.stop.load(Ordering::Relaxed) {
                    break;
                }

                if score <= alpha {
                    // Fail low — widen alpha, report upperbound
                    if self.is_main_thread {
                        let elapsed = self.start_time.elapsed().as_millis() as u64;
                        let total_nodes = self.nodes.load(Ordering::Relaxed);
                        let nps = if elapsed > 0 { (total_nodes * 1000) / elapsed } else { 0 };
                        println!(
                            "info depth {} seldepth {} multipv 1 score {} upperbound nodes {} nps {} hashfull {} tbhits 0 time {}",
                            d, self.seldepth, self.format_score(score), total_nodes, nps, self.tt.hashfull(), elapsed
                        );
                    }
                    alpha = (alpha - delta).max(-INFINITY);
                    delta = (delta * 2).min(10_000);
                } else if score >= beta {
                    // Fail high — widen beta, report lowerbound
                    if self.is_main_thread {
                        let elapsed = self.start_time.elapsed().as_millis() as u64;
                        let total_nodes = self.nodes.load(Ordering::Relaxed);
                        let nps = if elapsed > 0 { (total_nodes * 1000) / elapsed } else { 0 };
                        println!(
                            "info depth {} seldepth {} multipv 1 score {} lowerbound nodes {} nps {} hashfull {} tbhits 0 time {}",
                            d, self.seldepth, self.format_score(score), total_nodes, nps, self.tt.hashfull(), elapsed
                        );
                    }
                    beta = (beta + delta).min(INFINITY);
                    delta = (delta * 2).min(10_000);
                } else {
                    best_score = score;
                    // Probe TT for the best move found at this depth
                    if let Some(entry) = self.tt.probe(board.hash) {
                        best_move = entry.best_move;
                    }
                    break;
                }

                if delta >= 10_000 {
                    alpha = -INFINITY;
                    beta = INFINITY;
                }
            }

            if self.stop.load(Ordering::Relaxed) {
                break;
            }

            last_completed_depth = d;

            if self.is_main_thread {
                self.pv = self.extract_pv(board);
            }

            let elapsed = self.start_time.elapsed().as_millis() as u64;
            let total_nodes = self.nodes.load(Ordering::Relaxed);
            let nps = if elapsed > 0 { (total_nodes * 1000) / elapsed } else { 0 };

            let pv_str = self.pv.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(" ");

            let info = format!(
                "info depth {} seldepth {} multipv 1 score {} nodes {} nps {} hashfull {} tbhits 0 time {} pv {}",
                d, self.seldepth, self.format_score(best_score), total_nodes, nps, self.tt.hashfull(), elapsed, pv_str
            );

            println!("{}", info);
            info_lines.push(info);

            if let Some(soft) = self.soft_time_limit {
                let elapsed_dur = self.start_time.elapsed();
                if elapsed_dur >= soft {
                    let mut stable = true;
                    if previous_best_move.is_some() && best_move != previous_best_move {
                        stable = false;
                    }
                    if best_score < previous_score - 50 {
                        stable = false;
                    }
                    if stable {
                        break;
                    } else if let Some(hard) = self.hard_time_limit {
                        if elapsed_dur >= hard {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }

            previous_best_move = best_move;
            previous_score = best_score;
        }

        SearchResult {
            best_move,
            score: best_score,
            depth: last_completed_depth,
            info_lines,
            pv: self.pv.clone(),
        }
    }

    fn negamax(
        &mut self,
        board: &mut Board,
        mut depth: u32,
        mut alpha: i32,
        mut beta: i32,
        ply: u32,
        excluded_move: Option<Move>,
    ) -> (Option<Move>, i32) {

        let is_pv_node = beta - alpha > 1;

        if (self.nodes.fetch_add(1, Ordering::Relaxed) & 2047) == 0 {
            self.check_time();
        }

        if self.stop.load(Ordering::Relaxed) {
            return (None, 0);
        }

        if ply >= MAX_PLY as u32 - 1 {
            return (None, evaluate(board, Some(&mut self.pawn_table)));
        }

        if board.is_repetition() || board.halfmove_clock >= 100 {
            return (None, 0);
        }

        if ply > 0 {
            // Mate distance pruning
            alpha = alpha.max(-MATE_VALUE + ply as i32);
            beta = beta.min(MATE_VALUE - ply as i32 - 1);
            if alpha >= beta { return (None, alpha); }
        }

        self.seldepth = self.seldepth.max(ply);

        let (pinned, checkers) = board.pins_and_checkers(board.side_to_move);
        let in_check = checkers != 0;
        if in_check && ply < 16 { depth += 1; }

        // TT Probe
        let tt_entry = self.tt.probe(board.hash);
        let mut tt_move = None;
        if let Some(ref entry) = tt_entry {
            tt_move = entry.best_move;

            let mut tt_score = entry.score;
            if tt_score > MATE_VALUE - 1000 { tt_score -= ply as i32; }
            else if tt_score < -MATE_VALUE + 1000 { tt_score += ply as i32; }

            if !is_pv_node && ply > 0 && entry.depth >= depth as u8 {
                match entry.node_type {
                    NodeType::Exact => return (tt_move, tt_score),
                    NodeType::Alpha if tt_score <= alpha => return (tt_move, tt_score),
                    NodeType::Beta if tt_score >= beta => return (tt_move, tt_score),
                    _ => {}
                }
            }
        }

        // IID
        if is_pv_node && tt_move.is_none() && depth >= 4 {
            let d = depth - 2;
            self.negamax(board, d, alpha, beta, ply, None);
            if let Some(entry) = self.tt.probe(board.hash) {
                tt_move = entry.best_move;
            }
        }

        if depth == 0 {
            return (None, self.quiescence(board, alpha, beta, ply));
        }

        let static_eval = evaluate(board, Some(&mut self.pawn_table));
        self.eval_history[ply as usize] = static_eval;
        let improving = ply >= 2 && static_eval > self.eval_history[ply as usize - 2];

        // RFP
        if !is_pv_node && depth <= 8 && !in_check && ply > 0 {
            let margin = if improving { 80 } else { 120 } * depth as i32;
            if static_eval - margin >= beta {
                return (None, static_eval - margin);
            }
        }

        // Null move
        if !is_pv_node && depth >= 3 && !in_check && ply > 0 {
            if board.has_non_pawn_material(board.side_to_move)
                && board.has_non_pawn_material(board.side_to_move.opposite()) {

                let eval_margin = (static_eval - beta) / 200;
                let r = 3 + depth / 6 + eval_margin.clamp(0, 3) as u32;

                let state = board.make_null_move();
                let (_, mut score) = self.negamax(board, depth.saturating_sub(r + 1), -beta, -beta + 1, ply + 1, None);
                score = -score;
                board.unmake_null_move(state);

                if score >= beta { 
                    let nmp_score = if score > MATE_VALUE - 1000 { beta } else { score };
                    return (None, nmp_score); 
                }
            }
        }

        // Singular Extension
        let mut extension = 0;
        if depth >= 8 && tt_move.is_some() && excluded_move.is_none() {
            if let Some(ref entry) = tt_entry {
                if entry.depth >= depth as u8 - 3 && entry.node_type != NodeType::Alpha {
                    let tt_score = if entry.score > MATE_VALUE - 1000 { entry.score - ply as i32 }
                                   else if entry.score < -MATE_VALUE + 1000 { entry.score + ply as i32 }
                                   else { entry.score };
                    
                    let margin = 2 * depth as i32;
                    let singular_beta = tt_score - margin;
                    let singular_depth = (depth - 1) / 2;

                    let (_, s_score) = self.negamax(board, singular_depth, singular_beta - 1, singular_beta, ply, tt_move);
                    if s_score < singular_beta {
                        extension = 1;
                    }
                }
            }
        }

        use crate::engine::movepicker::MovePicker;
        
        let mut legal_moves = 0;
        let mut best_m = None;
        let mut max_score = -INFINITY;
        let old_alpha = alpha;

        let mut is_first_move = true;
        let killers = if ply < MAX_PLY as u32 { self.killer_moves[ply as usize] } else { [None, None] };
        let mut picker = if let Some(excluded) = excluded_move {
            MovePicker::with_excluded(tt_move, killers, Some(excluded), in_check)
        } else {
            MovePicker::new(tt_move, killers, false, in_check)
        };

        while let Some(m) = picker.next(self, board, ply) {
            if !board.is_legal_fast(m, pinned, checkers) {
                continue;
            }

            let is_quiet = (m.flags() & flags::CAPTURE) == 0;
            let is_capture = (m.flags() & flags::CAPTURE) != 0;
            let is_promotion = (m.flags() & 0x8) != 0;

            // SEE pruning (must be before make_move — SEE inspects the pre-move board)
            if !is_pv_node && is_capture && depth <= 8 && legal_moves > 0 {
                let see = board.see(m);
                if see < -50 * (depth as i32) * (depth as i32) {
                    continue;
                }
            }

            // LMP
            if !is_pv_node && !in_check && depth <= 4 && is_quiet &&
                legal_moves >= (3 + 3 * depth as usize * depth as usize / 2) {
                continue;
            }

            // Futility
            if !is_pv_node && depth <= 6 && !in_check && is_quiet && !is_promotion && legal_moves > 0 {
                let margin = 120 * depth as i32 + 50;
                if static_eval + margin <= alpha {
                    continue;
                }
            }

            // History Pruning: skip quiet moves with terrible history at shallow depths
            if !is_pv_node && !in_check && is_quiet && depth <= 3 && legal_moves > 0 {
                let hist = self.history[board.side_to_move.idx()][m.from() as usize][m.to() as usize];
                if hist < -(depth as i32 * depth as i32 * 100) {
                    continue;
                }
            }

            if ply == 0 && self.is_main_thread {
                let elapsed = self.start_time.elapsed().as_millis();
                if elapsed > 300 {
                    println!("info depth {} currmove {} currmovenumber {}", depth, m, legal_moves + 1);
                }
            }

            let state = board.make_move(m);

            legal_moves += 1;

            let mut score;

            let is_pv_move = is_pv_node && is_first_move;

            // LMR
            if !is_pv_move && depth >= 3 && legal_moves > 1 && is_quiet && !is_promotion && Some(m) != tt_move {
                let mut reduction = self.lmr_table[depth.min(63) as usize][legal_moves.min(63) as usize];

                reduction = reduction.min(depth - 2);

                if !improving { reduction += 1; }

                let d = depth.saturating_sub(reduction + 1);

                (_, score) = self.negamax(board, d, -alpha - 1, -alpha, ply + 1, None);
                score = -score;

                if score > alpha && reduction > 0 {
                    (_, score) = self.negamax(board, depth - 1 + extension, -alpha - 1, -alpha, ply + 1, None);
                    score = -score;
                }

            } else {
                if is_first_move {
                    (_, score) = self.negamax(board, depth - 1 + extension, -beta, -alpha, ply + 1, None);
                } else {
                    (_, score) = self.negamax(board, depth - 1 + extension, -alpha - 1, -alpha, ply + 1, None);
                }
                score = -score;
            }

            // PV re-search
            if !is_first_move && is_pv_node && score > alpha && score < beta {
                (_, score) = self.negamax(board, depth - 1 + extension, -beta, -alpha, ply + 1, None);
                score = -score;
            }

            board.unmake_move(m, state);

            if score > max_score {
                max_score = score;
                best_m = Some(m);
            }

            if score > alpha {
                alpha = score;
                if alpha >= beta {
                    if is_quiet { self.update_heuristics(m, board, depth, ply); }
                    break;
                }
            } else if is_quiet {
                self.apply_history_malus(m, board, depth);
            }

            is_first_move = false;
        }

        if legal_moves == 0 {
            return if in_check {
                (None, -MATE_VALUE + ply as i32)
            } else {
                (None, 0)
            };
        }

        let node_type = if max_score >= beta { NodeType::Beta }
                        else if max_score > old_alpha { NodeType::Exact }
                        else { NodeType::Alpha };

        let mut store_score = max_score;
        if store_score > MATE_VALUE - 1000 { store_score += ply as i32; }
        else if store_score < -MATE_VALUE + 1000 { store_score -= ply as i32; }

        self.tt.store(board.hash, depth as u8, store_score, node_type, best_m, self.age);

        (best_m, max_score)
    }

    fn quiescence(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: u32) -> i32 {
            // Node counting is done in negamax; avoid double-counting here.
            // Only check time periodically.
            if (self.nodes.load(Ordering::Relaxed) & 2047) == 0 { self.check_time(); }

            if ply >= MAX_PLY as u32 - 1 {
                return evaluate(board, Some(&mut self.pawn_table));
            }
            
            if board.is_repetition() || board.halfmove_clock >= 100 {
                return 0;
            }

            self.seldepth = self.seldepth.max(ply);

            let (pinned, checkers) = board.pins_and_checkers(board.side_to_move);
            let in_check = checkers != 0;
            let stand_pat = if in_check { -INFINITY } else { evaluate(board, Some(&mut self.pawn_table)) };

            let mut best_score = stand_pat;

            if !in_check {
                if stand_pat >= beta { return stand_pat; }

                // Delta Pruning: if even the biggest possible gain can't reach alpha, bail
                let big_delta = SEE_PIECE_VALUES[4] + DELTA_MARGIN; // Queen value + margin
                if stand_pat + big_delta < alpha {
                    return stand_pat;
                }

                if stand_pat > alpha { alpha = stand_pat; }
            }

            let tt_entry = self.tt.probe(board.hash);
            let mut tt_move = None;
            if let Some(ref entry) = tt_entry {
                tt_move = entry.best_move;
                let mut tt_score = entry.score;
                if tt_score > MATE_VALUE - 1000 { tt_score -= ply as i32; }
                else if tt_score < -MATE_VALUE + 1000 { tt_score += ply as i32; }

                match entry.node_type {
                    NodeType::Exact => return tt_score,
                    NodeType::Alpha if tt_score <= alpha => return tt_score,
                    NodeType::Beta if tt_score >= beta => return tt_score,
                    _ => {}
                }
            }

            use crate::engine::movepicker::MovePicker;
            
            let mut legal_moves = 0;
            let killers = [None, None];
            let mut picker = MovePicker::new(tt_move, killers, true, in_check);

            while let Some(m) = picker.next(self, board, ply) {
                if !board.is_legal_fast(m, pinned, checkers) { continue; }

                // Delta Pruning per-move: skip captures that can't raise alpha
                if !in_check {
                    let captured_pt = board.pieces[m.to() as usize];
                    let captured_val = SEE_PIECE_VALUES[captured_pt as usize];
                    if stand_pat + captured_val + DELTA_MARGIN < alpha {
                        continue;
                    }
                }

                self.nodes.fetch_add(1, Ordering::Relaxed);

                let state = board.make_move(m);
                legal_moves += 1;

                let score = -self.quiescence(board, -beta, -alpha, ply + 1);
                board.unmake_move(m, state);

                if score > best_score {
                    best_score = score;
                    if score > alpha {
                        alpha = score;
                        if score >= beta { return score; }
                    }
                }
            }

            if in_check && legal_moves == 0 { return -MATE_VALUE + ply as i32; }
            best_score
        }

    pub fn pick_move(&self, moves: &mut [Move], scores: &mut [i32], start: usize) {
        let mut best_idx = start;
        let mut best_score = scores[start];
        for i in start + 1..moves.len() {
            if scores[i] > best_score {
                best_score = scores[i];
                best_idx = i;
            }
        }
        moves.swap(start, best_idx);
        scores.swap(start, best_idx);
    }

    pub fn score_move(&self, m: Move, board: &Board, tt_move: Option<Move>, ply: u32, is_qsearch: bool) -> i32 {
        if Some(m) == tt_move {
            return 1_000_000;
        }

        let is_capture = (m.flags() & flags::CAPTURE) != 0;
        let is_promotion = (m.flags() & 0x8) != 0;

        if is_capture {
            let victim = board.pieces[m.to() as usize];
            let attacker = board.pieces[m.from() as usize];
            
            // MVV-LVA base score
            let score = 50_000 + 10 * self.val(victim) - self.val(attacker);
            
            if is_qsearch {
                // In qsearch, we can afford SEE if it's not a clear win
                if self.val(victim) <= self.val(attacker) {
                    let see = board.see(m);
                    if see < 0 { return -50_000 + see; }
                }
            } else {
                // In main search, we can use SEE to penalize bad captures
                let see = board.see(m);
                if see < 0 { return -5_000 + see; }
            }
            return score;
        }

        if is_promotion {
            return match m.flags() {
                flags::PROMOTE_QUEEN | flags::PROMOTE_QUEEN_CAPTURE => 45_000,
                flags::PROMOTE_ROOK | flags::PROMOTE_ROOK_CAPTURE => 25_000,
                flags::PROMOTE_BISHOP | flags::PROMOTE_BISHOP_CAPTURE => 24_000,
                flags::PROMOTE_KNIGHT | flags::PROMOTE_KNIGHT_CAPTURE => 23_000,
                _ => 10_000,
            };
        }

        // Countermove heuristic
        if let Some(last_move) = board.last_move {
            let last_idx = (last_move.from() as usize) | ((last_move.to() as usize) << 6);
            if Some(m) == self.counter_moves[board.side_to_move.idx()][last_idx] {
                return 9_500;
            }
        }

        // Killer moves
        if ply < MAX_PLY as u32 {
            if Some(m) == self.killer_moves[ply as usize][0] {
                return 9_000;
            }
            if Some(m) == self.killer_moves[ply as usize][1] {
                return 8_000;
            }
        }

        // History heuristic
        self.history[board.side_to_move.idx()][m.from() as usize][m.to() as usize] / 10
    }

    pub fn val(&self, pt: PieceType) -> i32 {
        match pt {
            PieceType::Pawn => 1,
            PieceType::Knight => 3,
            PieceType::Bishop => 3,
            PieceType::Rook => 5,
            PieceType::Queen => 9,
            PieceType::King => 0,
            PieceType::Empty => 0,
        }
    }

    fn check_time(&mut self) {
        let limit = self.hard_time_limit.or(self.soft_time_limit);
        if let Some(l) = limit {
            if self.start_time.elapsed() >= l {
                self.stop.store(true, Ordering::Relaxed);
            }
        }
    }

    fn update_heuristics(&mut self, m: Move, board: &Board, depth: u32, ply: u32) {
        // Killer moves
        if ply < MAX_PLY as u32 && Some(m) != self.killer_moves[ply as usize][0] {
            self.killer_moves[ply as usize][1] = self.killer_moves[ply as usize][0];
            self.killer_moves[ply as usize][0] = Some(m);
        }

        // History with gravity (positive bonus for the cutoff move)
        let bonus = (depth * depth).min(400) as i32;
        let entry =
            &mut self.history[board.side_to_move.idx()][m.from() as usize][m.to() as usize];
        let current = *entry as i64;
        let b = bonus as i64;
        *entry = (current + b - (current * b / 32_768)) as i32;

        // Counter move — store m as the reply to the opponent's last move.
        if let Some(last_move) = board.last_move {
            let last_idx = (last_move.from() as usize) | ((last_move.to() as usize) << 6);
            self.counter_moves[board.side_to_move.idx()][last_idx] = Some(m);
        }
    }

    /// Apply a negative history adjustment to a quiet move that was
    /// searched but failed to produce a beta cutoff.
    fn apply_history_malus(&mut self, m: Move, board: &Board, depth: u32) {
        let malus = -((depth * depth).min(400) as i32);
        let entry =
            &mut self.history[board.side_to_move.idx()][m.from() as usize][m.to() as usize];
        let current = *entry as i64;
        let b = malus as i64;
        // Same gravity formula as the bonus path, keeping values bounded.
       *entry = (current + b - (current.abs() * b / 32_768)) as i32;
    }

    fn extract_pv(&self, board: &mut Board) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut states = Vec::new();
        let mut visited = Vec::new();

        // Limit the maximum PV length to prevent overly long extractions
        let mut max_len = 64;

        while max_len > 0 {
            if visited.contains(&board.hash) {
                break;
            }
            visited.push(board.hash);

            if let Some(entry) = self.tt.probe(board.hash) {
                if let Some(m) = entry.best_move {
                    // Quick legality check to avoid panics on corrupted or colliding TT entries
                    let pseudo_moves = movegen::generate_pseudo_legal_moves(board);
                    if !pseudo_moves.as_slice().contains(&m) || !board.is_legal(m) {
                        break;
                    }
                    pv.push(m);
                    states.push(board.make_move(m));
                    max_len -= 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Unmake moves to restore board state
        for (m, state) in pv.iter().rev().zip(states.into_iter().rev()) {
            board.unmake_move(*m, state);
        }

        pv
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;
    use crate::movegen::init_all;
    use crate::board::piece::Color;

    #[test]
    fn test_mate_in_one() {
        init_all();
        let mut board = Board {
            pawns: [0; 2],
            knights: [0; 2],
            bishops: [0; 2],
            rooks: [0; 2],
            queens: [0; 2],
            kings: [0; 2],
            occupancy: [0; 2],
            all_occupancy: 0,
            side_to_move: crate::board::piece::Color::White,
            castling_rights: 0,
            en_passant_square: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            last_move: None,
            history: Vec::new(),
            pieces: [PieceType::Empty; 64],
            colors: [Color::None; 64],
            hash: 0,
            mg_pst: 0,
            eg_pst: 0,
            pawn_hash: 0,
        };
        board.put_piece(7, crate::board::piece::PieceType::King, crate::board::piece::Color::White);
        board.put_piece(48, crate::board::piece::PieceType::Rook, crate::board::piece::Color::White);
        board.put_piece(49, crate::board::piece::PieceType::Rook, crate::board::piece::Color::White);
        board.put_piece(63, crate::board::piece::PieceType::King, crate::board::piece::Color::Black);
        board.update_occupancy();
        board.hash = board.compute_hash();

        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);
        let result = searcher.search(&mut board, 3, None, None, 1);

        assert!(result.best_move.is_some());
        assert_eq!(result.depth, 3);
        assert!(!result.info_lines.is_empty());
        assert!(result.score >= MATE_VALUE - 100);
    }

    #[test]
    fn test_search_depth_with_time_limit() {
        init_all();
        let mut board = Board::startpos();
        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);
        let result = searcher.search(&mut board, 64, Some(Duration::from_millis(500)), Some(Duration::from_millis(500)), 1);
        assert!(result.depth > 1);
        assert!(result.depth <= 64);
    }

    #[test]
    fn test_search_multithreaded() {
        init_all();
        let mut board = Board::startpos();
        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);
        let result = searcher.search(&mut board, 6, None, None, 2);
        assert!(result.best_move.is_some());
        assert!(result.depth >= 6);
    }

    #[test]
    fn test_repetition_detection() {
        init_all();
        let mut board = Board::startpos();
        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);

        // Repetition: Nf3-g1, Nc6-b8, Ng1-f3, Nb8-c6
        // White moves: g1f3, b1c3, f3g1, c3b1, g1f3
        // Black moves: g8f6, b8c6, f6g8, c6b8
        let moves = ["g1f3", "g8f6", "b1c3", "b8c6", "f3g1", "f6g8", "c3b1", "c6b8", "g1f3"];
        for m_str in moves {
            let m = board.parse_move(m_str).unwrap_or_else(|| panic!("Failed to parse {}", m_str));
            board.make_move(m);
        }

        assert!(board.is_repetition());
        
        // Search should return 0 for this position if it's a draw
        let result = searcher.search(&mut board, 1, None, None, 1);
        assert_eq!(result.score, 0);
    }

    #[test]
    fn test_mate_in_two() {
        init_all();
        // Position: r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5Q2/PPPP1PPP/RNB1K1NR w KQkq - 0 1
        // Scholars mate threat
        let mut board = Board::from_fen("r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5Q2/PPPP1PPP/RNB1K1NR w KQkq - 4 4").unwrap();
        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);
        let result = searcher.search(&mut board, 4, None, None, 1);
        
        assert!(result.score >= MATE_VALUE - 100);
        let best_move = result.best_move.unwrap();
        assert_eq!(best_move.to_string(), "f3f7");
    }

    #[test]
    fn test_quiescence_tactical() {
        init_all();
        // White to move, can win a piece by capture chain
        let mut board = Board::from_fen("r1bqk2r/pppp1ppp/2n2n2/4p3/2B1P3/2N2N2/PPPP1PPP/R1BQK2R w KQkq - 0 1").unwrap();
        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);
        let result = searcher.search(&mut board, 1, None, None, 1);
        // Position is roughly balanced but engine may find tactical threats in qsearch
        // (e.g. Ng5 attacking f7). Score should still be reasonable.
        assert!(result.score.abs() < 700);
    }
}