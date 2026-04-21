// Core board representation using bitboards.

use super::bitboard::*;
use super::piece::{Color, PieceType};
use super::r#move::{Move, flags};
use super::zobrist::ZOBRIST;
use super::pst::{MG_VALUE, EG_VALUE, MG_PST, EG_PST};
use crate::movegen::knight::get_knight_attacks;
use crate::movegen::king::get_king_attacks;
use crate::movegen::sliding::{get_bishop_attacks, get_rook_attacks};

/// Main board structure
#[derive(Clone)]
pub struct Board {
    // Piece bitboards (index = Color)
    pub pawns: [u64; 2],
    pub knights: [u64; 2],
    pub bishops: [u64; 2],
    pub rooks: [u64; 2],
    pub queens: [u64; 2],
    pub kings: [u64; 2],

    // Occupancy bitboards
    pub occupancy: [u64; 2], // per color
    pub all_occupancy: u64,  // combined

    // Side to move
    pub side_to_move: Color,

    // Castling rights (4 bits: WK, WQ, BK, BQ)
    pub castling_rights: u8,

    // En passant target square (if any)
    pub en_passant_square: Option<u8>,

    // Halfmove clock (for 50-move rule)
    pub halfmove_clock: u16,

    // Fullmove number
    pub fullmove_number: u16,

    // Last move made on board
    pub last_move: Option<Move>,

    // History of hashes for repetition detection
    pub history: Vec<u64>,

    // Piece at square (optional optimization)
    pub pieces: [PieceType; 64],
    pub colors: [Color; 64],

    // Zobrist hash
    pub hash: u64,

    // Incremental evaluation scores (White - Black)
    pub mg_pst: i32,
    pub eg_pst: i32,

    pub pawn_hash: u64,
}

pub mod castling {
    pub const WHITE_KING: u8 = 1 << 0;
    pub const WHITE_QUEEN: u8 = 1 << 1;
    pub const BLACK_KING: u8 = 1 << 2;
    pub const BLACK_QUEEN: u8 = 1 << 3;
}

#[derive(Copy, Clone)]
pub struct UndoState {
    pub castling_rights: u8,
    pub en_passant_square: Option<u8>,
    pub halfmove_clock: u16,
    pub captured_piece: PieceType,
    pub last_move: Option<Move>,
    pub hash: u64,
    pub mg_pst: i32,
    pub eg_pst: i32,
    pub pawn_hash: u64,
}

impl Board {
    /// Creates the standard chess starting position
    pub fn startpos() -> Self {
        let mut board = Self {
            pawns: [0; 2],
            knights: [0; 2],
            bishops: [0; 2],
            rooks: [0; 2],
            queens: [0; 2],
            kings: [0; 2],
            occupancy: [0; 2],
            all_occupancy: 0,
            side_to_move: Color::White,
            castling_rights: 0xF, // All rights initially
            en_passant_square: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            last_move: None,
            history: Vec::with_capacity(128),
            pieces: [PieceType::Empty; 64],
            colors: [Color::None; 64],
            hash: 0,
            mg_pst: 0,
            eg_pst: 0,
            pawn_hash: 0,
        };

        // =========================
        // Set up initial positions
        // =========================

        // Helper to put a piece
        let mut put = |sq: u8, pt: PieceType, color: Color| {
            let bb = bit(sq);
            let c = color.idx();
            match pt {
                PieceType::Pawn => board.pawns[c] |= bb,
                PieceType::Knight => board.knights[c] |= bb,
                PieceType::Bishop => board.bishops[c] |= bb,
                PieceType::Rook => board.rooks[c] |= bb,
                PieceType::Queen => board.queens[c] |= bb,
                PieceType::King => board.kings[c] |= bb,
                PieceType::Empty => {},
            }
            board.pieces[sq as usize] = pt;
            board.colors[sq as usize] = color;
        };

        // White
        for i in 8..16 { put(i, PieceType::Pawn, Color::White); }
        put(0, PieceType::Rook, Color::White);
        put(1, PieceType::Knight, Color::White);
        put(2, PieceType::Bishop, Color::White);
        put(3, PieceType::Queen, Color::White);
        put(4, PieceType::King, Color::White);
        put(5, PieceType::Bishop, Color::White);
        put(6, PieceType::Knight, Color::White);
        put(7, PieceType::Rook, Color::White);

        // Black
        for i in 48..56 { put(i, PieceType::Pawn, Color::Black); }
        put(56, PieceType::Rook, Color::Black);
        put(57, PieceType::Knight, Color::Black);
        put(58, PieceType::Bishop, Color::Black);
        put(59, PieceType::Queen, Color::Black);
        put(60, PieceType::King, Color::Black);
        put(61, PieceType::Bishop, Color::Black);
        put(62, PieceType::Knight, Color::Black);
        put(63, PieceType::Rook, Color::Black);

        // Compute occupancy bitboards
        board.update_occupancy();
        board.hash = board.compute_hash();

        board
    }

