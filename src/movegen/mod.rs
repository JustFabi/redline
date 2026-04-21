pub mod pawn;
pub mod knight;
pub mod sliding;
pub mod king;
pub mod move_list;

use crate::board::board::Board;
use crate::board::r#move::flags;
use crate::movegen::move_list::MoveList;
use crate::board::bitboard::{bit, count_bits};

pub fn init_all() {
    knight::init_knight_attacks();
    king::init_king_attacks();
    pawn::init_pawn_attacks();
    crate::magic::init_magics();
}

#[derive(PartialEq, Clone, Copy)]
pub enum GenType {
    All,
    Captures,
    Quiets,
}

    pub fn generate_pseudo_legal_moves(board: &Board) -> MoveList {
        let mut moves = MoveList::new();
        pawn::generate_pawn_moves(board, &mut moves, GenType::All);
        knight::generate_knight_moves(board, &mut moves, GenType::All);
        sliding::generate_sliding_moves(board, &mut moves, GenType::All);
        king::generate_king_moves(board, &mut moves, GenType::All);
        moves
    }

    pub fn generate_captures(board: &Board) -> MoveList {
        let mut moves = MoveList::new();
        pawn::generate_pawn_moves(board, &mut moves, GenType::Captures);
        knight::generate_knight_moves(board, &mut moves, GenType::Captures);
        sliding::generate_sliding_moves(board, &mut moves, GenType::Captures);
        king::generate_king_moves(board, &mut moves, GenType::Captures);
        moves
    }

    pub fn generate_quiets(board: &Board) -> MoveList {
        let mut moves = MoveList::new();
        pawn::generate_pawn_moves(board, &mut moves, GenType::Quiets);
        knight::generate_knight_moves(board, &mut moves, GenType::Quiets);
        sliding::generate_sliding_moves(board, &mut moves, GenType::Quiets);
        king::generate_king_moves(board, &mut moves, GenType::Quiets);
        moves
    }

    pub fn generate_evasions(board: &Board) -> MoveList {
        let mut moves = MoveList::new();
        let side = board.side_to_move;
        let king_sq = board.kings[side.idx()].trailing_zeros() as u8;
        let (_, checkers) = board.pins_and_checkers(side);
        let num_checkers = count_bits(checkers);

        // 1. King moves
        king::generate_king_moves(board, &mut moves, GenType::All);

        // 2. If single check, we can block or capture
        if num_checkers == 1 {
            let checker_sq = checkers.trailing_zeros() as u8;
            let target_mask = bit(checker_sq) | board.between(king_sq, checker_sq);
            
            let mut all_pseudo = MoveList::new();
            pawn::generate_pawn_moves(board, &mut all_pseudo, GenType::All);
            knight::generate_knight_moves(board, &mut all_pseudo, GenType::All);
            sliding::generate_sliding_moves(board, &mut all_pseudo, GenType::All);

            for i in 0..all_pseudo.len() {
                let m = all_pseudo.get(i);
                if (bit(m.to()) & target_mask) != 0 || m.flags() == flags::EN_PASSANT {
                    moves.push(m);
                }
            }
        }
        // If double check, only king moves (already added)

        moves
    }

pub fn generate_legal_moves(board: &Board) -> MoveList {
    let mut legal_moves = MoveList::new();
    let pseudo_moves = generate_pseudo_legal_moves(board);

    for i in 0..pseudo_moves.len() {
        let m = pseudo_moves.get(i);
        if board.is_legal(m) {
            legal_moves.push(m);
        }
    }

    legal_moves
}

pub fn is_check(board: &Board) -> bool {
    let side = board.side_to_move;
    let king_bb = board.kings[side.idx()];
    if king_bb == 0 { return false; }
    let king_sq = king_bb.trailing_zeros() as u8;
    board.is_square_attacked(king_sq, side.opposite())
}

#[derive(Debug, PartialEq, Eq)]
pub enum GameState {
    Ongoing,
    Checkmate,
    Stalemate,
    DrawInsufficientMaterial,
    DrawFiftyMoveRule,
}

