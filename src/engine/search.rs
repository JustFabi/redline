use crate::board::board::Board;
use crate::board::r#move::{Move, flags};
use crate::board::piece::PieceType;
use crate::movegen;
use crate::engine::eval::evaluate;
use crate::engine::tt::{TranspositionTable, NodeType};
use std::time::{Instant, Duration};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const INFINITY: i32 = 1000000;
const MATE_VALUE: i32 = 100000;

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
    pub nodes: u64,
    pub depth: u32,
    pub info_lines: Vec<String>,
}

pub struct Searcher {
    pub nodes: Arc<AtomicU64>,
    pub seldepth: u32,
    pub start_time: Instant,
    pub time_limit: Option<Duration>,
    pub stop: Arc<AtomicBool>,
    pub tt: Arc<TranspositionTable>,
    pub killer_moves: [[Option<Move>; 2]; 128],
    pub history: [[[i32; 64]; 64]; 2],
    pub counter_moves: [[Option<Move>; 4096]; 2],
    pub age: u8,
    pub eval_history: [i32; 128],
    lmr_table: [[u32; 64]; 64],
}

impl Searcher {
    pub fn new(tt: Arc<TranspositionTable>) -> Self {
            Self {
                nodes: Arc::new(AtomicU64::new(0)),
                seldepth: 0,
                start_time: Instant::now(),
                time_limit: None,
                stop: Arc::new(AtomicBool::new(false)),
                tt,
                killer_moves: [[None; 2]; 128],
                history: [[[0; 64]; 64]; 2],
                counter_moves: [[None; 4096]; 2],
                age: 0,
                eval_history: [0; 128],
                lmr_table: build_lmr_table(),
            }
        }

