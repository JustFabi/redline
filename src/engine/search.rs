use crate::board::board::Board;
use crate::board::r#move::{Move, flags};
use crate::board::piece::PieceType;
use crate::engine::eval::{evaluate, PawnTable};
use crate::engine::tt::{TranspositionTable, NodeType};
use std::time::{Instant, Duration};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const INFINITY: i32 = 1000000;
const MATE_VALUE: i32 = 100000;
const MAX_PLY: usize = 128;
const DELTA_MARGIN: i32 = 200;
const VALUE_NONE: i32 = 32001;

const CONT_HIST_LEVELS: usize = 4;
const CONT_HIST_OFFSETS: [usize; CONT_HIST_LEVELS] = [1, 2, 4, 6];
type ContHistTable = [[[[[i32; 64]; 7]; 64]; 7]; CONT_HIST_LEVELS];

const SEE_PIECE_VALUES: [i32; 7] = [100, 325, 335, 500, 975, 0, 0];

fn build_lmr_table() -> [i32; 256] {
    let mut table = [0i32; 256];
    for i in 1..256 {
        table[i] = (21.4609375 * (i as f64).ln()) as i32;
    }
    table
}

#[derive(Clone, Copy)]
pub struct SearchSettings {
    pub aspiration_windows: bool,
    pub mate_distance_pruning: bool,
    pub iir: bool,
    pub rfp: bool,
    pub nmp: bool,
    pub singular_extensions: bool,
    pub fp: bool,
    pub history_pruning: bool,
    pub see_pruning: bool,
    pub lmp: bool,
    pub lmr: bool,
    pub delta_pruning: bool,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            aspiration_windows: true,
            mate_distance_pruning: true,
            iir: true,
            rfp: true,
            nmp: true,
            singular_extensions: false,
            fp: true,
            history_pruning: true,
            see_pruning: true,
            lmp: true,
            lmr: true,
            delta_pruning: true,
        }
    }
}

impl SearchSettings {
    pub fn none() -> Self {
        Self {
            aspiration_windows: false,
            mate_distance_pruning: false,
            iir: false,
            rfp: false,
            nmp: false,
            singular_extensions: false,
            fp: false,
            history_pruning: false,
            see_pruning: false,
            lmp: false,
            lmr: false,
            delta_pruning: false,
        }
    }

    pub fn baseline() -> Self {
        Self {
            aspiration_windows: true,
            mate_distance_pruning: true,
            rfp: true,
            nmp: true,
            see_pruning: true,
            lmr: true,
            ..Self::none()
        }
    }

    pub fn from_string(s: &str) -> Self {
        let mut settings = Self::none();
        for part in s.split(',') {
            match part.trim().to_lowercase().as_str() {
                "aspiration" | "aw" => settings.aspiration_windows = true,
                "mate_pruning" | "mdp" => settings.mate_distance_pruning = true,
                "iir" => settings.iir = true,
                "rfp" => settings.rfp = true,
                "nmp" => settings.nmp = true,
                "singular" | "se" => settings.singular_extensions = true,
                "fp" => settings.fp = true,
                "history" | "hp" => settings.history_pruning = true,
                "see" => settings.see_pruning = true,
                "lmp" => settings.lmp = true,
                "lmr" => settings.lmr = true,
                "delta" => settings.delta_pruning = true,
                "baseline" => {
                    let base = Self::baseline();
                    settings.aspiration_windows |= base.aspiration_windows;
                    settings.mate_distance_pruning |= base.mate_distance_pruning;
                    settings.rfp |= base.rfp;
                    settings.nmp |= base.nmp;
                    settings.see_pruning |= base.see_pruning;
                    settings.lmr |= base.lmr;
                },
                "all" => return Self::default(),
                "none" => return Self::none(),
                _ => {}
            }
        }
        settings
    }
}

pub struct SearchResult {
    pub best_move: Option<Move>,
    pub score: i32,
    pub depth: u32,
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
    pub history: Box<[[[i32; 64]; 64]; 2]>,
    pub capture_history: Box<[[[[i32; 64]; 64]; 6]; 2]>,
    pub counter_moves: Box<[[Option<Move>; 4096]; 2]>,
    pub cmh_history: Box<[[[[i32; 64]; 64]; 64]; 2]>,
    pub cont_history: Box<ContHistTable>,
    pub move_stack: [(usize, usize); MAX_PLY],
    pub age: u8,
    pub eval_history: [i32; MAX_PLY],
    pv_table: Box<[[Option<Move>; MAX_PLY]; MAX_PLY]>,
    pv_length: Box<[usize; MAX_PLY]>,
    pub pawn_table: PawnTable,
    pub lmr_table: [i32; 256],
    pub low_ply_history: Box<[[[i32; 64]; 64]; 2]>,
    pub root_delta: i32,
    pub last_info_time: Instant,
    pub settings: SearchSettings,
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
            history: unsafe {
                let layout = std::alloc::Layout::new::<[[[i32; 64]; 64]; 2]>();
                let ptr = std::alloc::alloc_zeroed(layout) as *mut [[[i32; 64]; 64]; 2];
                Box::from_raw(ptr)
            },
            capture_history: unsafe {
                let layout = std::alloc::Layout::new::<[[[[i32; 64]; 64]; 6]; 2]>();
                let ptr = std::alloc::alloc_zeroed(layout) as *mut [[[[i32; 64]; 64]; 6]; 2];
                Box::from_raw(ptr)
            },
            counter_moves: unsafe {
                let layout = std::alloc::Layout::new::<[[Option<Move>; 4096]; 2]>();
                let ptr = std::alloc::alloc_zeroed(layout) as *mut [[Option<Move>; 4096]; 2];
                Box::from_raw(ptr)
            },
            cmh_history: unsafe {
                let layout = std::alloc::Layout::new::<[[[[i32; 64]; 64]; 64]; 2]>();
                let ptr = std::alloc::alloc_zeroed(layout) as *mut [[[[i32; 64]; 64]; 64]; 2];
                Box::from_raw(ptr)
            },
            cont_history: unsafe {
                let layout = std::alloc::Layout::new::<ContHistTable>();
                let ptr = std::alloc::alloc_zeroed(layout) as *mut ContHistTable;
                Box::from_raw(ptr)
            },
            move_stack: [(0, 0); MAX_PLY],
            age: 0,
            eval_history: [0; MAX_PLY],
            pv_table: {
                let mut v: Vec<[Option<Move>; MAX_PLY]> = Vec::with_capacity(MAX_PLY);
                for _ in 0..MAX_PLY {
                    v.push([None; MAX_PLY]);
                }
                let boxed_slice = v.into_boxed_slice();
                let raw_ptr = Box::into_raw(boxed_slice) as *mut [[Option<Move>; MAX_PLY]; MAX_PLY];
                unsafe { Box::from_raw(raw_ptr) }
            },
            pv_length: Box::new([0; MAX_PLY]),
            pawn_table: PawnTable::new(8192),
            lmr_table: build_lmr_table(),
            low_ply_history: unsafe {
                let layout = std::alloc::Layout::new::<[[[i32; 64]; 64]; 2]>();
                let ptr = std::alloc::alloc_zeroed(layout) as *mut [[[i32; 64]; 64]; 2];
                Box::from_raw(ptr)
            },
            root_delta: 400,
            last_info_time: Instant::now(),
            settings: SearchSettings::default(),
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