pub fn get_game_state(board: &Board) -> GameState {
    let moves = generate_legal_moves(board);
    if moves.is_empty() {
        if is_check(board) {
            return GameState::Checkmate;
        } else {
            return GameState::Stalemate;
        }
    }

    if is_insufficient_material(board) {
        return GameState::DrawInsufficientMaterial;
    }

    if board.halfmove_clock >= 100 {
        return GameState::DrawFiftyMoveRule;
    }

    GameState::Ongoing
}

fn is_insufficient_material(board: &Board) -> bool {
    let total_pieces = board.piece_count();
    if total_pieces > 4 { return false; } // Too many pieces for simple draw

    if total_pieces == 2 { return true; } // King vs King

    if total_pieces == 3 {
        // King + (Knight or Bishop) vs King
        if board.knights[0] != 0 || board.knights[1] != 0 || 
           board.bishops[0] != 0 || board.bishops[1] != 0 {
            return true;
        }
    }

    // King + Bishop vs King + Bishop (same color) is omitted for simplicity for now
    
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;
    use crate::board::piece::{Color, PieceType};
    use crate::board::r#move::{Move, flags};

    fn perft(board: &Board, depth: u32) -> u64 {
        if depth == 0 { return 1; }
        let mut nodes = 0;

        let moves = generate_legal_moves(board);
        for m in moves {
            let mut temp = board.clone();
            temp.make_move(m);
            nodes += perft(&temp, depth - 1);
        }
        nodes
    }

    #[test]
    fn test_perft_startpos() {
        init_all();
        let b = Board::startpos();
        assert_eq!(perft(&b, 1), 20);
        assert_eq!(perft(&b, 2), 400);
        assert_eq!(perft(&b, 3), 8902);
    }

    #[test]
    fn test_checkmate() {
        init_all();
        let mut b = Board::startpos();
        // Fool's mate
        // 1. f3 e5 2. g4 Qh4#
        b.make_move(Move::new(13, 21, flags::QUIET)); // f2-f3
        b.make_move(Move::new(52, 36, flags::DOUBLE_PAWN)); // e7-e5
        b.make_move(Move::new(14, 30, flags::DOUBLE_PAWN)); // g2-g4
        b.make_move(Move::new(59, 31, flags::QUIET)); // Qd8-h4

        assert_eq!(get_game_state(&b), GameState::Checkmate);
    }

    #[test]
    fn test_stalemate() {
        init_all();
        // A known stalemate position
        let mut b = Board {
            pawns: [0; 2],
            knights: [0; 2],
            bishops: [0; 2],
            rooks: [0; 2],
            queens: [0; 2],
            kings: [0; 2],
            occupancy: [0; 2],
            all_occupancy: 0,
            side_to_move: Color::Black,
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
        // White: Kh1, Qc7
        // Black: Ka8
        b.put_piece(7, PieceType::King, Color::White);
        b.put_piece(50, PieceType::Queen, Color::White);
        b.put_piece(56, PieceType::King, Color::Black);
        b.update_occupancy();

        assert_eq!(get_game_state(&b), GameState::Stalemate);
    }

    #[test]
    fn test_en_passant_generation() {
        init_all();
        // FEN: rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2
        let board = Board::from_fen("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2").unwrap();
        let moves = generate_legal_moves(&board);
        
        let mut found_ep = false;
        for m in moves {
            if m.flags() == flags::EN_PASSANT {
                found_ep = true;
                // White e5 pawn captures on d6
                assert_eq!(m.from(), 36); // e5 is square 36
                assert_eq!(m.to(), 43); // d6 is square 43
            }
        }
        assert!(found_ep, "En Passant move should be generated and legal");
    }

    #[test]
    fn test_search_en_passant() {
        init_all();
        // FEN: rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2
        let mut board = Board::from_fen("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2").unwrap();
        let tt = std::sync::Arc::new(crate::engine::tt::TranspositionTable::new(1));
        let mut searcher = crate::engine::search::Searcher::new(tt);
        let result = searcher.search(&mut board, 5, None, None, 1);
        
        assert!(result.best_move.is_some(), "Search should return a move for en passant position");
        // Often e5d6 is a very good move here or at least considered.
    }
}