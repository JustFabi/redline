use crate::board::board::Board;
use crate::engine::search::Searcher;
use crate::engine::tt::TranspositionTable;
use std::io::{self, BufRead};
use std::time::Duration;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub struct Uci {
    board: Board,
    searcher: Searcher,
    num_threads: usize,
    search_thread: Option<std::thread::JoinHandle<()>>,
}

impl Uci {
    pub fn new() -> Self {
        let tt = Arc::new(TranspositionTable::new(64)); // 64MB default
        Self {
            board: Board::startpos(),
            searcher: Searcher::new(tt),
            num_threads: 1,
            search_thread: None,
        }
    }

    pub fn loop_communication(&mut self) {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.unwrap();
            let responses = self.process_command(&line);
            for response in responses {
                println!("{}", response);
            }
            if line.trim() == "quit" {
                break;
            }
        }
    }

    pub fn process_command(&mut self, command: &str) -> Vec<String> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return vec![];
        }

        match parts[0] {
            "uci" => vec![
                "id name Redline".into(),
                "id author Fabian Merkl".into(),
                "uciok".into(),
            ],
            "isready" => vec!["readyok".into()],
            "ucinewgame" => {
                self.board = Board::startpos();
                vec![]
            }
            "position" => {
                self.handle_position(&parts[1..]);
                vec![]
            }
            "setoption" => {
                self.handle_setoption(&parts[1..]);
                vec![]
            }
            "go" => self.handle_go_api(&parts[1..]),
            "stop" => {
                self.searcher.stop.store(true, Ordering::SeqCst);
                if let Some(handle) = self.search_thread.take() {
                    let _ = handle.join();
                }
                vec![]
            }
            "quit" => {
                self.searcher.stop.store(true, Ordering::SeqCst);
                if let Some(handle) = self.search_thread.take() {
                    let _ = handle.join();
                }
                vec![]
            }
            _ => vec![],
        }
    }


    fn handle_position(&mut self, args: &[&str]) {
        if args.is_empty() { return; }

        let mut current_idx = 0;
        if args[0] == "startpos" {
            self.board = Board::startpos();
            current_idx = 1;
        } else if args[0] == "fen" {
            // Reconstruct FEN string from parts until "moves" or end
            let mut fen_parts = Vec::new();
            current_idx = 1;
            while current_idx < args.len() && args[current_idx] != "moves" {
                fen_parts.push(args[current_idx]);
                current_idx += 1;
            }
            let fen = fen_parts.join(" ");
            if let Some(b) = Board::from_fen(&fen) {
                self.board = b;
            }
        }

        if current_idx < args.len() && args[current_idx] == "moves" {
            for &m_str in &args[current_idx + 1..] {
                if let Some(m) = self.board.parse_move(m_str) {
                    self.board.make_move(m);
                }
            }
        }
    }

    fn handle_setoption(&mut self, args: &[&str]) {
        if args.len() >= 4 && args[0] == "name" && args[1] == "Threads" && args[2] == "value" {
            if let Ok(threads) = args[3].parse::<usize>() {
                self.num_threads = threads;
            }
        } else if args.len() >= 4 && args[0] == "name" && args[1] == "Hash" && args[2] == "value" {
            if let Ok(mb) = args[3].parse::<usize>() {
                let tt = Arc::new(TranspositionTable::new(mb));
                self.searcher.tt = tt;
            }
        }
    }

    fn handle_go_api(&mut self, args: &[&str]) -> Vec<String> {
        // Stop any current search
        self.searcher.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.search_thread.take() {
            let _ = handle.join();
        }

        let mut depth = None;
        let mut wtime = None;
        let mut btime = None;
        let mut winc = 0;
        let mut binc = 0;
        let mut movestogo = None;
        let mut movetime = None;
        let mut nodes = None;
        let mut infinite = false;

        let mut i = 0;
        while i < args.len() {
            match args[i] {
                "depth" => {
                    if i + 1 < args.len() {
                        depth = Some(args[i + 1].parse().unwrap_or(64));
                        i += 1;
                    }
                }
                "wtime" => {
                    if i + 1 < args.len() {
                        wtime = Some(args[i + 1].parse::<u64>().unwrap_or(0));
                        i += 1;
                    }
                }
                "btime" => {
                    if i + 1 < args.len() {
                        btime = Some(args[i + 1].parse::<u64>().unwrap_or(0));
                        i += 1;
                    }
                }
                "winc" => {
                    if i + 1 < args.len() {
                        winc = args[i + 1].parse::<u64>().unwrap_or(0);
                        i += 1;
                    }
                }
                "binc" => {
                    if i + 1 < args.len() {
                        binc = args[i + 1].parse::<u64>().unwrap_or(0);
                        i += 1;
                    }
                }
                "movestogo" => {
                    if i + 1 < args.len() {
                        movestogo = Some(args[i + 1].parse::<u64>().unwrap_or(30));
                        i += 1;
                    }
                }
                "movetime" => {
                    if i + 1 < args.len() {
                        movetime = Some(args[i + 1].parse::<u64>().unwrap_or(0));
                        i += 1;
                    }
                }
                "nodes" => {
                    if i + 1 < args.len() {
                        nodes = Some(args[i + 1].parse::<u64>().unwrap_or(0));
                        i += 1;
                    }
                }
                "infinite" => {
                    infinite = true;
                }
                _ => {}
            }
            i += 1;
        }

        let time_limit = if let Some(mt) = movetime {
            Some(Duration::from_millis(mt.saturating_sub(20))) // subtract a small margin
        } else if infinite {
            None
        } else {
            self.allocate_time(wtime, btime, winc, binc, movestogo)
        };
        
        // If depth is not specified, use a large depth if we have time control or infinite
        let search_depth = if let Some(d) = depth {
            d
        } else if time_limit.is_some() || infinite || nodes.is_some() {
            100 // Large depth for infinite or time control
        } else {
            6
        };

        // Clone needed data for the search thread
        let mut board_clone = self.board.clone();
        let num_threads = self.num_threads;
        
        // Use a pointer to searcher or a way to access it? 
        // Searcher has Arc<AtomicBool> stop and Arc<TranspositionTable> tt.
        // We need to be able to call search on it.
        // Actually Searcher itself might need to be clonable or we create a new one with same Arcs.
        
        let tt_clone = Arc::clone(&self.searcher.tt);
        let nodes_clone = Arc::clone(&self.searcher.nodes);
        let stop_clone = Arc::clone(&self.searcher.stop);
        
        let handle = std::thread::spawn(move || {
            let mut thread_searcher = Searcher::new(tt_clone);
            thread_searcher.nodes = nodes_clone;
            thread_searcher.stop = stop_clone;
            
            let result = thread_searcher.search(&mut board_clone, search_depth, time_limit, num_threads);
            
            let mut best_move = result.best_move;
            let mut score = result.score;
            let mut depth = result.depth;
            
            // If best_move is None (e.g. search was stopped before any move was found at depth 1), 
            // try to find the best move from the TT.
            if best_move.is_none() {
                if let Some(entry) = thread_searcher.tt.probe(board_clone.hash) {
                    best_move = entry.best_move;
                    score = entry.score;
                    depth = entry.depth as u32;
                }
            }

            if let Some(m) = best_move {
                let elapsed = thread_searcher.start_time.elapsed().as_millis() as u64;
                let total_nodes = thread_searcher.nodes.load(Ordering::Relaxed);
                let nps = if elapsed > 0 { (total_nodes * 1000) / elapsed } else { 0 };
                let hashfull = thread_searcher.tt.hashfull();
                println!("info depth {} seldepth {} multipv 1 score {} nodes {} nps {} hashfull {} tbhits 0 time {}",
                    depth, thread_searcher.seldepth, thread_searcher.format_score(score), total_nodes, nps, hashfull, elapsed);
                println!("bestmove {}", m);
            } else {
                // Fallback: pick the first legal move if nothing was found
                let moves = crate::movegen::generate_pseudo_legal_moves(&mut board_clone);
                for m in moves {
                    let state = board_clone.make_move(m);
                    if !board_clone.is_in_check(board_clone.side_to_move.opposite()) {
                        println!("bestmove {}", m);
                        board_clone.unmake_move(m, state);
                        return;
                    }
                    board_clone.unmake_move(m, state);
                }
            }
        });

        self.search_thread = Some(handle);
        
        vec![]
    }


    fn allocate_time(&self, wtime: Option<u64>, btime: Option<u64>, winc: u64, binc: u64, movestogo: Option<u64>) -> Option<Duration> {
        let (my_time, my_inc) = if self.board.side_to_move == crate::board::piece::Color::White {
            (wtime, winc)
        } else {
            (btime, binc)
        };

        if let Some(time) = my_time {
            let moves_to_go = movestogo.unwrap_or(30);
            
            // Basic allocation: a fraction of remaining time
            let mut allocated = time / moves_to_go + my_inc * 3 / 4;
            
            // Adjust based on game phase (complexity)
            // Total pieces on board is a simple proxy for complexity
            let piece_count = self.board.all_occupancy.count_ones();
            if piece_count > 20 {
                // More time in complex positions (middlegame)
                allocated = allocated * 12 / 10;
            } else if piece_count < 10 {
                // Less time in simple positions (endgame)
                allocated = allocated * 8 / 10;
            }

            // Never spend more than 80% of total time on one move
            if allocated > time * 8 / 10 {
                allocated = time * 8 / 10;
            }

            // Safety margin: subtract 50ms for communication overhead
            if allocated > 50 {
                allocated -= 50;
            } else {
                allocated = allocated.min(10); // at least 10ms if we are very low
            }

            Some(Duration::from_millis(allocated))
        } else {
            None
        }
    }

}