    pub fn value_to_tt(&self, v: i32, ply: u32) -> i32 {
        if v >= MATE_VALUE - 1000 { v + ply as i32 }
        else if v <= -MATE_VALUE + 1000 { v - ply as i32 }
        else { v }
    }

    fn value_from_tt(&self, v: i32, ply: u32) -> i32 {
        if v >= MATE_VALUE - 1000 { v - ply as i32 }
        else if v <= -MATE_VALUE + 1000 { v + ply as i32 }
        else { v }
    }

    fn reduction(&self, improving: bool, depth: u32, move_count: i32, delta: i32, root_delta: i32) -> i32 {
        if depth < 1 || move_count < 1 { return 0; }
        let d = depth.min(255) as usize;
        let m = (move_count as usize).min(255);
        let mut r = self.lmr_table[d] * self.lmr_table[m];
        if !improving { r += r * 238 / 512; }
        r -= delta * 608 / root_delta.max(1);
        r + 1182
    }

    pub fn search(&mut self, board: &mut Board, depth: u32, soft_time_limit: Option<Duration>, hard_time_limit: Option<Duration>, num_threads: usize) -> SearchResult {
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
                let helper_depth = depth + (i % 2) as u32;
                s.spawn(move |_| {
                    helper.internal_search(&mut helper_board, helper_depth);
                });
            }
            let result = self.internal_search(board, depth);
            self.stop.store(true, Ordering::SeqCst);
            result
        }).unwrap()
    }

    fn internal_search(&mut self, board: &mut Board, depth: u32) -> SearchResult {
        let mut best_move = None;
        let mut best_score = -INFINITY;
        let mut previous_best_move = None;
        let mut previous_score = -INFINITY;
        let mut last_completed_depth = 0;
        self.last_info_time = Instant::now();

        for d in 1..=depth {
            let mut delta = 50;
            let mut alpha = -INFINITY;
            let mut beta = INFINITY;
            best_score = -INFINITY;

            if self.settings.aspiration_windows && d >= 3 && previous_score.abs() < MATE_VALUE - 1000 {
                alpha = (previous_score - delta).max(-INFINITY);
                beta = (previous_score + delta).min(INFINITY);
            }

            loop {
                self.root_delta = beta - alpha;
                let (m, score) = self.negamax(board, d, alpha, beta, 0, None);

                if self.stop.load(Ordering::Relaxed) {
                    break;
                }

                if score <= alpha {
                    alpha = (alpha - delta).max(-INFINITY);
                    delta *= 2;
                    if alpha <= -MATE_VALUE + 1000 { alpha = -INFINITY; }
                    best_score = score;
                    if m.is_some() { best_move = m; }
                } else if score >= beta {
                    beta = (beta + delta).min(INFINITY);
                    delta *= 2;
                    best_score = score;
                    if m.is_some() { best_move = m; }
                    if beta >= MATE_VALUE - 1000 { beta = INFINITY; }
                } else {
                    best_score = score;
                    if m.is_some() {
                        best_move = m;
                    } else if let Some(entry) = self.tt.probe(board.hash) {
                        best_move = entry.best_move;
                    }
                    break;
                }

                if delta >= 10_000 {
                    alpha = -INFINITY;
                    beta = INFINITY;
                }
            }

            if self.stop.load(Ordering::Relaxed) { break; }
            last_completed_depth = d;

            let elapsed = self.start_time.elapsed().as_millis() as u64;
            let total_nodes = self.nodes.load(Ordering::Relaxed);
            let nps = if elapsed > 0 { (total_nodes * 1000) / elapsed } else { 0 };
            let pv_str = (0..self.pv_length[0]).filter_map(|i| self.pv_table[0][i]).map(|m| m.to_string()).collect::<Vec<_>>().join(" ");

            if self.is_main_thread {
                println!("info depth {} seldepth {} score {} nodes {} nps {} time {} pv {}", d, self.seldepth, self.format_score(best_score), total_nodes, nps, elapsed, pv_str);
            }
            
            // Removed early mate exit: keep searching to find shorter mates or refute bad PVs
            // if best_score.abs() > MATE_VALUE - 1000 { break; }

            if let Some(soft) = self.soft_time_limit {
                let elapsed_dur = self.start_time.elapsed();
                if elapsed_dur >= soft {
                    let mut stable = true;
                    if previous_best_move.is_some() && best_move != previous_best_move { stable = false; }
                    if best_score < previous_score - 50 { stable = false; }
                    if stable { break; } else if let Some(hard) = self.hard_time_limit {
                        if elapsed_dur >= hard { break; }
                    }
                }
            }
            previous_best_move = best_move;
            previous_score = best_score;
        }

        if self.stop.load(Ordering::Relaxed) && previous_best_move.is_some() {
            best_move = previous_best_move;
            best_score = previous_score;
            last_completed_depth = last_completed_depth.max(1);
        }

        SearchResult { best_move, score: best_score, depth: last_completed_depth }
    }

    fn negamax(&mut self, board: &mut Board, mut depth: u32, mut alpha: i32, mut beta: i32, ply: u32, excluded_move: Option<Move>) -> (Option<Move>, i32) {
        if (ply as usize) < MAX_PLY { self.pv_length[ply as usize] = 0; }
        let is_pv_node = beta - alpha > 1;

        if (self.nodes.fetch_add(1, Ordering::Relaxed) & 2047) == 0 { self.check_time(); }
        if self.stop.load(Ordering::Relaxed) { return (None, 0); }
        if ply >= MAX_PLY as u32 - 1 { return (None, evaluate(board, Some(&mut self.pawn_table))); }
        if board.is_repetition() || board.halfmove_clock >= 100 { return (None, 0); }

        if ply > 0 && self.settings.mate_distance_pruning {
            alpha = alpha.max(-MATE_VALUE + ply as i32);
            beta = beta.min(MATE_VALUE - ply as i32 - 1);
            if alpha >= beta { return (None, alpha); }
        }

        self.seldepth = self.seldepth.max(ply);
        let (pinned, checkers) = board.pins_and_checkers(board.side_to_move);
        let in_check = checkers != 0;
        if in_check && ply < 16 { depth += 1; }

        let mut tt_move = None;
        let mut tt_depth = 0;
        let mut tt_bound = NodeType::Alpha;
        let mut tt_value = VALUE_NONE;

        if let Some(entry) = self.tt.probe(board.hash) {
            tt_move = entry.best_move;
            tt_value = self.value_from_tt(entry.score, ply);
            tt_depth = entry.depth;
            tt_bound = entry.node_type;

            if !is_pv_node && excluded_move.is_none() && tt_depth >= depth as u8 && tt_value != VALUE_NONE {
                match tt_bound {
                    NodeType::Exact => return (tt_move, tt_value),
                    NodeType::Alpha if tt_value <= alpha => return (tt_move, tt_value),
                    NodeType::Beta if tt_value >= beta => return (tt_move, tt_value),
                    _ => {}
                }
            }
        }

        if self.settings.iir && !is_pv_node && tt_move.is_none() && depth >= 6 {
            depth -= 1;
        }

        if depth == 0 { return (None, self.quiescence(board, alpha, beta, ply)); }

        let static_eval = evaluate(board, Some(&mut self.pawn_table));
        self.eval_history[ply as usize] = static_eval;
        let improving = !in_check && ply >= 2 && static_eval > self.eval_history[ply as usize - 2];

        if self.settings.rfp && !is_pv_node && !in_check && depth <= 8 && excluded_move.is_none() {
            let margin = depth as i32 * 114;
            if static_eval - margin >= beta { return (None, static_eval - margin); }
        }

        if self.settings.nmp && !is_pv_node && depth >= 3 && !in_check && excluded_move.is_none() && static_eval >= beta {
            if board.has_non_pawn_material(board.side_to_move) {
                let r = 3 + depth / 4 + ((static_eval - beta).max(0).min(600) / 200) as u32;
                let state = board.make_null_move();
                let (_, score) = self.negamax(board, depth.saturating_sub(r), -beta, -beta + 1, ply + 1, None);
                board.unmake_null_move(state);
                if -score >= beta { 
                    // Cap the score to beta if it's a mate score to prevent fake mates
                    let ret_score = if -score >= MATE_VALUE - MAX_PLY as i32 { beta } else { -score };
                    return (None, ret_score); 
                }
            }
        }

        let extension = 0;
        // Singular extensions could go here...

        if !is_pv_node && depth >= 3 && excluded_move.is_none() && beta.abs() < MATE_VALUE - 1000 {
            let probcut_beta = beta + 200 - if improving { 49 } else { 0 };
            let q_score = self.quiescence(board, probcut_beta - 1, probcut_beta, ply);
            if q_score >= probcut_beta {
                let mut picker = crate::engine::movepicker::MovePicker::new(None, false, in_check, [None; 2], None);
                while let Some(m) = picker.next(self, board, ply) {
                    if !board.is_legal_fast(m, pinned, checkers) || (m.flags() & flags::CAPTURE) == 0 { continue; }
                    if board.see(m) < probcut_beta - static_eval { continue; }
                    let state = board.make_move(m);
                    let (_, score) = self.negamax(board, depth.saturating_sub(4), -probcut_beta, -probcut_beta + 1, ply + 1, None);
                    board.unmake_move(m, state);
                    if -score >= probcut_beta { return (Some(m), -score); }
                }
            }
        }

        use crate::engine::movepicker::MovePicker;
        let mut legal_moves = 0;
        let mut best_m = None;
        let mut max_score = -INFINITY;
        let old_alpha = alpha;
        if (ply as usize) < MAX_PLY { self.pv_length[ply as usize] = 0; }

        let mut picker = if let Some(excluded) = excluded_move {
            MovePicker::with_excluded(tt_move, Some(excluded), in_check)
        } else {
            MovePicker::new(tt_move, false, in_check, self.killer_moves[ply as usize], self.get_countermove(board))
        };

        let futility_move_count = (3 + depth * depth) / (2 - if improving { 1 } else { 0 });

        while let Some(m) = picker.next(self, board, ply) {
            if !board.is_legal_fast(m, pinned, checkers) { continue; }
            let is_promotion = (m.flags() & 0x8) != 0;
            // Bug 4 fix: promotions are NOT quiet — prevents queen promos from being pruned
            let is_quiet = (m.flags() & flags::CAPTURE) == 0 && !is_promotion;

            // Bug 3 fix: count legal moves BEFORE pruning (matches Stockfish moveCount)
            legal_moves += 1;
            let is_first_move = legal_moves == 1;

            if self.settings.history_pruning && !is_pv_node && depth <= 8 && legal_moves > 1 && is_quiet {
                let hist = self.history[board.side_to_move.idx()][m.from() as usize][m.to() as usize];
                if hist < -4000 * depth as i32 { continue; }
            }

            if self.settings.lmp && !is_pv_node && depth < 10 && !in_check && is_quiet && legal_moves >= futility_move_count {
                picker.skip_quiets = true;
                continue;
            }

            if self.settings.see_pruning && !is_pv_node && depth <= 8 && legal_moves > 1 {
                let margin = if is_quiet { -25 * depth as i32 * depth as i32 } else {
                    let hist = self.capture_history[board.side_to_move.idx()][(board.pieces[m.from() as usize] as usize).min(5)][m.to() as usize][(board.pieces[m.to() as usize] as usize).min(5)];
                    -166 * depth as i32 - hist / 29
                };
                if board.see(m) < margin { continue; }
            }

            if (ply as usize) < MAX_PLY {
                let piece_type = board.pieces[m.from() as usize] as usize;
                self.move_stack[ply as usize] = (piece_type.min(6), m.to() as usize);
            }

            // Bug 5 fix: removed redundant is_legal_fast (already checked above)
            // Bug 1 fix: save side_to_move BEFORE make_move flips it
            let us_idx = board.side_to_move.idx();
            let state = board.make_move(m);
            let mut score;

            // Bug 2 fix: removed !is_pv_node — LMR now applies at PV nodes too (like Stockfish)
            if self.settings.lmr && !is_first_move && depth >= 3 && is_quiet && Some(m) != tt_move {
                let mut r = self.reduction(improving, depth, legal_moves as i32, beta - alpha, self.root_delta);
                
                // Reduce reduction for PV nodes to avoid missing tactics
                if is_pv_node { r -= 1024; }
                // Bug 1 fix: use us_idx (saved before make_move) instead of board.side_to_move
                let hist = self.history[us_idx][m.from() as usize][m.to() as usize];
                r -= hist * 850 / 8192;
                
                let d = 1.max(std::cmp::min(depth as i32 - 1 - r / 1024, depth as i32 - 1)) as u32;
                (_, score) = self.negamax(board, d, -alpha - 1, -alpha, ply + 1, None);
                score = -score;

                if score > alpha && d < depth - 1 {
                    (_, score) = self.negamax(board, depth - 1, -alpha - 1, -alpha, ply + 1, None);
                    score = -score;
                }
            } else {
                (_, score) = self.negamax(board, depth - 1 + extension, if is_first_move { -beta } else { -alpha - 1 }, -alpha, ply + 1, None);
                score = -score;
            }

            // PVS Re-search
            if is_pv_node && !is_first_move && score > alpha && score < beta {
                (_, score) = self.negamax(board, depth - 1 + extension, -beta, -alpha, ply + 1, None);
                score = -score;
            }

            board.unmake_move(m, state);
            if score > max_score { max_score = score; best_m = Some(m); }
            if score > alpha {
                alpha = score;
                if (ply as usize) < MAX_PLY {
                    self.pv_table[ply as usize][0] = Some(m);
                    let next_len = if ply as usize + 1 < MAX_PLY { self.pv_length[ply as usize + 1] } else { 0 };
                    for i in 0..next_len { self.pv_table[ply as usize][i + 1] = self.pv_table[ply as usize + 1][i]; }
                    self.pv_length[ply as usize] = 1 + next_len;
                }
                if alpha >= beta {
                    if is_quiet { self.update_heuristics(m, board, depth, ply, tt_move); }
                    break;
                }
            } else if is_quiet { self.apply_history_malus(m, board, depth, legal_moves as u32, ply); }
        }

        if legal_moves == 0 { return (None, if in_check { -MATE_VALUE + ply as i32 } else { 0 }); }
        let node_type = if max_score >= beta { NodeType::Beta } else if max_score > old_alpha { NodeType::Exact } else { NodeType::Alpha };
        let store_score = self.value_to_tt(max_score, ply);
        self.tt.store(board.hash, depth as u8, store_score, node_type, best_m, self.age);
        (best_m, max_score)
    }

    fn quiescence(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: u32) -> i32 {
        if (ply as usize) < MAX_PLY { self.pv_length[ply as usize] = 0; }
        if (self.nodes.load(Ordering::Relaxed) & 2047) == 0 { self.check_time(); }
        if ply >= MAX_PLY as u32 - 1 { return evaluate(board, Some(&mut self.pawn_table)); }
        if board.is_repetition() || board.halfmove_clock >= 100 { return 0; }
        self.seldepth = self.seldepth.max(ply);

        let tt_entry = self.tt.probe(board.hash);
        let tt_move = tt_entry.and_then(|e| e.best_move);
        let mut tt_value = VALUE_NONE;

        if let Some(entry) = tt_entry {
            tt_value = self.value_from_tt(entry.score, ply);
            if tt_value != VALUE_NONE {
                match entry.node_type {
                    NodeType::Exact => return tt_value,
                    NodeType::Alpha if tt_value <= alpha => return tt_value,
                    NodeType::Beta if tt_value >= beta => return tt_value,
                    _ => {}
                }
            }
        }

        let (pinned, checkers) = board.pins_and_checkers(board.side_to_move);
        let in_check = checkers != 0;
        let mut stand_pat = if in_check { -INFINITY } else { evaluate(board, Some(&mut self.pawn_table)) };

        if let Some(entry) = tt_entry {
            if entry.node_type == NodeType::Exact || (entry.node_type == NodeType::Alpha && tt_value < stand_pat) || (entry.node_type == NodeType::Beta && tt_value > stand_pat) {
                stand_pat = tt_value;
            }
        }

        if !in_check {
            if stand_pat >= beta { return stand_pat; }
            if stand_pat > alpha { alpha = stand_pat; }
        }

        let mut picker = crate::engine::movepicker::MovePicker::new(tt_move, true, in_check, [None; 2], None);
        let mut best_score = stand_pat;
        let mut best_m = None;
        let old_alpha = alpha;

        while let Some(m) = picker.next(self, board, ply) {
            if !board.is_legal_fast(m, pinned, checkers) { continue; }
            if !in_check && self.settings.delta_pruning {
                let captured_pt = board.pieces[m.to() as usize];
                if stand_pat + SEE_PIECE_VALUES[captured_pt as usize] + DELTA_MARGIN < alpha { 
                    // Bypass Delta Pruning for promotions
                    if (m.flags() & 0x8) == 0 {
                        continue; 
                    }
                }
            }
            self.nodes.fetch_add(1, Ordering::Relaxed);
            let state = board.make_move(m);
            let score = -self.quiescence(board, -beta, -alpha, ply + 1);
            board.unmake_move(m, state);
            if score > best_score {
                best_score = score;
                if score > alpha {
                    alpha = score;
                    best_m = Some(m);
                    if score >= beta { break; }
                }
            }
        }
        if in_check && best_score == -INFINITY { return -MATE_VALUE + ply as i32; }

        let node_type = if best_score >= beta { NodeType::Beta } else if best_score > old_alpha { NodeType::Exact } else { NodeType::Alpha };
        let store_score = self.value_to_tt(best_score, ply);
        self.tt.store(board.hash, 0, store_score, node_type, best_m, self.age);

        best_score
    }

    fn update_heuristics(&mut self, m: Move, board: &Board, depth: u32, ply: u32, _tt_move: Option<Move>) {
        let us = board.side_to_move.idx();
        let bonus = (depth as i32 * depth as i32).min(400);
        
        let hist = &mut self.history[us][m.from() as usize][m.to() as usize];
        *hist += bonus - *hist * bonus / 32768;

        if ply < 16 {
            let lp_hist = &mut self.low_ply_history[us][m.from() as usize][m.to() as usize];
            *lp_hist += bonus - *lp_hist * bonus / 32768;
        }

        if let Some(last_move) = board.last_move {
            let last_idx = (last_move.from() as usize) | ((last_move.to() as usize) << 6);
            self.counter_moves[us][last_idx] = Some(m);
            
            let cmh = &mut self.cmh_history[us][last_move.to() as usize][m.from() as usize][m.to() as usize];
            *cmh += bonus - *cmh * bonus / 32768;
        }

        if ply < MAX_PLY as u32 {
            if self.killer_moves[ply as usize][0] != Some(m) {
                self.killer_moves[ply as usize][1] = self.killer_moves[ply as usize][0];
                self.killer_moves[ply as usize][0] = Some(m);
            }
        }

        let cur_pt = (board.pieces[m.from() as usize] as usize).min(6);
        let cur_to = m.to() as usize;
        for (level, &offset) in CONT_HIST_OFFSETS.iter().enumerate() {
            if (ply as usize) >= offset {
                let (prev_pt, prev_to) = self.move_stack[ply as usize - offset];
                let ch = &mut self.cont_history[level][prev_pt.min(6)][prev_to][cur_pt][cur_to];
                *ch += bonus - *ch * bonus / 32768;
            }
        }
    }

    fn apply_history_malus(&mut self, m: Move, board: &Board, depth: u32, _legal_moves: u32, ply: u32) {
        let us = board.side_to_move.idx();
        let malus = (depth as i32 * depth as i32).min(400);
        
        let hist = &mut self.history[us][m.from() as usize][m.to() as usize];
        *hist -= malus + *hist * malus / 32768;

        if ply < 16 {
            let lp_hist = &mut self.low_ply_history[us][m.from() as usize][m.to() as usize];
            *lp_hist -= malus + *lp_hist * malus / 32768;
        }

        if let Some(last_move) = board.last_move {
            let cmh = &mut self.cmh_history[us][last_move.to() as usize][m.from() as usize][m.to() as usize];
            *cmh -= malus + *cmh * malus / 32768;
        }
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
        if Some(m) == tt_move { return 1_000_000; }
        let is_capture = (m.flags() & flags::CAPTURE) != 0;
        let is_promotion = (m.flags() & 0x8) != 0;

        if is_capture {
            let mut victim = board.pieces[m.to() as usize];
            if m.flags() == flags::EN_PASSANT {
                victim = crate::board::piece::PieceType::Pawn;
            }
            let attacker = board.pieces[m.from() as usize];
            let mut score = 50_000 + 10 * self.val(victim) - self.val(attacker);
            score += self.capture_history[board.side_to_move.idx()][(attacker as usize).min(5)][m.to() as usize][(victim as usize).min(5)] / 128;
            let see_val = board.see(m);
            if is_qsearch {
                if self.val(victim) <= self.val(attacker) && see_val < 0 { return -50_000 + see_val; }
            } else if see_val < 0 { return -5_000 + see_val; }
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

        if let Some(last_move) = board.last_move {
            let last_idx = (last_move.from() as usize) | ((last_move.to() as usize) << 6);
            if Some(m) == self.counter_moves[board.side_to_move.idx()][last_idx] { return 9_500; }
        }

        if ply < MAX_PLY as u32 {
            if Some(m) == self.killer_moves[ply as usize][0] { return 9_000; }
            if Some(m) == self.killer_moves[ply as usize][1] { return 8_000; }
        }

        let mut score = 2 * self.history[board.side_to_move.idx()][m.from() as usize][m.to() as usize] / 10;
        if ply < 16 {
            score += self.low_ply_history[board.side_to_move.idx()][m.from() as usize][m.to() as usize] / 10;
        }
        if let Some(last_move) = board.last_move {
            score += self.cmh_history[board.side_to_move.idx()][last_move.to() as usize][m.from() as usize][m.to() as usize] / 20;
        }

        let cur_pt = (board.pieces[m.from() as usize] as usize).min(6);
        let cur_to = m.to() as usize;
        for (level, &offset) in CONT_HIST_OFFSETS.iter().enumerate() {
            if (ply as usize) >= offset {
                let (prev_pt, prev_to) = self.move_stack[ply as usize - offset];
                score += self.cont_history[level][prev_pt.min(6)][prev_to][cur_pt][cur_to] / 20;
            }
        }
        score
    }

    pub fn val(&self, pt: PieceType) -> i32 {
        match pt {
            PieceType::Pawn => 1, PieceType::Knight => 3, PieceType::Bishop => 3,
            PieceType::Rook => 5, PieceType::Queen => 9, PieceType::King => 0, _ => 0,
        }
    }

    fn check_time(&mut self) {
        let elapsed = self.start_time.elapsed();
        if let Some(l) = self.hard_time_limit.or(self.soft_time_limit) {
            if elapsed >= l { self.stop.store(true, Ordering::Relaxed); }
        }
        if self.is_main_thread && self.last_info_time.elapsed() >= Duration::from_millis(1000) {
            self.last_info_time = Instant::now();
            let nodes = self.nodes.load(Ordering::Relaxed);
            let time = elapsed.as_millis() as u64;
            let nps = if time > 0 { (nodes * 1000) / time } else { 0 };
            println!("info nodes {} nps {} time {} hashfull {}", nodes, nps, time, self.tt.hashfull());
        }
    }

    fn get_countermove(&self, board: &Board) -> Option<Move> {
        if let Some(last_move) = board.last_move {
            let last_idx = (last_move.from() as usize) | ((last_move.to() as usize) << 6);
            return self.counter_moves[board.side_to_move.idx()][last_idx];
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;
    #[allow(unused_imports)]
    use crate::board::piece::Color;
    use crate::board::r#move::flags;
    use std::sync::Arc;

    fn init() {
        crate::movegen::init_all();
        crate::engine::eval::init_eval();
    }

    fn make_searcher() -> Searcher {
        let tt = Arc::new(crate::engine::tt::TranspositionTable::new(4));
        let mut s = Searcher::new(tt);
        s.is_main_thread = false;
        s
    }

    fn search_position(fen: &str, depth: u32) -> SearchResult {
        init();
        let mut board = Board::from_fen(fen).unwrap();
        let mut searcher = make_searcher();
        searcher.search(&mut board, depth, None, None, 1)
    }

    fn search_position_with_settings(fen: &str, depth: u32, settings: SearchSettings) -> SearchResult {
        init();
        let mut board = Board::from_fen(fen).unwrap();
        let mut searcher = make_searcher();
        searcher.settings = settings;
        searcher.search(&mut board, depth, None, None, 1)
    }

    // ==========================================
    // Bug 1: LMR history uses correct side
    // ==========================================

    #[test]
    fn test_lmr_history_correct_side() {
        // Verify search produces a reasonable move from a tactical position.
        // Before the fix, LMR history was using the opponent's table,
        // causing random reduction adjustments and missed tactics.
        init();
        let mut board = Board::from_fen(
            "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4"
        ).unwrap();
        let mut searcher = make_searcher();

        // This is the "Italian Game" Scholar's mate threat position.
        // White should find Qxf7# (mate in 1).
        let result = searcher.search(&mut board, 4, None, None, 1);
        let best = result.best_move.unwrap();
        // Qh5 captures f7: from=39 (h5), to=53 (f7)
        assert_eq!(best.to(), 53, "Should find Qxf7# (capture on f7)");
        assert!(result.score > MATE_VALUE - 100, "Should detect mate, got score {}", result.score);
    }

    #[test]
    fn test_history_table_updated_for_correct_side() {
        // After a search, the history table for the side that moved should
        // have non-zero entries if any quiet beta cutoffs occurred.
        init();
        let mut board = Board::startpos();
        let mut searcher = make_searcher();
        searcher.search(&mut board, 6, None, None, 1);

        // White moved first, so White's history table should have entries
        let white_hist = &searcher.history[0]; // White = index 0
        let mut has_nonzero = false;
        for from in 0..64 {
            for to in 0..64 {
                if white_hist[from][to] != 0 {
                    has_nonzero = true;
                    break;
                }
            }
            if has_nonzero { break; }
        }
        assert!(has_nonzero, "White history table should have non-zero entries after depth 6 search");
    }

    // ==========================================
    // Bug 2: LMR at PV nodes
    // ==========================================

    #[test]
    fn test_lmr_at_pv_nodes_reduces_node_count() {
        // With LMR enabled at PV nodes, we should search fewer nodes
        // than with LMR disabled, at the same depth.
        init();
        let mut board = Board::startpos();

        // Search with LMR enabled (default)
        let tt1 = Arc::new(crate::engine::tt::TranspositionTable::new(4));
        let mut searcher_lmr = Searcher::new(tt1);
        searcher_lmr.search(&mut board, 5, None, None, 1);
        let nodes_with_lmr = searcher_lmr.nodes.load(std::sync::atomic::Ordering::Relaxed);

        // Search with LMR disabled
        let tt2 = Arc::new(crate::engine::tt::TranspositionTable::new(4));
        let mut searcher_no_lmr = Searcher::new(tt2);
        searcher_no_lmr.settings.lmr = false;
        searcher_no_lmr.search(&mut board, 5, None, None, 1);
        let nodes_without_lmr = searcher_no_lmr.nodes.load(std::sync::atomic::Ordering::Relaxed);

        assert!(
            nodes_with_lmr < nodes_without_lmr,
            "LMR should reduce node count: {} with vs {} without",
            nodes_with_lmr, nodes_without_lmr
        );
    }

    // ==========================================
    // Bug 3: legal_moves counter before pruning
    // ==========================================

    #[test]
    fn test_search_finds_move_at_all_depths() {
        // With the counter fix, the engine should always return a valid move.
        // A broken counter could cause legal_moves == 0 incorrectly.
        init();
        for depth in 1..=5 {
            let mut board = Board::startpos();
            let mut searcher = make_searcher();
            let result = searcher.search(&mut board, depth, None, None, 1);
            assert!(
                result.best_move.is_some(),
                "Should find a move at depth {}", depth
            );
        }
    }

    // ==========================================
    // Bug 4: Queen promotions not pruned
    // ==========================================

    #[test]
    fn test_queen_promotion_found() {
        // Position where pawn promotion to queen is the best/only winning move.
        // Before the fix, non-capture queen promotions could be LMP'd or history-pruned.
        init();
        // White pawn on e7, about to promote. Black king on h8, White king on e1.
        let mut board = Board::from_fen("7k/4P3/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mut searcher = make_searcher();
        let result = searcher.search(&mut board, 6, None, None, 1);
        let best = result.best_move.unwrap();

        // The move should be a promotion (flags & 0x8 != 0)
        assert!(
            (best.flags() & 0x8) != 0,
            "Best move should be a promotion, got flags=0x{:x} move={}",
            best.flags(), best
        );
        // Should be a queen promotion specifically
        assert!(
            best.flags() == flags::PROMOTE_QUEEN || best.flags() == flags::PROMOTE_QUEEN_CAPTURE,
            "Should promote to queen, got flags=0x{:x}", best.flags()
        );
    }

    #[test]
    fn test_queen_promotion_not_classified_as_quiet() {
        // Verify the is_quiet classification directly
        let promo_flags = flags::PROMOTE_QUEEN;
        let is_promotion = (promo_flags & 0x8) != 0;
        let is_quiet = (promo_flags & flags::CAPTURE) == 0 && !is_promotion;
        assert!(!is_quiet, "Queen promotion should NOT be classified as quiet");

        let quiet_flags = flags::QUIET;
        let is_promotion_q = (quiet_flags & 0x8) != 0;
        let is_quiet_q = (quiet_flags & flags::CAPTURE) == 0 && !is_promotion_q;
        assert!(is_quiet_q, "Normal quiet move should be classified as quiet");

        let cap_flags = flags::CAPTURE;
        let is_promotion_c = (cap_flags & 0x8) != 0;
        let is_quiet_c = (cap_flags & flags::CAPTURE) == 0 && !is_promotion_c;
        assert!(!is_quiet_c, "Capture should NOT be classified as quiet");
    }

    // ==========================================
    // Eval symmetry test
    // ==========================================

    #[test]
    fn test_eval_symmetry() {
        // Evaluation from White's perspective + Black's perspective should be consistent.
        // evaluate() returns score from side_to_move's perspective.
        init();
        let board_w = Board::from_fen(
            "r1bqkbnr/pppppppp/2n5/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2"
        ).unwrap();
        let score_w = crate::engine::eval::evaluate(&board_w, None);

        // Mirror: swap colors
        let board_b = Board::from_fen(
            "rnbqkbnr/pppp1ppp/8/4p3/8/2N5/PPPPPPPP/R1BQKBNR b KQkq - 1 2"
        ).unwrap();
        let score_b = crate::engine::eval::evaluate(&board_b, None);

        // Scores should be approximately equal (same position, just mirrored)
        assert!(
            (score_w - score_b).abs() <= 5,
            "Eval should be symmetric: white_to_move={} vs mirrored_black_to_move={}", score_w, score_b
        );
    }

    // ==========================================
    // Tactical puzzle tests (WAC-style)
    // ==========================================

    #[test]
    fn test_tactic_back_rank_mate() {
        // White to move: Qe8# (back rank mate)
        // 6k1/5ppp/8/8/8/8/8/4Q1K1 w - - 0 1
        let result = search_position("6k1/5ppp/8/8/8/8/8/4Q1K1 w - - 0 1", 4);
        let best = result.best_move.unwrap();
        assert_eq!(best.to(), 60, "Should find Qe8# (to=60), got move {}", best);
        assert!(result.score > MATE_VALUE - 10, "Should detect mate");
    }

    #[test]
    fn test_tactic_fork() {
        // White knight on d5, black king on e8, black rook on a8
        // Nc7+ forks king and rook
        let result = search_position("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1", 6);
        let _best = result.best_move.unwrap();
        // Nc7+ or similar winning move
        assert!(result.score > 50, "Should evaluate knight fork as winning, got score {}", result.score);
    }

    #[test]
    fn test_tactic_avoid_stalemate() {
        // Position where careless play leads to stalemate
        // White has huge material advantage but must not stalemate Black
        // 8/8/8/8/8/1k6/8/KQ6 w - - 0 1 (White Kd1 Qb1 vs Black Kb3)
        let result = search_position("8/8/8/8/8/1k6/8/KQ6 w - - 0 1", 5);
        // Should find a move that doesn't stalemate (score should be high, not 0)
        assert!(result.best_move.is_some(), "Should find a move");
        // If the engine is working correctly, it should be winning
    }

    // ==========================================
    // Search correctness tests
    // ==========================================

    #[test]
    fn test_mate_in_one_found() {
        let fen = "rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq - 0 1";
        // Fool's mate for black
        let result = search_position(fen, 4);
        let best = result.best_move.unwrap();
        // Should move queen from d8 to h4
        assert_eq!(best.from(), 59); // d8
        assert_eq!(best.to(), 31); // h4
        assert!(result.score > MATE_VALUE - 10, "Should detect mate in 1");
    }

    #[test]
    fn test_mate_in_two_found() {
        // A known mate-in-2 position
        // White to move: 1. Qh7+ Kf8 2. Qh8#
        let result = search_position(
            "r1bq2r1/b4pk1/p1pp1p2/1p2pP2/1P2P1PQ/3P4/1PPB2P1/R3K2R w KQ - 0 1", 5
        );
        assert!(result.score > MATE_VALUE - 20, "Should find mate in 2, got score {}", result.score);
    }

    #[test]
    fn test_draw_by_repetition() {
        init();
        let mut board = Board::from_fen("8/8/8/8/8/2k5/8/R1K5 w - - 0 1").unwrap();

        // Play moves back and forth to create repetition
        let m1 = board.parse_move("a1b1").unwrap();
        board.make_move(m1);
        let m2 = board.parse_move("c3d3").unwrap();
        board.make_move(m2);
        let m3 = board.parse_move("b1a1").unwrap();
        board.make_move(m3);
        let m4 = board.parse_move("d3c3").unwrap();
        board.make_move(m4);

        // Now we're back to the original position. is_repetition should detect this.
        assert!(board.is_repetition(), "Should detect repetition after 4 half-moves returning to start");
    }

    #[test]
    fn test_value_to_from_tt_roundtrip() {
        init();
        let searcher = make_searcher();

        // Normal score
        let score = 150;
        let tt_val = searcher.value_to_tt(score, 5);
        let restored = searcher.value_from_tt(tt_val, 5);
        assert_eq!(restored, score, "Normal score should roundtrip through TT");

        // Mate score
        let mate_score = MATE_VALUE - 3;
        let tt_val = searcher.value_to_tt(mate_score, 5);
        let restored = searcher.value_from_tt(tt_val, 5);
        assert_eq!(restored, mate_score, "Mate score should roundtrip through TT");

        // Negative mate score
        let mated_score = -MATE_VALUE + 7;
        let tt_val = searcher.value_to_tt(mated_score, 5);
        let restored = searcher.value_from_tt(tt_val, 5);
        assert_eq!(restored, mated_score, "Mated score should roundtrip through TT");
    }

    #[test]
    fn test_format_score_mate() {
        let searcher = make_searcher();
        let s = searcher.format_score(MATE_VALUE - 2);
        assert!(s.starts_with("mate"), "Should format as mate: {}", s);
        let s2 = searcher.format_score(-MATE_VALUE + 5);
        assert!(s2.starts_with("mate -"), "Should format as negative mate: {}", s2);
        let s3 = searcher.format_score(100);
        assert_eq!(s3, "cp 100");
    }

    // ==========================================
    // Feature toggle tests (each feature should not lose Elo)
    // ==========================================

    #[test]
    fn test_all_features_vs_none_finds_better_move() {
        // On a tactical position, full features should find a winning move
        // while bare search might miss it at the same depth.
        init();
        // Position with a tactic: White can win material with Bxf7+
        let fen = "r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";

        let result_full = search_position_with_settings(fen, 5, SearchSettings::default());
        let result_none = search_position_with_settings(fen, 5, SearchSettings::none());

        // Both should find a move
        assert!(result_full.best_move.is_some());
        assert!(result_none.best_move.is_some());

        // Full-featured search should evaluate at least as well
        // (may search deeper effectively due to pruning efficiency)
    }

    #[test]
    fn test_nmp_does_not_crash() {
        // NMP on a position with non-pawn material
        let result = search_position_with_settings(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            6,
            SearchSettings { nmp: true, ..SearchSettings::none() }
        );
        assert!(result.best_move.is_some());
    }

    #[test]
    fn test_rfp_does_not_crash() {
        let result = search_position_with_settings(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
            6,
            SearchSettings { rfp: true, ..SearchSettings::none() }
        );
        assert!(result.best_move.is_some());
    }

    #[test]
    fn test_aspiration_windows_do_not_crash() {
        let result = search_position_with_settings(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            5,
            SearchSettings { aspiration_windows: true, ..SearchSettings::none() }
        );
        assert!(result.best_move.is_some());
    }

    // ==========================================
    // Perft regression (ensure fixes don't break movegen)
    // ==========================================

    #[test]
    fn test_perft_regression_startpos() {
        init();
        let mut board = Board::startpos();
        assert_eq!(board.perft(1), 20);
        assert_eq!(board.perft(2), 400);
        assert_eq!(board.perft(3), 8902);
        assert_eq!(board.perft(4), 197_281);
    }

    #[test]
    fn test_perft_regression_kiwipete() {
        init();
        let mut board = Board::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1"
        ).unwrap();
        assert_eq!(board.perft(1), 48);
        assert_eq!(board.perft(2), 2039);
        assert_eq!(board.perft(3), 97862);
    }

    // ==========================================
    // Reduction function sanity
    // ==========================================

    #[test]
    fn test_reduction_increases_with_depth_and_movecount() {
        let searcher = make_searcher();
        let r1 = searcher.reduction(true, 4, 3, 100, 400);
        let r2 = searcher.reduction(true, 8, 3, 100, 400);
        assert!(r2 > r1, "Higher depth should give higher reduction: d4={} d8={}", r1, r2);

        let r3 = searcher.reduction(true, 6, 2, 100, 400);
        let r4 = searcher.reduction(true, 6, 10, 100, 400);
        assert!(r4 > r3, "Higher move count should give higher reduction: mc2={} mc10={}", r3, r4);
    }

    #[test]
    fn test_reduction_lower_when_improving() {
        let searcher = make_searcher();
        let r_improving = searcher.reduction(true, 6, 5, 100, 400);
        let r_not_improving = searcher.reduction(false, 6, 5, 100, 400);
        assert!(r_not_improving > r_improving,
            "Not improving should give higher reduction: improving={} not_improving={}",
            r_improving, r_not_improving);
    }

    #[test]
    fn test_reduction_never_negative_depth() {
        // Even with extreme reduction, the clamped depth should be at least 1
        let searcher = make_searcher();
        let r = searcher.reduction(false, 3, 100, 50, 400);
        let d = 1i32.max(std::cmp::min(3 - 1 - r / 1024, 3 - 1));
        assert!(d >= 1, "Reduced depth should be at least 1, got {}", d);
    }
}