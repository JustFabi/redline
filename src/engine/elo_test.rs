use crate::board::board::Board;
use crate::engine::search::{Searcher, SearchSettings};
use crate::engine::tt::TranspositionTable;
use std::sync::Arc;
use std::time::Duration;

pub fn run_elo_test(num_games: usize, depth_limit: Option<u32>) {
    let mut wins = 0;
    let mut losses = 0;
    let mut draws = 0;

    println!("Starting Elo test: {} games", num_games);
    if let Some(d) = depth_limit {
        println!("Search limit: depth {}", d);
    } else {
        println!("Search limit: 50ms per move");
    }

    for i in 0..num_games {
        let mut board = Board::startpos();
        
        // Full engine is Player 1, Base engine is Player 2
        // Alternate colors: Game 0 (P1 White), Game 1 (P1 Black), etc.
        let player1_is_white = i % 2 == 0;
        
        // Use separate TTs for each engine to avoid interference
        let tt1 = Arc::new(TranspositionTable::new(16)); // 16MB
        let tt2 = Arc::new(TranspositionTable::new(16));
        
        let mut searcher_full = Searcher::new(tt1);
        searcher_full.settings = SearchSettings::default();
        
        let mut searcher_base = Searcher::new(tt2);
        searcher_base.settings = SearchSettings::none();

        let mut game_over = false;
        let mut result = 0.5; // 1.0 for player1 win, 0.0 for player1 loss, 0.5 for draw

        while !game_over {
            let is_player1_turn = (board.side_to_move == crate::board::piece::Color::White) == player1_is_white;
            
            let search_depth = depth_limit.unwrap_or(64);
            let soft_limit = None;
            let hard_limit = if depth_limit.is_some() { None } else { Some(Duration::from_millis(50)) };

            let search_res = if is_player1_turn {
                searcher_full.search(&mut board, search_depth, soft_limit, hard_limit, 1)
            } else {
                searcher_base.search(&mut board, search_depth, soft_limit, hard_limit, 1)
            };

            if let Some(m) = search_res.best_move {
                board.make_move(m);
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

        println!("Game {}/{}: Result {}, Score: {} - {} - {} ({:.1}%), Elo: {:+.1}", 
            i + 1, num_games, 
            if result == 1.0 { "Win" } else if result == 0.0 { "Loss" } else { "Draw" },
            wins, draws, losses, win_rate * 100.0, elo_diff);
    }

    let total_score = wins as f64 + 0.5 * draws as f64;
    let win_rate = total_score / num_games as f64;
    let final_elo_diff = if win_rate <= 0.0 { -1000.0 } 
                         else if win_rate >= 1.0 { 1000.0 }
                         else { -400.0 * (1.0 / win_rate - 1.0).log10() };
    
    println!("\nFinal Result:");
    println!("Full vs Base: {} - {} - {} (Wins - Draws - Losses)", wins, draws, losses);
    println!("Elo Difference: {:+.1}", final_elo_diff);
}
