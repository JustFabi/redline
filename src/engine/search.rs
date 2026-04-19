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
    pub killer_moves: [[Option<Move>; 2]; 128], // Killer moves for move ordering
    pub history: [[[i32; 64]; 64]; 2],         // History heuristic: history[color][from][to]
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
        }
    }

    pub fn search(&mut self, board: &mut Board, depth: u32, time_limit: Option<Duration>, num_threads: usize) -> SearchResult {
        self.stop.store(false, Ordering::SeqCst);
        self.start_time = Instant::now();
        self.time_limit = time_limit;
        self.nodes.store(0, Ordering::SeqCst);
        self.seldepth = 0;

        let num_threads = num_threads.max(1);

        if num_threads == 1 {
            return self.internal_search(board, depth);
        }

        // Lazy SMP implementation
        crossbeam::scope(|s| {
            for i in 0..num_threads {
                let mut thread_searcher = Searcher::new(Arc::clone(&self.tt));
                thread_searcher.nodes = Arc::clone(&self.nodes);
                thread_searcher.stop = Arc::clone(&self.stop);
                thread_searcher.time_limit = self.time_limit;
                thread_searcher.start_time = self.start_time;
                let mut thread_board = board.clone();

                // Asymmetric search: different threads search to slightly different depths
                let thread_depth = depth + (i % 2) as u32;

                s.spawn(move |_| {
                    thread_searcher.internal_search(&mut thread_board, thread_depth)
                });
            }
        }).expect("Thread search failed");

        // After all threads are done, the result for the requested depth should be in the TT.
        // We do one quick probe to get the best move and score.
        let mut best_move = None;
        let mut best_score = -INFINITY;
        let mut reached_depth = 0;

        if let Some(entry) = self.tt.probe(board.hash) {
            best_move = entry.best_move;
            best_score = entry.score;
            reached_depth = entry.depth as u32;
        }

        SearchResult {
            best_move,
            score: best_score,
            nodes: self.nodes.load(Ordering::SeqCst), // This only counts nodes from the main thread's previous searches, but TT was shared.
            depth: reached_depth,
            info_lines: Vec::new(),
        }
    }

    fn extract_pv(&self, board: &mut Board, depth: u32) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut current_board = board.clone();

        for _ in 0..depth {
            if let Some(entry) = self.tt.probe(current_board.hash) {
                if let Some(m) = entry.best_move {
                    // Check move legality (optional but recommended)
                    let state = current_board.make_move(m);
                    if current_board.is_in_check(current_board.side_to_move.opposite()) {
                        current_board.unmake_move(m, state);
                        break;
                    }
                    pv.push(m);
                    // No unmake needed as we cloned the board or we can just continue
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        pv
    }

    fn internal_search(&mut self, board: &mut Board, depth: u32) -> SearchResult {
        let mut best_move = None;
        let mut best_score = -INFINITY;
        let mut alpha;
        let mut beta;
        let mut last_completed_depth = 0;
        let mut info_lines = Vec::new();

        for d in 1..=depth {
            // Aspiration Windows
            let mut delta = 20;
            if d > 4 {
                alpha = best_score - delta;
                beta = best_score + delta;
            } else {
                alpha = -INFINITY;
                beta = INFINITY;
            }

            let mut depth_interrupted = false;
            loop {
                let (m, score) = self.negamax(board, d, alpha, beta, 0);

                if self.stop.load(Ordering::Relaxed) {
                    depth_interrupted = true;
                    break;
                }

                if score <= alpha {
                    alpha = (alpha - delta).max(-INFINITY);
                    delta *= 2;
                } else if score >= beta {
                    beta = (beta + delta).min(INFINITY);
                    delta *= 2;
                } else {
                    best_move = m;
                    best_score = score;
                    break;
                }

                if delta > 1000 { // Fallback if aspiration fails badly
                    alpha = -INFINITY;
                    beta = INFINITY;
                }
            }

            if depth_interrupted { break; }
            last_completed_depth = d;

            let elapsed = self.start_time.elapsed().as_millis() as u64;
            let total_nodes = self.nodes.load(Ordering::Relaxed);
            let nps = if elapsed > 0 { (total_nodes * 1000) / elapsed } else { 0 };
            let hashfull = self.tt.hashfull();
            let pv = self.extract_pv(board, d);
            let pv_str = pv.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(" ");

            let info = format!("info depth {} seldepth {} multipv 1 score cp {} nodes {} nps {} hashfull {} tbhits 0 time {} pv {}",
                d, self.seldepth, best_score, total_nodes, nps, hashfull, elapsed, pv_str);
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

    fn negamax(&mut self, board: &mut Board, depth: u32, alpha: i32, beta: i32, ply: u32) -> (Option<Move>, i32) {
            let current_nodes = self.nodes.fetch_add(1, Ordering::Relaxed) + 1;
            self.seldepth = self.seldepth.max(ply);
            if (current_nodes & 2047) == 0 { self.check_time(); }
            if self.stop.load(Ordering::Relaxed) { return (None, 0); }

            // Mate distance pruning
            let mut alpha = alpha;
            let mut beta = beta;
            alpha = alpha.max(-MATE_VALUE + ply as i32);
            beta = beta.min(MATE_VALUE - ply as i32);
            if alpha >= beta { return (None, alpha); }

            let mut tt_move = None;
            if let Some(entry) = self.tt.probe(board.hash) {
                if entry.depth >= depth as u8 && ply > 0 {
                    match entry.node_type {
                        NodeType::Exact => return (entry.best_move, entry.score),
                        NodeType::Alpha if entry.score <= alpha => return (entry.best_move, alpha),
                        NodeType::Beta if entry.score >= beta => return (entry.best_move, beta),
                        _ => {}
                    }
                }
                tt_move = entry.best_move;
            }

            if depth == 0 {
                return (None, self.quiescence(board, alpha, beta, ply));
            }

            // Null Move Pruning (NMP)
            if depth >= 3 && !board.is_in_check(board.side_to_move) && ply > 0 {
                // Check if we have pieces other than pawns (simplified check)
                let major_pieces = board.occupancy[board.side_to_move.idx()] & !(board.pawns[board.side_to_move.idx()] | board.kings[board.side_to_move.idx()]);
                if major_pieces != 0 {
                    let state = board.make_null_move();
                    let (_, score) = self.negamax(board, depth - 3, -beta, -beta + 1, ply + 1);
                    board.unmake_null_move(state);
                    if -score >= beta {
                        return (None, beta);
                    }
                }
            }

            // Use generate_pseudo_legal_moves for performance
            let mut moves = movegen::generate_pseudo_legal_moves(board);

            // RFP / Static Null Move Pruning
            if depth <= 3 && !board.is_in_check(board.side_to_move) && ply > 0 {
                let static_eval = evaluate(board);
                let margin = 120 * depth as i32;
                if static_eval - margin >= beta {
                    return (None, static_eval - margin);
                }
            }

            let mut best_move = None;
            let mut max_score = -INFINITY;
            let old_alpha = alpha;
            let mut legal_moves_count = 0;

            for i in 0..moves.len() {
                self.pick_move(&mut moves, i, board, tt_move, ply);
                let m = moves[i];

                let state = board.make_move(m);
                // Legality check
                if board.is_in_check(board.side_to_move.opposite()) {
                    board.unmake_move(m, state);
                    continue;
                }
                legal_moves_count += 1;

                // Futility Pruning
                if depth == 1 && !board.is_in_check(board.side_to_move) && legal_moves_count > 1 
                    && (m.flags() & flags::CAPTURE) == 0 && (m.flags() & (flags::PROMOTE_QUEEN | flags::PROMOTE_QUEEN_CAPTURE)) == 0 {
                    let static_eval = evaluate(board);
                    if static_eval + 150 < alpha {
                        board.unmake_move(m, state);
                        continue;
                    }
                }

                if ply == 0 {
                    let elapsed = self.start_time.elapsed().as_millis() as u64;
                    let total_nodes = self.nodes.load(Ordering::Relaxed);
                    let nps = if elapsed > 0 { (total_nodes * 1000) / elapsed } else { 0 };
                    println!("info depth {} currmove {} currmovenumber {} nodes {} nps {} time {}", depth, m, legal_moves_count, total_nodes, nps, elapsed);
                }

                let mut score;
                // Late Move Reductions (LMR)
                if depth >= 3 && legal_moves_count > 4 && (m.flags() & flags::CAPTURE) == 0 && (m.flags() & (flags::PROMOTE_QUEEN | flags::PROMOTE_QUEEN_CAPTURE)) == 0 && !board.is_in_check(board.side_to_move) {
                    let mut reduction: u32 = 1;
                    if legal_moves_count > 12 { reduction += 1; }
                    if depth > 5 { reduction += 1; }
                    
                    // History-based LMR
                    let history_score = self.history[board.side_to_move.idx()][m.from() as usize][m.to() as usize];
                    if history_score > 5000 {
                        reduction = reduction.saturating_sub(1);
                    }

                    let (_, s) = self.negamax(board, depth.saturating_sub(1 + reduction), -alpha - 1, -alpha, ply + 1);
                    score = -s;
                    
                    // If the reduced search improved alpha, re-search at full depth
                    if score > alpha {
                        let (_, s) = self.negamax(board, depth - 1, -beta, -alpha, ply + 1);
                        score = -s;
                    }
                } else {
                    let (_, s) = self.negamax(board, depth - 1, -beta, -alpha, ply + 1);
                    score = -s;
                }
                
                board.unmake_move(m, state);

                if self.stop.load(Ordering::Relaxed) { return (None, 0); }

                if score > max_score {
                    max_score = score;
                    best_move = Some(m);
                }
                if score > alpha {
                    alpha = score;
                    if alpha >= beta { 
                        // Update heuristics for quiet moves
                        if (m.flags() & flags::CAPTURE) == 0 {
                            // Store killer moves
                            if ply < 128 {
                                self.killer_moves[ply as usize][1] = self.killer_moves[ply as usize][0];
                                self.killer_moves[ply as usize][0] = Some(m);
                            }
                            // Update history heuristic
                            let bonus = (depth * depth) as i32;
                            let entry = &mut self.history[board.side_to_move.idx()][m.from() as usize][m.to() as usize];
                            *entry = (*entry + bonus).min(100000);
                        }
                        break; 
                    }
                }
            }

            if legal_moves_count == 0 {
                return if board.is_in_check(board.side_to_move) { (None, -MATE_VALUE + ply as i32) } else { (None, 0) };
            }

            let node_type = if max_score <= old_alpha { NodeType::Alpha } else if max_score >= beta { NodeType::Beta } else { NodeType::Exact };
            self.tt.store(board.hash, depth as u8, max_score, node_type, best_move);

            (best_move, max_score)
        }

    fn quiescence(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: u32) -> i32 {
            let current_nodes = self.nodes.fetch_add(1, Ordering::Relaxed) + 1;
            self.seldepth = self.seldepth.max(ply);
            if (current_nodes & 2047) == 0 { self.check_time(); }
            if self.stop.load(Ordering::Relaxed) { return 0; }

            let stand_pat = evaluate(board);
            if stand_pat >= beta { return beta; }
            if alpha < stand_pat { alpha = stand_pat; }

            let mut moves = movegen::generate_pseudo_legal_moves(board);
            // Only keep captures
            moves.retain(|m| (m.flags() & flags::CAPTURE) != 0);

            for i in 0..moves.len() {
                self.pick_move(&mut moves, i, board, None, 0);
                let m = moves[i];
                let state = board.make_move(m);
                if board.is_in_check(board.side_to_move.opposite()) {
                    board.unmake_move(m, state);
                    continue;
                }
                let score = -self.quiescence(board, -beta, -alpha, ply + 1);
                board.unmake_move(m, state);

                if self.stop.load(Ordering::Relaxed) { return 0; }

                if score >= beta { return beta; }
                if score > alpha { alpha = score; }
            }
            alpha
        }

    fn pick_move(&self, moves: &mut [Move], start: usize, board: &Board, tt_move: Option<Move>, ply: u32) {
            let mut best_idx = start;
            let mut best_score = -1;
            for i in start..moves.len() {
                let score = self.score_move(moves[i], board, tt_move, ply);
                if score > best_score {
                    best_score = score;
                    best_idx = i;
                }
            }
            moves.swap(start, best_idx);
        }

    fn score_move(&self, m: Move, board: &Board, tt_move: Option<Move>, ply: u32) -> i32 {
            if Some(m) == tt_move { return 1000000; }
            let mut score;
            if (m.flags() & flags::CAPTURE) != 0 {
                let attacker = board.pieces[m.from() as usize].unwrap_or(PieceType::Pawn);
                let victim = board.pieces[m.to() as usize].unwrap_or(PieceType::Pawn);
                score = 10000 + 10 * self.val(victim) - self.val(attacker);
            } else {
                // Killer moves
                if ply < 128 {
                    if Some(m) == self.killer_moves[ply as usize][0] { return 9000; }
                    if Some(m) == self.killer_moves[ply as usize][1] { return 8000; }
                }
                // History heuristic
                score = self.history[board.side_to_move.idx()][m.from() as usize][m.to() as usize] / 10;
            }
            
            // Promotion
            match m.flags() {
                flags::PROMOTE_QUEEN | flags::PROMOTE_QUEEN_CAPTURE => score += 9000,
                flags::PROMOTE_ROOK | flags::PROMOTE_ROOK_CAPTURE => score += 5000,
                flags::PROMOTE_BISHOP | flags::PROMOTE_BISHOP_CAPTURE => score += 3300,
                flags::PROMOTE_KNIGHT | flags::PROMOTE_KNIGHT_CAPTURE => score += 3200,
                _ => {}
            }
            
            score
        }


    fn val(&self, pt: PieceType) -> i32 {
            match pt {
                PieceType::Pawn => 1, PieceType::Knight => 3, PieceType::Bishop => 3,
                PieceType::Rook => 5, PieceType::Queen => 9, PieceType::King => 0,
            }
        }

        fn check_time(&mut self) {
            if let Some(limit) = self.time_limit {
                if self.start_time.elapsed() >= limit { self.stop.store(true, Ordering::Relaxed); }
            }
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
        // Mate in one position: White to move
        // White: Kh1, Ra7, Ra8
        // Black: Kh8
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
            pieces: [None; 64],
            colors: [None; 64],
            hash: 0,
        };
        board.put_piece(7, crate::board::piece::PieceType::King, crate::board::piece::Color::White); // h1
        board.put_piece(48, crate::board::piece::PieceType::Rook, crate::board::piece::Color::White); // a7
        board.put_piece(49, crate::board::piece::PieceType::Rook, crate::board::piece::Color::White); // b7
        board.put_piece(63, crate::board::piece::PieceType::King, crate::board::piece::Color::Black); // h8
        board.update_occupancy();
        board.hash = board.compute_hash();

        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);
        let result = searcher.search(&mut board, 3, None, 1);
        
        assert!(result.best_move.is_some());
        assert_eq!(result.depth, 3);
        assert!(!result.info_lines.is_empty());
        // Looking for something that delivers mate, e.g., Ra8 (sq 56) or similar
        // Let's just check if it finds a winning move (score should be MATE_VALUE)
        assert!(result.score >= MATE_VALUE - 100);
    }

    #[test]
    fn test_search_depth_with_time_limit() {
        init_all();
        let mut board = Board::startpos();
        let tt = Arc::new(TranspositionTable::new(1));
        let mut searcher = Searcher::new(tt);
        
        // Search for 500ms, should definitely reach more than depth 1
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
        
        // Multithreaded search should return a valid result from TT
        let result = searcher.search(&mut board, 6, None, 2);
        
        assert!(result.best_move.is_some());
        // Since we probe TT, depth might be 6 or more (due to asymmetric search)
        assert!(result.depth >= 6);
    }
}