    /// Creates a board from a FEN string
    pub fn from_fen(fen: &str) -> Option<Self> {
        let mut board = Self {
            pawns: [0; 2],
            knights: [0; 2],
            bishops: [0; 2],
            rooks: [0; 2],
            queens: [0; 2],
            kings: [0; 2],
            occupancy: [0; 2],
            all_occupancy: 0,
            side_to_move: Color::White,
            castling_rights: 0,
            en_passant_square: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            last_move: None,
            history: Vec::with_capacity(128),
            pieces: [PieceType::Empty; 64],
            colors: [Color::None; 64],
            hash: 0,
            mg_pst: 0,
            eg_pst: 0,
            pawn_hash: 0,
        };

        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.is_empty() { return None; }

        // 1. Piece positions
        let mut row = 7;
        let mut col = 0;
        for c in parts[0].chars() {
            if c == '/' {
                row -= 1;
                col = 0;
            } else if c.is_ascii_digit() {
                col += c.to_digit(10).unwrap() as u8;
            } else {
                let sq = row * 8 + col;
                let color = if c.is_uppercase() { Color::White } else { Color::Black };
                let pt = match c.to_ascii_lowercase() {
                    'p' => PieceType::Pawn,
                    'n' => PieceType::Knight,
                    'b' => PieceType::Bishop,
                    'r' => PieceType::Rook,
                    'q' => PieceType::Queen,
                    'k' => PieceType::King,
                    _ => return None,
                };
                board.put_piece(sq, pt, color);
                col += 1;
            }
        }

        // 2. Side to move
        if parts.len() > 1 {
            board.side_to_move = match parts[1] {
                "w" => Color::White,
                "b" => Color::Black,
                _ => return None,
            };
        }

        // 3. Castling rights
        if parts.len() > 2 {
            if parts[2] != "-" {
                for c in parts[2].chars() {
                    match c {
                        'K' => board.castling_rights |= castling::WHITE_KING,
                        'Q' => board.castling_rights |= castling::WHITE_QUEEN,
                        'k' => board.castling_rights |= castling::BLACK_KING,
                        'q' => board.castling_rights |= castling::BLACK_QUEEN,
                        _ => return None,
                    }
                }
            }
        }

        // 4. En passant square
        if parts.len() > 3 {
            if parts[3] != "-" {
                let bytes = parts[3].as_bytes();
                if bytes.len() != 2 { return None; }
                let col = bytes[0] - b'a';
                let row = bytes[1] - b'1';
                board.en_passant_square = Some(row * 8 + col);
            }
        }

        // 5. Halfmove clock
        if parts.len() > 4 {
            board.halfmove_clock = parts[4].parse().ok()?;
        }

        // 6. Fullmove number
        if parts.len() > 5 {
            board.fullmove_number = parts[5].parse().ok()?;
        }

        board.update_occupancy();
        board.hash = board.compute_hash();
        Some(board)
    }
/// Returns whether a move is **pseudo-legal** and does **not** leave the king in check.
    /// This is the standard "legal move" check in chess engines.
    ///
    /// Highly optimized:
    /// - Early exit for most common cases
    /// - Reuses `is_in_check` and `pins_and_checkers`
    /// - Special fast paths for castling, en passant, and pinned pieces
    #[inline(always)]
    pub fn is_legal(&self, m: Move) -> bool {
        let from = m.from();
        let to = m.to();
        let flags = m.flags();

        // 1. Basic sanity: from square must have our piece
        let moving_color = self.side_to_move;
        if self.colors[from as usize] != moving_color {
            return false;
        }

        // 2. Cannot capture own piece
        let color = self.colors[to as usize];
        if color != Color::None {
            if color == moving_color {
                return false;
            }
        }

        let pt = self.pieces[from as usize];

        // 3. Special move handling (castling, en passant, promotion)
        match flags {
            // ====================== CASTLING ======================
            flags::KING_CASTLE | flags::QUEEN_CASTLE => {
                return self.is_legal_castling(m);
            }

            // ====================== EN PASSANT ======================
            flags::EN_PASSANT => {
                return self.is_legal_en_passant(m);
            }

            _ => {}
        }

        // 4. Normal moves (including promotions and captures)

        // Fast path: if not in check and piece is not pinned → almost always legal
        if !self.is_in_check(moving_color) {
            let (pinned, _) = self.pins_and_checkers(moving_color);

            if pt == PieceType::King {
                let occ = self.all_occupancy ^ bit(from);
                return !self.is_square_attacked_with_occ(to, moving_color.opposite(), occ);
            }

            // If the piece is not pinned, the move is legal (we already checked basic validity)
            if (pinned & bit(from)) == 0 {
                return true;
            }

            // Piece is pinned → must stay on the pin line
            return self.is_pinned_move_legal(from, to, pinned);
        }

        // 5. We are in check → more expensive validation
        self.is_legal_when_in_check(m)
    }

    // ===================================================================
    // Helper: Castling legality
    // ===================================================================
    #[inline(never)]
    fn is_legal_castling(&self, m: Move) -> bool {
        let side = self.side_to_move;
        let king_from = if side == Color::White { 4 } else { 60 };
        let flags = m.flags();

        // King must be on starting square and not in check
        if self.kings[side.idx()] != bit(king_from) || self.is_in_check(side) {
            return false;
        }

        let (king_to, _rook_from, _rook_to) = if flags == flags::KING_CASTLE {
            if side == Color::White { (6, 7, 5) } else { (62, 63, 61) }
        } else {
            if side == Color::White { (2, 0, 3) } else { (58, 56, 59) }
        };

        // Check castling rights
        let required_right = if flags == flags::KING_CASTLE {
            if side == Color::White { castling::WHITE_KING } else { castling::BLACK_KING }
        } else {
            if side == Color::White { castling::WHITE_QUEEN } else { castling::BLACK_QUEEN }
        };
        if (self.castling_rights & required_right) == 0 {
            return false;
        }

        // Squares between king and rook must be empty
        let between_mask = if flags == flags::KING_CASTLE {
            if side == Color::White { bit(5) | bit(6) } else { bit(61) | bit(62) }
        } else {
            if side == Color::White { bit(1) | bit(2) | bit(3) } else { bit(57) | bit(58) | bit(59) }
        };
        if (self.all_occupancy & between_mask) != 0 {
            return false;
        }

        // King cannot pass through or land on attacked squares
        let passing_sq = if flags == flags::KING_CASTLE {
            if side == Color::White { 5 } else { 61 }
        } else {
            if side == Color::White { 3 } else { 59 }
        };

        !self.is_square_attacked(king_from, side.opposite()) && // redundant but cheap
        !self.is_square_attacked(passing_sq, side.opposite()) &&
        !self.is_square_attacked(king_to, side.opposite())
    }

