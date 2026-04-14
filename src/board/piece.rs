// Defines basic chess concepts: Color and Piece.

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)] // Ensures predictable integer layout (important for performance)
pub enum Color {
    White = 0,
    Black = 1,
}

impl Color {
    #[inline(always)] 
    pub fn opposite(self) -> Self {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    #[inline(always)]
    pub fn idx(self) -> usize {
        self as usize
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum PieceType {
    Pawn = 0,
    Knight = 1,
    Bishop = 2,
    Rook = 3,
    Queen = 4,
    King = 5,
}

impl PieceType {
    #[inline(always)]
    pub fn idx(self) -> usize {
        self as usize
    }
}