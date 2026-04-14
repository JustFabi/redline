mod board;
mod movegen;
mod engine;
mod uci;
mod api;

use movegen::knight::init_knight_attacks;
use movegen::king::init_king_attacks;
use board::board::Board;
use movegen::GameState;
use std::io::{self, Write};
use uci::Uci;
use engine::search::Searcher;

fn main() {
    // Initialize precomputed tables
    init_knight_attacks();
    init_king_attacks();

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "uci" {
        let mut uci = Uci::new();
        uci.loop_communication();
        return;
    }

    if args.len() > 1 && args[1] == "api" {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(api::run_server());
        return;
    }

    println!("Redline Chess Engine");
    println!("Commands: 'startpos', 'fen <fen>', 'move <m>', 'go', 'quit'");

    let mut board = Board::startpos();
    let tt = std::sync::Arc::new(engine::tt::TranspositionTable::new(64));
    let mut searcher = Searcher::new(tt);

    loop {
        board.print_board();
        
        let state = movegen::get_game_state(&board);
        match state {
            GameState::Checkmate => {
                println!("CHECKMATE! {:?} wins.", board.side_to_move.opposite());
                break;
            }
            GameState::Stalemate => {
                println!("DRAW! Stalemate.");
                break;
            }
            GameState::DrawInsufficientMaterial => {
                println!("DRAW! Insufficient material.");
                break;
            }
            GameState::DrawFiftyMoveRule => {
                println!("DRAW! 50-move rule.");
                break;
            }
            GameState::Ongoing => {
                if movegen::is_check(&board) {
                    println!("CHECK!");
                }
            }
        }

        print!("{:?}> ", board.side_to_move);
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();

        if input == "quit" || input == "exit" {
            break;
        } else if input == "startpos" {
            board = Board::startpos();
        } else if input == "go" {
            let result = searcher.search(&mut board, 6, None, 1);
            if let Some(m) = result.best_move {
                println!("Engine suggests: {} (score: {}, nodes: {})", move_to_str(m), result.score, result.nodes);
                board.make_move(m);
            }
        } else if input.starts_with("fen ") {
            if let Some(new_board) = Board::from_fen(&input[4..]) {
                board = new_board;
            } else {
                println!("Invalid FEN.");
            }
        } else if input.starts_with("move ") {
            let m_str = &input[5..];
            if let Some(m) = board.parse_move(m_str) {
                board.make_move(m);
            } else {
                println!("Invalid or illegal move.");
            }
        } else if !input.is_empty() {
            // Try parsing as move directly
            if let Some(m) = board.parse_move(input) {
                board.make_move(m);
            } else {
                println!("Unknown command or invalid move.");
            }
        }
    }
}

fn move_to_str(m: crate::board::r#move::Move) -> String {
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