    // ===================================================================
    // Helper: En passant legality
    // ===================================================================
    #[inline(never)]
    fn is_legal_en_passant(&self, m: Move) -> bool {
        let side = self.side_to_move;
        let to = m.to();
        let from = m.from();

        if self.en_passant_square != Some(to) {
            return false;
        }

        let cap_sq = if side == Color::White { to - 8 } else { to + 8 };
        if self.pieces[cap_sq as usize] != PieceType::Pawn {
            return false;
        }

        let king_sq = self.kings[side.idx()].trailing_zeros() as u8;
        let occ = self.all_occupancy ^ bit(from) ^ bit(cap_sq) | bit(to);
        let enemy = side.opposite();
        let e_idx = enemy.idx();
        
        // 1. Sliding attacks with new occupancy
        let rooks_queens = self.rooks[e_idx] | self.queens[e_idx];
        let bishops_queens = self.bishops[e_idx] | self.queens[e_idx];
        
        if (crate::magic::get_rook_attacks(king_sq, occ) & rooks_queens) != 0 { return false; }
        if (crate::magic::get_bishop_attacks(king_sq, occ) & bishops_queens) != 0 { return false; }
        
        // 2. Knight attacks
        if (crate::movegen::knight::get_knight_attacks(king_sq) & self.knights[e_idx]) != 0 { return false; }
        
        // 3. Pawn attacks (enemy pawns EXCEPT the captured one)
        let enemy_pawns = self.pawns[e_idx] & !bit(cap_sq);
        if (crate::movegen::pawn::get_pawn_attacks(king_sq, side) & enemy_pawns) != 0 { return false; }
        
        // 4. King attacks
        if (crate::movegen::king::get_king_attacks(king_sq) & self.kings[e_idx]) != 0 { return false; }
        
        true
    }

    /// Optimized legality check that uses pre-calculated pin and checker masks.
    #[inline(always)]
    pub fn is_legal_fast(&self, m: Move, pinned: u64, checkers: u64) -> bool {
        let from = m.from();
        let to = m.to();
        let flags = m.flags();
        let moving_color = self.side_to_move;

        // 1. Basic sanity (optional, but good for safety)
        if self.colors[from as usize] != moving_color { return false; }
        if self.colors[to as usize] == moving_color { return false; }

        let pt = self.pieces[from as usize];

        // 2. Special moves
        match flags {
            flags::KING_CASTLE | flags::QUEEN_CASTLE => {
                return self.is_legal_castling(m);
            }
            flags::EN_PASSANT => {
                return self.is_legal_en_passant(m);
            }
            _ => {}
        }

        // 3. Normal moves
        if checkers == 0 {
            if pt == PieceType::King {
                let occ = self.all_occupancy ^ bit(from);
                return !self.is_square_attacked_with_occ(to, moving_color.opposite(), occ);
            }
            if (pinned & bit(from)) == 0 {
                return true;
            }
            return self.is_pinned_move_legal(from, to, pinned);
        }

        // 4. In check
        self.is_legal_when_in_check_fast(m, pinned, checkers)
    }

    #[inline(never)]
    fn is_legal_when_in_check_fast(&self, m: Move, _pinned: u64, checkers: u64) -> bool {
        let side = self.side_to_move;
        let num_checkers = count_bits(checkers);

        let from = m.from();
        let to = m.to();
        let pt = self.pieces[from as usize];

        if pt == PieceType::King {
            let occ = self.all_occupancy ^ bit(from);
            return !self.is_square_attacked_with_occ(to, side.opposite(), occ);
        }

        if num_checkers > 1 { return false; }

        let checker_sq = checkers.trailing_zeros() as u8;
        if to == checker_sq {
            // Must not be pinned! (If we are pinned, we can't capture the checker unless it's on the pin ray)
            // But wait, the standard is_legal handles pinned already?
            // Actually, if we are in check, a pinned piece can only capture the checker if the checker is on the pin ray.
            // Since is_pinned_move_legal handles that, we should use it.
            return self.is_pinned_move_legal(from, to, _pinned);
        }

        let checker_pt = self.pieces[checker_sq as usize];
        if checker_pt == PieceType::Knight || checker_pt == PieceType::Pawn {
            return false;
        }

        let block_mask = self.between(checker_sq, self.kings[side.idx()].trailing_zeros() as u8);
        if (block_mask & bit(to)) != 0 {
             return self.is_pinned_move_legal(from, to, _pinned);
        }
        false
    }

    // ===================================================================
    // Helper: Pinned piece move validation
    // ===================================================================
    #[inline(always)]
    fn is_pinned_move_legal(&self, from: u8, to: u8, pinned: u64) -> bool {
        if (pinned & bit(from)) == 0 {
            return true;
        }
        let king_sq = self.kings[self.side_to_move.idx()].trailing_zeros() as u8;
        crate::magic::aligned(from, to, king_sq)
    }

    // ===================================================================
    // Helper: Legal when in check (single or double check)
    // ===================================================================
    #[inline(never)]
    fn is_legal_when_in_check(&self, m: Move) -> bool {
        let side = self.side_to_move;
        let (_, checkers) = self.pins_and_checkers(side);

        let num_checkers = count_bits(checkers);

        if num_checkers == 0 {
            return true; // should not happen
        }

        let from = m.from();
        let to = m.to();
        let pt = self.pieces[from as usize];

        // King moves are always legal if target is not attacked (must exclude original square from occupancy)
        if pt == PieceType::King {
            let occ = self.all_occupancy ^ bit(from);
            return !self.is_square_attacked_with_occ(to, side.opposite(), occ);
        }

        // Double check → only king moves are possible
        if num_checkers > 1 {
            return false;
        }

        // Single check → can capture the checker or block it
        let checker_sq = checkers.trailing_zeros() as u8;
        let checker_pt = self.pieces[checker_sq as usize];

        // Capture the checking piece?
        if to == checker_sq {
            return true; // pinned pieces already handled earlier
        }

        // Blocking (only possible against sliding pieces)
        if checker_pt == PieceType::Knight || checker_pt == PieceType::Pawn {
            return false; // cannot block
        }

        // Can we block?
        let block_mask = self.between(checker_sq, self.kings[side.idx()].trailing_zeros() as u8);
        (block_mask & bit(to)) != 0
    }