    pub fn reset_heuristics(&mut self) {
        self.killer_moves = [[None; 2]; 128];
        for val in self.history.iter_mut().flatten().flatten() {
            *val >>= 1;
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
            time_limit: Option<Duration>,
            num_threads: usize,
        ) -> SearchResult {
            self.stop.store(false, Ordering::SeqCst);
            self.start_time = Instant::now();
            self.time_limit = time_limit;
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
                    helper.time_limit = self.time_limit;
                    helper.start_time = self.start_time;
                    helper.age = self.age;
                    let mut helper_board = board.clone();
                    // Helpers search slightly different depths to broaden TT coverage (Lazy SMP)
                    let helper_depth = depth + (i % 2) as u32;
                    s.spawn(move |_| {
                        helper.internal_search(&mut helper_board, helper_depth);
                    });
                }
                self.internal_search(board, depth)
            })
            .unwrap()
        }

    fn extract_pv(&self, board: &Board, depth: u32) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut current_board = board.clone();
        let mut seen_hashes = [0u64; 128];
        let mut seen_count = 0;

        for _ in 0..depth.min(127) {
            let hash = current_board.hash;
            
            let mut repeated = false;
            for i in 0..seen_count {
                if seen_hashes[i] == hash {
                    repeated = true;
                    break;
                }
            }
            if repeated { break; }
            seen_hashes[seen_count] = hash;
            seen_count += 1;

            if let Some(entry) = self.tt.probe(hash) {
                if let Some(m) = entry.best_move {
                    let state = current_board.make_move(m);
                    if current_board.is_in_check(current_board.side_to_move.opposite()) {
                        current_board.unmake_move(m, state);
                        break;
                    }
                    pv.push(m);
                } else { break; }
            } else { break; }
        }
        pv
    }

    fn internal_search(&mut self, board: &mut Board, depth: u32) -> SearchResult {
        let mut best_move = None;
        let mut best_score = -INFINITY;
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
                    alpha = (alpha - delta).max(-INFINITY);
                    beta = (beta + delta / 2).min(INFINITY);
                    delta = (delta * 2).min(10_000);
                } else if score >= beta {
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

            let elapsed = self.start_time.elapsed().as_millis() as u64;
            let total_nodes = self.nodes.load(Ordering::Relaxed);
            let nps = if elapsed > 0 { (total_nodes * 1000) / elapsed } else { 0 };

            let pv = self.extract_pv(board, d);
            let pv_str = pv.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(" ");

            let info = format!(
                "info depth {} seldepth {} multipv 1 score {} nodes {} nps {} hashfull {} tbhits 0 time {} pv {}",
                d, self.seldepth, self.format_score(best_score), total_nodes, nps, self.tt.hashfull(), elapsed, pv_str
            );

            println!("{}", info);
            info_lines.push(info);
        }

        SearchResult {
            best_move,
            score: best_score,
            nodes: self.nodes.load(Ordering::SeqCst),
            depth: last_completed_depth,
            info_lines,
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

        let in_check = board.is_in_check(board.side_to_move);
        if in_check && depth < 5 { depth += 1; }

        // TT Probe
        let tt_entry = self.tt.probe(board.hash);
        let mut tt_move = None;
        if let Some(ref entry) = tt_entry {
            tt_move = entry.best_move;

            let mut tt_score = entry.score;
            if tt_score > MATE_VALUE - 1000 { tt_score -= ply as i32; }
            else if tt_score < -MATE_VALUE + 1000 { tt_score += ply as i32; }

            if ply > 0 && entry.depth >= depth as u8 && excluded_move.is_none() {
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

        let static_eval = evaluate(board);
        self.eval_history[ply as usize] = static_eval;
        let improving = ply >= 2 && static_eval > self.eval_history[ply as usize - 2];

        // RFP
        if depth <= 8 && !in_check && ply > 0 && excluded_move.is_none() {
            let margin = if improving { 80 } else { 120 } * depth as i32;
            if static_eval - margin >= beta {
                return (None, static_eval - margin);
            }
        }

        // Null move
        if depth >= 3 && !in_check && ply > 0 && excluded_move.is_none() {
            if board.has_non_pawn_material(board.side_to_move)
                && board.has_non_pawn_material(board.side_to_move.opposite()) {

                let r = 3 + depth / 4;
                let state = board.make_null_move();
                let (_, score) = self.negamax(board, depth.saturating_sub(r + 1), -beta, -beta + 1, ply + 1, None);
                board.unmake_null_move(state);

                if -score >= beta { return (None, beta); }
            }
        }

        let mut moves = movegen::generate_pseudo_legal_moves(board);
        let mut scores = self.score_moves(moves.as_slice(), board, tt_move, ply, false);

        let mut legal_moves = 0;
        let mut best_m = None;
        let mut max_score = -INFINITY;
        let old_alpha = alpha;

        let mut is_first_move = true;

        for i in 0..moves.len() {
            self.pick_move(moves.as_mut_slice(), &mut scores, i);
            let m = moves.get(i);
            if Some(m) == excluded_move { continue; }

            let state = board.make_move(m);
            if board.is_in_check(board.side_to_move.opposite()) {
                board.unmake_move(m, state);
                continue;
            }

            legal_moves += 1;

            let is_quiet = (m.flags() & flags::CAPTURE) == 0;
            let is_capture = (m.flags() & flags::CAPTURE) != 0;
            let is_promotion = (m.flags() & 0x8) != 0;

            // SEE pruning
            if is_capture && depth <= 8 {
                let see = board.see(m);
                if see < -17 * (depth as i32) * (depth as i32) {
                    board.unmake_move(m, state);
                    continue;
                }
            }

            // LMP
            if !in_check && depth <= 4 && is_quiet &&
                legal_moves >= (3 + 3 * depth as usize * depth as usize / 2) {
                board.unmake_move(m, state);
                continue;
            }

            // Futility
            if depth <= 6 && !in_check && is_quiet && !is_promotion && legal_moves > 1 {
                let margin = 120 * depth as i32 + 50;
                if static_eval + margin <= alpha {
                    board.unmake_move(m, state);
                    continue;
                }
            }

            let mut score;

            let is_pv_move = is_pv_node && is_first_move;

            // LMR
            if !is_pv_move && depth >= 3 && legal_moves > 1 && is_quiet && !is_promotion && Some(m) != tt_move {
                let mut reduction = self.lmr_table[depth.min(63) as usize][legal_moves.min(63) as usize];

                if depth <= 3 {
                    reduction = 0;
                } else {
                    reduction = reduction.min(depth - 2);
                }

                if !improving { reduction += 1; }

                let d = depth.saturating_sub(reduction + 1);

                (_, score) = self.negamax(board, d, -alpha - 1, -alpha, ply + 1, None);
                score = -score;

                if score > alpha && reduction > 0 {
                    (_, score) = self.negamax(board, depth - 1, -alpha - 1, -alpha, ply + 1, None);
                    score = -score;
                }

            } else {
                if is_first_move {
                    (_, score) = self.negamax(board, depth - 1, -beta, -alpha, ply + 1, None);
                } else {
                    (_, score) = self.negamax(board, depth - 1, -alpha - 1, -alpha, ply + 1, None);
                }
                score = -score;
            }

            // PV re-search
            if !is_first_move && is_pv_node && score > alpha && score < beta {
                (_, score) = self.negamax(board, depth - 1, -beta, -alpha, ply + 1, None);
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

        if excluded_move.is_none() {
            self.tt.store(board.hash, depth as u8, store_score, node_type, best_m, self.age);
        }

        (best_m, max_score)
    }

    fn quiescence(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: u32) -> i32 {
            if (self.nodes.fetch_add(1, Ordering::Relaxed) & 2047) == 0 { self.check_time(); }
            self.seldepth = self.seldepth.max(ply);

            let in_check = board.is_in_check(board.side_to_move);
            let stand_pat = if in_check { -INFINITY } else { evaluate(board) };

            if stand_pat >= beta { return beta; }
            if stand_pat > alpha { alpha = stand_pat; }

            let mut moves = if in_check {
                movegen::generate_pseudo_legal_moves(board)
            } else {
                movegen::generate_captures(board)
            };

            let mut scores = self.score_moves(moves.as_slice(), board, None, ply, true);
            let mut legal_moves = 0;

            for i in 0..moves.len() {
                self.pick_move(moves.as_mut_slice(), &mut scores, i);
                let m = moves.get(i);
                let see = scores[i];

                if !in_check && see < 0 { continue; }

                let state = board.make_move(m);
                if board.is_in_check(board.side_to_move.opposite()) {
                    board.unmake_move(m, state);
                    continue;
                }
                legal_moves += 1;

                let score = -self.quiescence(board, -beta, -alpha, ply + 1);
                board.unmake_move(m, state);

                if score >= beta { return beta; }
                if score > alpha { alpha = score; }
            }

            if in_check && legal_moves == 0 { return -MATE_VALUE + ply as i32; }
            alpha
        }

    fn pick_move(&self, moves: &mut [Move], scores: &mut [i32], start: usize) {
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

    fn score_moves(
        &self,
        moves: &[Move],
        board: &Board,
        tt_move: Option<Move>,
        ply: u32,
        is_qsearch: bool,
    ) -> Vec<i32> {
        let mut scores = Vec::with_capacity(moves.len());
        for &m in moves {
            scores.push(self.score_move(m, board, tt_move, ply, is_qsearch));
        }
        scores
    }

    fn score_move(&self, m: Move, board: &Board, tt_move: Option<Move>, ply: u32, is_qsearch: bool) -> i32 {
        if Some(m) == tt_move {
            return 1_000_000;
        }

        let is_capture = (m.flags() & flags::CAPTURE) != 0;
        let is_promotion = (m.flags() & 0x8) != 0;

        if is_capture {
            let victim = board.pieces[m.to() as usize].unwrap_or(PieceType::Pawn);
            let attacker = board.pieces[m.from() as usize].unwrap_or(PieceType::Pawn);
            
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
                if see < 0 { return 20_000 + see; }
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
        if ply < 128 {
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

    fn val(&self, pt: PieceType) -> i32 {
        match pt {
            PieceType::Pawn => 1,
            PieceType::Knight => 3,
            PieceType::Bishop => 3,
            PieceType::Rook => 5,
            PieceType::Queen => 9,
            PieceType::King => 0,
        }
    }

    fn check_time(&mut self) {
        if let Some(limit) = self.time_limit {
            if self.start_time.elapsed() >= limit {
                self.stop.store(true, Ordering::Relaxed);
            }
        }
    }

    fn update_heuristics(&mut self, m: Move, board: &Board, depth: u32, ply: u32) {
        // Killer moves
        if ply < 128 && Some(m) != self.killer_moves[ply as usize][0] {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;
    use crate::movegen::init_all;

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
            pieces: [None; 64],
            colors: [None; 64],
            hash: 0,
        };
        board.put_piece(7, crate::board::piece::PieceType::King, crate::board::piece::Color::White);
        board.put_piece(48, crate::board::piece::PieceType::Rook, crate::board::piece::Color::White);
        board.put_piece(49, crate::board::piece::PieceType::Rook, crate::board::piece::Color::White);
        board.put_piece(63, crate::board::piece::PieceType::King, crate::board::piece::Color::Black);
        board.update_occupancy();
        board.hash = board.compute_hash();

        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);
        let result = searcher.search(&mut board, 3, None, 1);

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
        let result = searcher.search(&mut board, 64, Some(Duration::from_millis(500)), 1);
        assert!(result.depth > 1);
        assert!(result.depth <= 64);
    }

    #[test]
    fn test_search_multithreaded() {
        init_all();
        let mut board = Board::startpos();
        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);
        let result = searcher.search(&mut board, 6, None, 2);
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
        let result = searcher.search(&mut board, 1, None, 1);
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
        let result = searcher.search(&mut board, 4, None, 1);
        
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
        let result = searcher.search(&mut board, 1, None, 1);
        // It's balanced, but eval might not be exactly 0 due to some weights
        assert!(result.score.abs() < 500);
    }
}