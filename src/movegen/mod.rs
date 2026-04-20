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

    pub fn generate_pseudo_legal_moves(board: &Board) -> MoveList {
        let mut moves = MoveList::new();
        pawn::generate_pawn_moves(board, &mut moves, false);
        knight::generate_knight_moves(board, &mut moves, false);
        sliding::generate_sliding_moves(board, &mut moves, false);
        king::generate_king_moves(board, &mut moves, false);
        moves
    }

    pub fn generate_captures(board: &Board) -> MoveList {
        let mut moves = MoveList::new();
        pawn::generate_pawn_moves(board, &mut moves, true);
        knight::generate_knight_moves(board, &mut moves, true);
        sliding::generate_sliding_moves(board, &mut moves, true);
        king::generate_king_moves(board, &mut moves, true);
        moves
    }

    pub fn generate_evasions(board: &Board) -> MoveList {
        let mut moves = MoveList::new();
        let side = board.side_to_move;
        let king_sq = board.kings[side.idx()].trailing_zeros() as u8;
        let (_, checkers) = board.pins_and_checkers(side);
        let num_checkers = count_bits(checkers);

        // 1. King moves
        king::generate_king_moves(board, &mut moves, false);

        // 2. If single check, we can block or capture
        if num_checkers == 1 {
            let checker_sq = checkers.trailing_zeros() as u8;
            let target_mask = bit(checker_sq) | board.between(king_sq, checker_sq);
            
            let mut all_pseudo = MoveList::new();
            pawn::generate_pawn_moves(board, &mut all_pseudo, false);
            knight::generate_knight_moves(board, &mut all_pseudo, false);
            sliding::generate_sliding_moves(board, &mut all_pseudo, false);

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
    let side = board.side_to_move;
    let king_sq = board.kings[side.idx()].trailing_zeros() as u8;
    let (pinned, checkers) = board.pins_and_checkers(side);
    let num_checkers = count_bits(checkers);

    let mut legal_moves = MoveList::new();
    let pseudo_moves = generate_pseudo_legal_moves(board);
    let mut temp_board = board.clone();

    // If in double check, only king moves can be legal
    if num_checkers > 1 {
        for i in 0..pseudo_moves.len() {
            let m = pseudo_moves.get(i);
            if m.from() == king_sq {
                let state = temp_board.make_move(m);
                if !temp_board.is_in_check(side) {
                    legal_moves.push(m);
                }
                temp_board.unmake_move(m, state);
            }
        }
        return legal_moves;
    }

    // Single check or no check
    for i in 0..pseudo_moves.len() {
        let m = pseudo_moves.get(i);
        let from = m.from();
        let f = m.flags();

        // 1. King moves: always need to check if destination is attacked
        if from == king_sq {
            let state = temp_board.make_move(m);
            if !temp_board.is_in_check(side) {
                legal_moves.push(m);
            }
            temp_board.unmake_move(m, state);
            continue;
        }

        // 2. If in single check, non-king move must block or capture the checker
        if num_checkers == 1 {
            let checker_sq = checkers.trailing_zeros() as u8;
            let target_mask = bit(checker_sq) | board.between(king_sq, checker_sq);
            if (bit(m.to()) & target_mask) == 0 && f != flags::EN_PASSANT {
                continue;
            }
        }

        // 3. Pinned pieces can only move along the pin ray
        if (bit(from) & pinned) != 0 {
            let state = temp_board.make_move(m);
            if !temp_board.is_in_check(side) {
                legal_moves.push(m);
            }
            temp_board.unmake_move(m, state);
            continue;
        }

        // 4. En passant is special because it removes TWO pieces from the rank
        if f == flags::EN_PASSANT {
            let state = temp_board.make_move(m);
            if !temp_board.is_in_check(side) {
                legal_moves.push(m);
            }
            temp_board.unmake_move(m, state);
            continue;
        }

        // 5. All other moves are legal!
        legal_moves.push(m);
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
            pieces: [None; 64],
            colors: [None; 64],
            hash: 0,
        };
        // White: Kh1, Qc7
        // Black: Ka8
        b.put_piece(7, PieceType::King, Color::White);
        b.put_piece(50, PieceType::Queen, Color::White);
        b.put_piece(56, PieceType::King, Color::Black);
        b.update_occupancy();

        assert_eq!(get_game_state(&b), GameState::Stalemate);
    }
}