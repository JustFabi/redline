use rand::Rng;
use crate::board::piece::{Color, PieceType};

pub struct Zobrist {
    pub pieces: [[[u64; 64]; 6]; 2],
    pub side_to_move: u64,
    pub castling: [u64; 16],
    pub en_passant: [u64; 65], // 64 for squares + 1 for None
}

lazy_static::lazy_static! {
    pub static ref ZOBRIST: Zobrist = Zobrist::new();
}

impl Zobrist {
    fn new() -> Self {
        let mut rng = rand::thread_rng();
        let mut pieces = [[[0u64; 64]; 6]; 2];
        for c in 0..2 {
            for p in 0..6 {
                for s in 0..64 {
                    pieces[c][p][s] = rng.r#gen();
                }
            }
        }

        let side_to_move = rng.r#gen();

        let mut castling = [0u64; 16];
        for i in 0..16 {
            castling[i] = rng.r#gen();
        }

        let mut en_passant = [0u64; 65];
        for i in 0..65 {
            en_passant[i] = rng.r#gen();
        }

        Self {
            pieces,
            side_to_move,
            castling,
            en_passant,
        }
    }

    pub fn hash_piece(&self, color: Color, piece: PieceType, sq: u8) -> u64 {
        self.pieces[color.idx()][piece.idx()][sq as usize]
    }

    pub fn hash_side(&self) -> u64 {
        self.side_to_move
    }

    pub fn hash_castling(&self, rights: u8) -> u64 {
        self.castling[rights as usize]
    }

    pub fn hash_en_passant(&self, sq: Option<u8>) -> u64 {
        match sq {
            Some(s) => self.en_passant[s as usize],
            None => self.en_passant[64],
        }
    }
}