    /// Recomputes occupancy bitboards
    /// Must be called after any piece movement
    #[inline(always)]
    pub fn update_occupancy(&mut self) {
        let w = Color::White.idx();
        let b = Color::Black.idx();

        self.occupancy[w] =
            self.pawns[w]
            | self.knights[w]
            | self.bishops[w]
            | self.rooks[w]
            | self.queens[w]
            | self.kings[w];

        self.occupancy[b] =
            self.pawns[b]
            | self.knights[b]
            | self.bishops[b]
            | self.rooks[b]
            | self.queens[b]
            | self.kings[b];

        self.all_occupancy = self.occupancy[w] | self.occupancy[b];
    }

    pub fn to_fen(&self) -> String {
        let mut fen = String::new();

        // 1. Piece positions
        for row in (0..8).rev() {
            let mut empty = 0;
            for col in 0..8 {
                let sq = row * 8 + col;
                let pt = self.pieces[sq as usize];
                if pt != PieceType::Empty {
                    if empty > 0 {
                        fen.push_str(&empty.to_string());
                        empty = 0;
                    }
                    let mut c = match pt {
                        PieceType::Pawn => 'p',
                        PieceType::Knight => 'n',
                        PieceType::Bishop => 'b',
                        PieceType::Rook => 'r',
                        PieceType::Queen => 'q',
                        PieceType::King => 'k',
                        PieceType::Empty => '.',
                    };
                    if self.colors[sq as usize] == Color::White {
                        c = c.to_ascii_uppercase();
                    }
                    fen.push(c);
                } else {
                    empty += 1;
                }
            }
            if empty > 0 {
                fen.push_str(&empty.to_string());
            }
            if row > 0 {
                fen.push('/');
            }
        }

        // 2. Side to move
        fen.push(' ');
        fen.push(match self.side_to_move {
            Color::White => 'w',
            Color::Black => 'b',
            Color::None => '-',
        });

        // 3. Castling rights
        fen.push(' ');
        if self.castling_rights == 0 {
            fen.push('-');
        } else {
            if (self.castling_rights & castling::WHITE_KING) != 0 { fen.push('K'); }
            if (self.castling_rights & castling::WHITE_QUEEN) != 0 { fen.push('Q'); }
            if (self.castling_rights & castling::BLACK_KING) != 0 { fen.push('k'); }
            if (self.castling_rights & castling::BLACK_QUEEN) != 0 { fen.push('q'); }
        }

        // 4. En passant
        fen.push(' ');
        if let Some(sq) = self.en_passant_square {
            let col = (sq % 8) + b'a';
            let row = (sq / 8) + b'1';
            fen.push(col as char);
            fen.push(row as char);
        } else {
            fen.push('-');
        }

        // 5. Halfmove clock
        fen.push(' ');
        fen.push_str(&self.halfmove_clock.to_string());

        // 6. Fullmove number
        fen.push(' ');
        fen.push_str(&self.fullmove_number.to_string());

        fen
    }

    pub fn print_board(&self) {
        println!("  +-----------------+");
        for row in (0..8).rev() {
            print!("{} | ", row + 1);
            for col in 0..8 {
                let sq = row * 8 + col;
                let pt = self.pieces[sq as usize];
                if pt != PieceType::Empty {
                    let mut c = match pt {
                        PieceType::Pawn => 'p',
                        PieceType::Knight => 'n',
                        PieceType::Bishop => 'b',
                        PieceType::Rook => 'r',
                        PieceType::Queen => 'q',
                        PieceType::King => 'k',
                        PieceType::Empty => '.',
                    };
                    if self.colors[sq as usize] == Color::White {
                        c = c.to_ascii_uppercase();
                    }
                    print!("{} ", c);
                } else {
                    print!(". ");
                }
            }
            println!("|");
        }
        println!("  +-----------------+");
        println!("    a b c d e f g h");
        println!("FEN: {}", self.to_fen());
    }

    pub fn parse_move(&self, input: &str) -> Option<Move> {
        if input.len() < 4 { return None; }
        let bytes = input.as_bytes();
        let from_col = bytes[0] - b'a';
        let from_row = bytes[1] - b'1';
        let to_col = bytes[2] - b'a';
        let to_row = bytes[3] - b'1';

        if from_col > 7 || from_row > 7 || to_col > 7 || to_row > 7 {
            return None;
        }

        let from = from_row * 8 + from_col;
        let to = to_row * 8 + to_col;

        let legal_moves = crate::movegen::generate_legal_moves(self);
        for m in legal_moves {
            if m.from() == from && m.to() == to {
                if input.len() == 5 {
                    // Handle promotion
                    let promo = bytes[4] as char;
                    let f = m.flags();
                    let is_promo = match promo {
                        'n' => f == flags::PROMOTE_KNIGHT || f == flags::PROMOTE_KNIGHT_CAPTURE,
                        'b' => f == flags::PROMOTE_BISHOP || f == flags::PROMOTE_BISHOP_CAPTURE,
                        'r' => f == flags::PROMOTE_ROOK || f == flags::PROMOTE_ROOK_CAPTURE,
                        'q' => f == flags::PROMOTE_QUEEN || f == flags::PROMOTE_QUEEN_CAPTURE,
                        _ => false,
                    };
                    if is_promo { return Some(m); }
                } else {
                    // Not a promotion move
                    if m.flags() < 8 {
                        return Some(m);
                    }
                }
            }
        }
        None
    }

    #[inline(always)]
    pub fn remove_piece(&mut self, sq: u8, pt: PieceType, color: Color) {
        let bb = bit(sq);
        let mask = !bb;
        let c = color.idx();
        match pt {
            PieceType::Pawn => self.pawns[c] &= mask,
            PieceType::Knight => self.knights[c] &= mask,
            PieceType::Bishop => self.bishops[c] &= mask,
            PieceType::Rook => self.rooks[c] &= mask,
            PieceType::Queen => self.queens[c] &= mask,
            PieceType::King => self.kings[c] &= mask,
            PieceType::Empty => {},
        }
        self.pieces[sq as usize] = PieceType::Empty;
        self.colors[sq as usize] = Color::None;
        self.hash ^= ZOBRIST.hash_piece(color, pt, sq);

        let p_idx = pt.idx();
        let pst_sq = if color == Color::White { ((7 - (sq / 8)) * 8 + (sq % 8)) as usize } else { sq as usize };
        let mg = MG_VALUE[p_idx] + MG_PST[p_idx][pst_sq];
        let eg = EG_VALUE[p_idx] + EG_PST[p_idx][pst_sq];

        if color == Color::White {
            self.mg_pst -= mg;
            self.eg_pst -= eg;
        } else {
            self.mg_pst += mg;
            self.eg_pst += eg;
        }

        if pt == PieceType::Pawn {
            self.pawn_hash ^= ZOBRIST.hash_piece(color, pt, sq);
        }

        self.occupancy[c] &= mask;
        self.all_occupancy &= mask;
    }

