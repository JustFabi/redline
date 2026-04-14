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
}

impl Uci {
    pub fn new() -> Self {
        let tt = Arc::new(TranspositionTable::new(64)); // 64MB default
        Self {
            board: Board::startpos(),
            searcher: Searcher::new(tt),
            num_threads: 1,
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
                self.searcher.stop.store(true, Ordering::Relaxed);
                vec![]
            }
            "quit" => vec![],
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
        }
    }

    fn handle_go_api(&mut self, args: &[&str]) -> Vec<String> {
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
        } else {
            self.allocate_time(wtime, btime, winc, binc, movestogo)
        };
        
        // If depth is not specified, use a large depth if we have time control or infinite
        let search_depth = if let Some(d) = depth {
            d
        } else if time_limit.is_some() || infinite || nodes.is_some() {
            64
        } else {
            6
        };

        let result = self.searcher.search(&mut self.board, search_depth, time_limit, self.num_threads);
        
        let mut responses = Vec::new();
        for info in result.info_lines {
            responses.push(info);
        }

        if let Some(m) = result.best_move {
            let mut info = format!("info depth {} score cp {} nodes {}", result.depth, result.score, result.nodes);
            // NPS (Nodes Per Second)
            let elapsed = self.searcher.start_time.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                info.push_str(&format!(" nps {}", (result.nodes as f64 / elapsed) as u64));
            }
            responses.push(info);
            responses.push(format!("bestmove {}", self.move_to_uci(m)));
        }
        responses
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

    fn move_to_uci(&self, m: crate::board::r#move::Move) -> String {
        let from = m.from();
        let to = m.to();
        let from_file = (b'a' + (from % 8)) as char;
        let from_rank = (b'1' + (from / 8)) as char;
        let to_file = (b'a' + (to % 8)) as char;
        let to_rank = (b'1' + (to / 8)) as char;

        let mut s = format!("{}{}{}{}", from_file, from_rank, to_file, to_rank);
        
        use crate::board::r#move::flags;
        match m.flags() {
            flags::PROMOTE_QUEEN | flags::PROMOTE_QUEEN_CAPTURE => s.push('q'),
            flags::PROMOTE_ROOK | flags::PROMOTE_ROOK_CAPTURE => s.push('r'),
            flags::PROMOTE_BISHOP | flags::PROMOTE_BISHOP_CAPTURE => s.push('b'),
            flags::PROMOTE_KNIGHT | flags::PROMOTE_KNIGHT_CAPTURE => s.push('n'),
            _ => {}
        }
        s
    }
}
