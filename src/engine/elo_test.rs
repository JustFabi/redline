use crate::board::board::Board;
use crate::engine::search::{Searcher, SearchSettings};
use crate::engine::tt::TranspositionTable;
use std::sync::Arc;
use std::time::Duration;

/// Maximum number of half-moves per game before adjudicating as a draw.
const MAX_GAME_PLIES: usize = 400;

pub fn run_elo_test(num_games: usize, depth_limit: Option<u32>, base_settings: SearchSettings) {
    let mut wins = 0;
    let mut losses = 0;
    let mut draws = 0;
    use std::io::{self, Write};

    let time_per_move = 1000;

    println!("=== Elo Test: {} games ===", num_games);
    println!("Full engine: all heuristics enabled");
    println!("Base engine: custom settings");
    if let Some(d) = depth_limit {
        println!("Search limit: depth {} (hard time limit: {}ms)", d, time_per_move);
    } else {
        println!("Search limit: {}ms per move", time_per_move);
    }
    println!("Max game length: {} plies", MAX_GAME_PLIES);
    println!();

    let test_start = std::time::Instant::now();

    for i in 0..num_games {
        let game_start = std::time::Instant::now();
        let mut board = Board::startpos();
        let mut move_count = 0usize;
        let mut total_nodes = 0u64;
        
        // Full engine is Player 1, Base engine is Player 2
        // Alternate colors: Game 0 (P1 White), Game 1 (P1 Black), etc.
        let player1_is_white = i % 2 == 0;
        
        // Use separate TTs for each engine to avoid interference
        let tt1 = Arc::new(TranspositionTable::new(8)); // 8MB (smaller = faster alloc)
        let tt2 = Arc::new(TranspositionTable::new(8));
        
        let mut searcher_full = Searcher::new(tt1);
        searcher_full.settings = SearchSettings::default();
        searcher_full.is_main_thread = true;
        
        let mut searcher_base = Searcher::new(tt2);
        searcher_base.settings = base_settings;
        searcher_base.is_main_thread = true;

        let mut game_over = false;
        let mut result = 0.5; // 1.0 for player1 win, 0.0 for player1 loss, 0.5 for draw

        while !game_over {
            let is_player1_turn = (board.side_to_move == crate::board::piece::Color::White) == player1_is_white;
            
            let search_depth = depth_limit.unwrap_or(64);
            let soft_limit = if depth_limit.is_none() {
                Some(Duration::from_millis(time_per_move))
            } else {
                None
            };
            let hard_limit = Some(Duration::from_millis(time_per_move));

            if is_player1_turn {
                print!("F");
            } else {
                print!("B");
            }
            io::stdout().flush().unwrap();

            let search_res = if is_player1_turn {
                searcher_full.search(&mut board, search_depth, soft_limit, hard_limit, 1)
            } else {
                searcher_base.search(&mut board, search_depth, soft_limit, hard_limit, 1)
            };

            if let Some(m) = search_res.best_move {
                board.make_move(m);
                move_count += 1;
                if move_count % 10 == 0 {
                    print!(".");
                    io::stdout().flush().unwrap();
                }
                // Track nodes
                let nodes = if is_player1_turn { searcher_full.nodes.load(std::sync::atomic::Ordering::SeqCst) } else { searcher_base.nodes.load(std::sync::atomic::Ordering::SeqCst) };
                total_nodes += nodes;
            } else {
                // No legal moves - checkmate or stalemate
                if board.is_in_check(board.side_to_move) {
                    // Checkmate
                    if is_player1_turn {
                        result = 0.0; // Player 1 (Full) was to move and is in checkmate
                    } else {
                        result = 1.0; // Player 2 (Base) was to move and is in checkmate
                    }
                } else {
                    // Stalemate
                    result = 0.5;
                }
                game_over = true;
            }

            if !game_over {
                if board.halfmove_clock >= 100 || board.is_repetition() {
                    result = 0.5;
                    game_over = true;
                }
                // Adjudicate as draw if game is too long
                if move_count >= MAX_GAME_PLIES {
                    result = 0.5;
                    game_over = true;
                }
            }
        }

        if result == 1.0 { wins += 1; }
        else if result == 0.0 { losses += 1; }
        else { draws += 1; }

        let score = wins as f64 + 0.5 * draws as f64;
        let total = (i + 1) as f64;
        let win_rate = score / total;
        let elo_diff = if win_rate <= 0.0 { -1000.0 } 
                       else if win_rate >= 1.0 { 1000.0 }
                       else { -400.0 * (1.0 / win_rate - 1.0).log10() };

        let game_time = game_start.elapsed();
        let nps = if game_time.as_secs_f64() > 0.0 { total_nodes as f64 / game_time.as_secs_f64() } else { 0.0 };
        println!("Game {}/{}: {} ({} plies, {:.1}s, {} nodes, {:.0} nps) | W-D-L: {}-{}-{} ({:.1}%) | Elo: {:+.1}", 
            i + 1, num_games, 
            if result == 1.0 { "Win " } else if result == 0.0 { "Loss" } else { "Draw" },
            move_count,
            game_time.as_secs_f64(),
            total_nodes,
            nps,
            wins, draws, losses, win_rate * 100.0, elo_diff);
        io::stdout().flush().unwrap();
    }

    let total_time = test_start.elapsed();
    let total_score = wins as f64 + 0.5 * draws as f64;
    let win_rate = total_score / num_games as f64;
    let final_elo_diff = if win_rate <= 0.0 { -1000.0 } 
                         else if win_rate >= 1.0 { 1000.0 }
                         else { -400.0 * (1.0 / win_rate - 1.0).log10() };
    
    println!("\n=== Final Result ({:.1}s) ===", total_time.as_secs_f64());
    println!("Full vs Base: {} - {} - {} (Wins - Draws - Losses)", wins, draws, losses);
    println!("Elo Difference: {:+.1}", final_elo_diff);
}