    #[inline(always)]
    pub fn put_piece(&mut self, sq: u8, pt: PieceType, color: Color) {
        let bb = bit(sq);
        let c = color.idx();
        match pt {
            PieceType::Pawn => self.pawns[c] |= bb,
            PieceType::Knight => self.knights[c] |= bb,
            PieceType::Bishop => self.bishops[c] |= bb,
            PieceType::Rook => self.rooks[c] |= bb,
            PieceType::Queen => self.queens[c] |= bb,
            PieceType::King => self.kings[c] |= bb,
            PieceType::Empty => {},
        }
        self.pieces[sq as usize] = pt;
        self.colors[sq as usize] = color;
        self.hash ^= ZOBRIST.hash_piece(color, pt, sq);

        let p_idx = pt.idx();
        let pst_sq = if color == Color::White { ((7 - (sq / 8)) * 8 + (sq % 8)) as usize } else { sq as usize };
        let mg = MG_VALUE[p_idx] + MG_PST[p_idx][pst_sq];
        let eg = EG_VALUE[p_idx] + EG_PST[p_idx][pst_sq];

        if color == Color::White {
            self.mg_pst += mg;
            self.eg_pst += eg;
        } else {
            self.mg_pst -= mg;
            self.eg_pst -= eg;
        }

        if pt == PieceType::Pawn {
            self.pawn_hash ^= ZOBRIST.hash_piece(color, pt, sq);
        }

        self.occupancy[c] |= bb;
        self.all_occupancy |= bb;
    }

   pub fn make_move(&mut self, m: Move) -> UndoState {
           // Save current state before modifying
           let from = m.from();
           let to = m.to();
           let f = m.flags();
           let side = self.side_to_move;
           let enemy = side.opposite();

           let state = UndoState {
               castling_rights: self.castling_rights,
               en_passant_square: self.en_passant_square,
               halfmove_clock: self.halfmove_clock,
               captured_piece: self.pieces[to as usize],
               last_move: self.last_move,
               hash: self.hash,
               mg_pst: self.mg_pst,
               eg_pst: self.eg_pst,
               pawn_hash: self.pawn_hash,
           };

           self.last_move = Some(m);

           // 1. Identify moving piece
           let pt = self.pieces[from as usize];

           // 2. Handle captures
           if f == flags::EN_PASSANT {
               let cap_sq = if side == Color::White { to - 8 } else { to + 8 };
               self.remove_piece(cap_sq, PieceType::Pawn, enemy);
           } else if (f & flags::CAPTURE) != 0 {
               if state.captured_piece != PieceType::Empty {
                   self.remove_piece(to, state.captured_piece, enemy);
               }
           }

           // 3. Move the piece
           self.remove_piece(from, pt, side);

           let mut new_pt = pt;
           if (f & 0x8) != 0 { // Promotion
               new_pt = match f {
                   flags::PROMOTE_KNIGHT | flags::PROMOTE_KNIGHT_CAPTURE => PieceType::Knight,
                   flags::PROMOTE_BISHOP | flags::PROMOTE_BISHOP_CAPTURE => PieceType::Bishop,
                   flags::PROMOTE_ROOK   | flags::PROMOTE_ROOK_CAPTURE   => PieceType::Rook,
                   flags::PROMOTE_QUEEN  | flags::PROMOTE_QUEEN_CAPTURE  => PieceType::Queen,
                   _ => pt,
               };
           }
           self.put_piece(to, new_pt, side);

           // 4. Handle Castling
           if f == flags::KING_CASTLE {
               if side == Color::White {
                   self.remove_piece(7, PieceType::Rook, Color::White);
                   self.put_piece(5, PieceType::Rook, Color::White);
               } else {
                   self.remove_piece(63, PieceType::Rook, Color::Black);
                   self.put_piece(61, PieceType::Rook, Color::Black);
               }
           } else if f == flags::QUEEN_CASTLE {
               if side == Color::White {
                   self.remove_piece(0, PieceType::Rook, Color::White);
                   self.put_piece(3, PieceType::Rook, Color::White);
               } else {
                   self.remove_piece(56, PieceType::Rook, Color::Black);
                   self.put_piece(59, PieceType::Rook, Color::Black);
               }
           }

           // 5. Update Metadata
           self.hash ^= ZOBRIST.hash_castling(self.castling_rights);
           self.update_castling_rights(from, to);
           self.hash ^= ZOBRIST.hash_castling(self.castling_rights);

           self.hash ^= ZOBRIST.hash_en_passant(self.en_passant_square);
           if f == flags::DOUBLE_PAWN {
               self.en_passant_square = Some(if side == Color::White { from + 8 } else { from - 8 });
           } else {
               self.en_passant_square = None;
           }
           self.hash ^= ZOBRIST.hash_en_passant(self.en_passant_square);

           if pt == PieceType::Pawn || (f & flags::CAPTURE) != 0 {
               self.halfmove_clock = 0;
           } else {
               self.halfmove_clock += 1;
           }

           if side == Color::Black { self.fullmove_number += 1; }

           self.side_to_move = enemy;
           self.hash ^= ZOBRIST.hash_side();

           self.history.push(state.hash);

           state
       }

       pub fn is_repetition(&self) -> bool {
           if self.history.is_empty() { return false; }
           // Check if current hash has appeared before in history
           // Only need to check moves within the current halfmove clock range
           let start = self.history.len().saturating_sub(self.halfmove_clock as usize);
           for i in (start..self.history.len()).rev() {
               if self.history[i] == self.hash {
                   return true;
               }
           }
           false
       }

    pub fn unmake_move(&mut self, m: Move, state: UndoState) {
        self.castling_rights = state.castling_rights;
        self.en_passant_square = state.en_passant_square;
        self.halfmove_clock = state.halfmove_clock;
        self.hash = state.hash;
        self.mg_pst = state.mg_pst;
        self.eg_pst = state.eg_pst;
        self.pawn_hash = state.pawn_hash;
        if self.side_to_move == Color::White {
            self.fullmove_number -= 1;
        }
        self.side_to_move = self.side_to_move.opposite();
        self.last_move = state.last_move;
        self.history.pop();

        let side = self.side_to_move;
        let enemy = side.opposite();
        let from = m.from();
        let to = m.to();
        let f = m.flags();

        // 1. Identify piece that was moved (it's currently at 'to')
        let pt = self.pieces[to as usize];
        
        // 2. Remove from 'to'
        self.remove_piece_no_hash(to, pt, side);

        // 3. Put back at 'from'
        let original_pt = if (f & 0x8) != 0 { PieceType::Pawn } else { pt };
        self.put_piece_no_hash(from, original_pt, side);

        // 4. Restore captured piece
        if f == flags::EN_PASSANT {
            let cap_sq = if side == Color::White { to - 8 } else { to + 8 };
            self.put_piece_no_hash(cap_sq, PieceType::Pawn, enemy);
        } else if state.captured_piece != PieceType::Empty {
            self.put_piece_no_hash(to, state.captured_piece, enemy);
        }

        // 5. Handle Castling
        if f == flags::KING_CASTLE {
            if side == Color::White {
                self.remove_piece_no_hash(5, PieceType::Rook, Color::White);
                self.put_piece_no_hash(7, PieceType::Rook, Color::White);
            } else {
                self.remove_piece_no_hash(61, PieceType::Rook, Color::Black);
                self.put_piece_no_hash(63, PieceType::Rook, Color::Black);
            }
        } else if f == flags::QUEEN_CASTLE {
            if side == Color::White {
                self.remove_piece_no_hash(3, PieceType::Rook, Color::White);
                self.put_piece_no_hash(0, PieceType::Rook, Color::White);
            } else {
                self.remove_piece_no_hash(59, PieceType::Rook, Color::Black);
                self.put_piece_no_hash(56, PieceType::Rook, Color::Black);
            }
        }
    }

    pub fn make_null_move(&mut self) -> UndoState {
        let state = UndoState {
            castling_rights: self.castling_rights,
            en_passant_square: self.en_passant_square,
            halfmove_clock: self.halfmove_clock,
            captured_piece: PieceType::Empty,
            last_move: self.last_move,
            hash: self.hash,
            mg_pst: self.mg_pst,
            eg_pst: self.eg_pst,
            pawn_hash: self.pawn_hash,
        };

        self.last_move = None;
        self.history.push(self.hash);

        self.hash ^= ZOBRIST.hash_en_passant(self.en_passant_square);
        self.en_passant_square = None;
        self.hash ^= ZOBRIST.hash_en_passant(self.en_passant_square);

        if self.side_to_move == Color::Black { self.fullmove_number += 1; }
        self.side_to_move = self.side_to_move.opposite();
        self.hash ^= ZOBRIST.hash_side();
        
        state
    }

    pub fn unmake_null_move(&mut self, state: UndoState) {
        self.castling_rights = state.castling_rights;
        self.en_passant_square = state.en_passant_square;
        self.halfmove_clock = state.halfmove_clock;
        self.hash = state.hash;
        self.mg_pst = state.mg_pst;
        self.eg_pst = state.eg_pst;
        if self.side_to_move == Color::White {
            self.fullmove_number -= 1;
        }
        self.side_to_move = self.side_to_move.opposite();
        self.last_move = state.last_move;
        self.history.pop();
    }

    fn remove_piece_no_hash(&mut self, sq: u8, pt: PieceType, color: Color) {
        let bb = bit(sq);
        let c = color.idx();
        let mask = !bb;
        match pt {
            PieceType::Pawn => self.pawns[c] &= mask,
            PieceType::Knight => self.knights[c] &= mask,
            PieceType::Bishop => self.bishops[c] &= mask,
            PieceType::Rook => self.rooks[c] &= mask,
            PieceType::Queen => self.queens[c] &= mask,
            PieceType::King => self.kings[c] &= mask,
            PieceType::Empty => {},
        }
        self.pieces[sq as usize] = PieceType::Empty;
        self.colors[sq as usize] = Color::None;
        // PST, hash, and pawn_hash are restored from UndoState in unmake_move;
        // do NOT update them here.
        self.occupancy[c] &= mask;
        self.all_occupancy &= mask;
    }

    fn put_piece_no_hash(&mut self, sq: u8, pt: PieceType, color: Color) {
        let bb = bit(sq);
        let c = color.idx();
        match pt {
            PieceType::Pawn => self.pawns[c] |= bb,
            PieceType::Knight => self.knights[c] |= bb,
            PieceType::Bishop => self.bishops[c] |= bb,
            PieceType::Rook => self.rooks[c] |= bb,
            PieceType::Queen => self.queens[c] |= bb,
            PieceType::King => self.kings[c] |= bb,
            PieceType::Empty => {},
        }
        self.pieces[sq as usize] = pt;
        self.colors[sq as usize] = color;
        // PST, hash, and pawn_hash are restored from UndoState in unmake_move;
        // do NOT update them here.
        self.occupancy[c] |= bb;
        self.all_occupancy |= bb;
    }

       // Check if the move is legal AFTER it has been made
       pub fn is_in_check(&self, color: Color) -> bool {
           let king_bb = self.kings[color.idx()];
           if king_bb == 0 { return false; }
           let king_sq = king_bb.trailing_zeros() as u8;
           self.is_square_attacked(king_sq, color.opposite())
       }

    fn update_castling_rights(&mut self, from: u8, to: u8) {
        // Remove rights if king or rook moves
        match from {
            4 => self.castling_rights &= !(castling::WHITE_KING | castling::WHITE_QUEEN),
            60 => self.castling_rights &= !(castling::BLACK_KING | castling::BLACK_QUEEN),
            0 => self.castling_rights &= !castling::WHITE_QUEEN,
            7 => self.castling_rights &= !castling::WHITE_KING,
            56 => self.castling_rights &= !castling::BLACK_QUEEN,
            63 => self.castling_rights &= !castling::BLACK_KING,
            _ => {}
        }
        // Remove rights if rook is captured
        match to {
            0 => self.castling_rights &= !castling::WHITE_QUEEN,
            7 => self.castling_rights &= !castling::WHITE_KING,
            56 => self.castling_rights &= !castling::BLACK_QUEEN,
            63 => self.castling_rights &= !castling::BLACK_KING,
            _ => {}
        }
    }

    /// Returns total number of pieces on board
    #[inline(always)]
    pub fn piece_count(&self) -> u32 {
        count_bits(self.all_occupancy)
    }

    pub fn has_non_pawn_material(&self, color: Color) -> bool {
        let c = color.idx();
        self.knights[c] != 0 || self.bishops[c] != 0 || self.rooks[c] != 0 || self.queens[c] != 0
    }

    pub fn is_square_attacked(&self, sq: u8, attacker_color: Color) -> bool {
        self.is_square_attacked_with_occ(sq, attacker_color, self.all_occupancy)
    }

    pub fn is_square_attacked_with_occ(&self, sq: u8, attacker_color: Color, occ: u64) -> bool {
        let c = attacker_color.idx();

        // Pawns
        if (crate::movegen::pawn::get_pawn_attacks(sq, attacker_color.opposite()) & self.pawns[c]) != 0 {
            return true;
        }

        // Knights
        if (get_knight_attacks(sq) & self.knights[c]) != 0 { return true; }

        // Kings
        if (get_king_attacks(sq) & self.kings[c]) != 0 { return true; }

        // Sliding pieces
        if (get_bishop_attacks(sq, occ) & (self.bishops[c] | self.queens[c])) != 0 { return true; }
        if (get_rook_attacks(sq, occ) & (self.rooks[c] | self.queens[c])) != 0 { return true; }

        false
    }

    pub fn see(&self, m: Move) -> i32 {
        let to = m.to();
        let from = m.from();
        
        let mut gain = [0i32; 32];
        let mut d = 0;
        
        let mut attacker_pt = self.pieces[from as usize];
        gain[d] = self.see_value(self.pieces[to as usize]);
        
        let mut occ = self.all_occupancy;
        let mut attackers = self.all_attackers_to(to, occ);
        
        let mut side = self.side_to_move;
        
        // Remove the first attacker
        occ &= !bit(from);
        attackers &= !bit(from);
        
        // Update attackers that might be revealed (sliding pieces)
        if attacker_pt == PieceType::Pawn || attacker_pt == PieceType::Bishop || attacker_pt == PieceType::Queen {
            attackers |= self.get_revealed_attackers(to, from, occ) & (self.bishops[0] | self.bishops[1] | self.queens[0] | self.queens[1]);
        }
        if attacker_pt == PieceType::Rook || attacker_pt == PieceType::Queen {
            attackers |= self.get_revealed_attackers(to, from, occ) & (self.rooks[0] | self.rooks[1] | self.queens[0] | self.queens[1]);
        }

        while attackers != 0 {
            d += 1;
            side = side.opposite();
            
            let attacker_sq = self.least_valuable_attacker(attackers, side);
            if attacker_sq == 64 { break; }
            
            attacker_pt = self.pieces[attacker_sq as usize];
            gain[d] = self.see_value(attacker_pt) - gain[d - 1];
            
            if gain[d].max(gain[d-1]) < 0 { break; } // Optimization
            
            occ &= !bit(attacker_sq);
            attackers &= !bit(attacker_sq);
            
            // Reveal sliding attackers
            if attacker_pt == PieceType::Pawn || attacker_pt == PieceType::Bishop || attacker_pt == PieceType::Queen {
                attackers |= self.get_revealed_attackers(to, attacker_sq, occ) & (self.bishops[0] | self.bishops[1] | self.queens[0] | self.queens[1]);
            }
            if attacker_pt == PieceType::Rook || attacker_pt == PieceType::Queen {
                attackers |= self.get_revealed_attackers(to, attacker_sq, occ) & (self.rooks[0] | self.rooks[1] | self.queens[0] | self.queens[1]);
            }
            
            // Re-filter attackers by current occupancy (it might have changed by revealed pieces)
            attackers &= occ;
        }
        
        while d > 0 {
            gain[d - 1] = -( (-gain[d - 1]).max(gain[d]) );
            d -= 1;
        }
        
        gain[0]
    }

    fn see_value(&self, pt: PieceType) -> i32 {
        match pt {
            PieceType::Pawn => 100,
            PieceType::Knight => 320,
            PieceType::Bishop => 330,
            PieceType::Rook => 500,
            PieceType::Queen => 900,
            PieceType::King => 20000,
            PieceType::Empty => 0,
        }
    }

    fn all_attackers_to(&self, sq: u8, occ: u64) -> u64 {
        let mut attackers = 0u64;
        
        // Pawns
        attackers |= crate::movegen::pawn::get_pawn_attacks(sq, Color::Black) & self.pawns[0];
        attackers |= crate::movegen::pawn::get_pawn_attacks(sq, Color::White) & self.pawns[1];
        
        attackers |= get_knight_attacks(sq) & (self.knights[0] | self.knights[1]);
        attackers |= get_king_attacks(sq) & (self.kings[0] | self.kings[1]);
        attackers |= get_bishop_attacks(sq, occ) & (self.bishops[0] | self.bishops[1] | self.queens[0] | self.queens[1]);
        attackers |= get_rook_attacks(sq, occ) & (self.rooks[0] | self.rooks[1] | self.queens[0] | self.queens[1]);
        
        attackers
    }

    fn least_valuable_attacker(&self, attackers: u64, side: Color) -> u8 {
        let c = side.idx();
        let my_attackers = attackers & self.occupancy[c];
        if my_attackers == 0 { return 64; }
        
        if (my_attackers & self.pawns[c]) != 0 { return (my_attackers & self.pawns[c]).trailing_zeros() as u8; }
        if (my_attackers & self.knights[c]) != 0 { return (my_attackers & self.knights[c]).trailing_zeros() as u8; }
        if (my_attackers & self.bishops[c]) != 0 { return (my_attackers & self.bishops[c]).trailing_zeros() as u8; }
        if (my_attackers & self.rooks[c]) != 0 { return (my_attackers & self.rooks[c]).trailing_zeros() as u8; }
        if (my_attackers & self.queens[c]) != 0 { return (my_attackers & self.queens[c]).trailing_zeros() as u8; }
        if (my_attackers & self.kings[c]) != 0 { return (my_attackers & self.kings[c]).trailing_zeros() as u8; }
        
        64
    }

    fn get_revealed_attackers(&self, target_sq: u8, _from_sq: u8, occ: u64) -> u64 {
        // Simple way: check if target_sq and from_sq are on the same line/diagonal
        // and then return attackers on that line.
        
        // This is a bit complex to do perfectly without precomputed directions.
        // For SEE, we can just re-check Bishop/Rook/Queen attacks from target_sq.
        (get_bishop_attacks(target_sq, occ) & (self.bishops[0] | self.bishops[1] | self.queens[0] | self.queens[1])) |
        (get_rook_attacks(target_sq, occ) & (self.rooks[0] | self.rooks[1] | self.queens[0] | self.queens[1]))
    }

    pub fn pins_and_checkers(&self, color: Color) -> (u64, u64) {
        let mut pinned = 0u64;
        let mut checkers = 0u64;
        let side = color.idx();
        let enemy = color.opposite().idx();
        let king_sq = self.kings[side].trailing_zeros() as u8;
        if king_sq >= 64 { return (0, 0); }

        let occ = self.all_occupancy;

        // Checkers
        // Knights
        checkers |= get_knight_attacks(king_sq) & self.knights[enemy];
        // Pawns
        checkers |= crate::movegen::pawn::get_pawn_attacks(king_sq, color) & self.pawns[enemy];

        // Sliding checkers and pins
        let rook_attacks = get_rook_attacks(king_sq, occ);
        let bishop_attacks = get_bishop_attacks(king_sq, occ);

        checkers |= rook_attacks & (self.rooks[enemy] | self.queens[enemy]);
        checkers |= bishop_attacks & (self.bishops[enemy] | self.queens[enemy]);

        // Potential pinners (sliding pieces even if blocked)
        let p_rook = get_rook_attacks(king_sq, 0) & (self.rooks[enemy] | self.queens[enemy]);
        let p_bishop = get_bishop_attacks(king_sq, 0) & (self.bishops[enemy] | self.queens[enemy]);

        let mut pin_candidates = p_rook | p_bishop;
        while pin_candidates != 0 {
            let p_sq = pop_lsb(&mut pin_candidates);
            let between = self.between(king_sq, p_sq) & occ;
            if count_bits(between) == 1 {
                pinned |= between & self.occupancy[side];
            }
        }

        (pinned, checkers)
    }

    pub fn between(&self, s1: u8, s2: u8) -> u64 {
        crate::magic::between_bb(s1, s2)
    }

    pub fn compute_hash(&self) -> u64 {
        let mut h = 0u64;
        for sq in 0..64 {
            let pt = self.pieces[sq];
            if pt != PieceType::Empty {
                h ^= ZOBRIST.hash_piece(self.colors[sq], pt, sq as u8);
            }
        }
        if self.side_to_move == Color::Black {
            h ^= ZOBRIST.hash_side();
        }
        h ^= ZOBRIST.hash_castling(self.castling_rights);
        h ^= ZOBRIST.hash_en_passant(self.en_passant_square);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_startpos_piece_count() {
        let b = Board::startpos();
        assert_eq!(b.piece_count(), 32);
    }

    #[test]
    fn test_white_pawns_position() {
        let b = Board::startpos();
        assert_eq!(b.pawns[0], 0x000000000000FF00);
    }

    #[test]
    fn test_black_pawns_position() {
        let b = Board::startpos();
        assert_eq!(b.pawns[1], 0x00FF000000000000);
    }

    #[test]
    fn test_make_move() {
        let mut b = Board::startpos();
        // Move e2 to e4 (White pawn double push)
        // e2 = 12, e4 = 28
        let m = Move::new(12, 28, flags::DOUBLE_PAWN);
        let _state = b.make_move(m);

        assert_eq!(b.pieces[12], PieceType::Empty);
        assert_eq!(b.pieces[28], PieceType::Pawn);
        assert_eq!(b.en_passant_square, Some(20));
        assert_eq!(b.side_to_move, Color::Black);
    }

    #[test]
    fn test_occupancy_updates() {
        let mut b = Board::startpos();

        // Remove a pawn manually
        b.remove_piece(8, PieceType::Pawn, Color::White);

        b.update_occupancy();

        assert_eq!(b.piece_count(), 31);
    }

    #[test]
    fn test_fen_conversion() {
        let b = Board::startpos();
        let fen = b.to_fen();
        assert_eq!(fen, "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    }

    #[test]
    fn test_fen_parsing() {
        let fen = "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq c6 0 2";
        let b = Board::from_fen(fen).unwrap();
        assert_eq!(b.to_fen(), fen);
        assert_eq!(b.en_passant_square, Some(42)); // c6 = 42
    }

    #[test]
    fn test_parse_move() {
        let b = Board::startpos();
        let m = b.parse_move("e2e4").unwrap();
        assert_eq!(m.from(), 12);
        assert_eq!(m.to(), 28);
        assert_eq!(m.flags(), flags::DOUBLE_PAWN);
    }
}