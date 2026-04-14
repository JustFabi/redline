/// 16-bit Move structure
/// 0-5: from (6 bits)
/// 6-11: to (6 bits)
/// 12-15: flags (4 bits)
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Move(u16);

impl std::fmt::Display for Move {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let from_sq = self.from();
        let to_sq = self.to();
        let from_col = (from_sq % 8) + b'a';
        let from_row = (from_sq / 8) + b'1';
        let to_col = (to_sq % 8) + b'a';
        let to_row = (to_sq / 8) + b'1';
        
        write!(f, "{}{}{}{}", from_col as char, from_row as char, to_col as char, to_row as char)?;
        
        match self.flags() {
            flags::PROMOTE_KNIGHT | flags::PROMOTE_KNIGHT_CAPTURE => write!(f, "n"),
            flags::PROMOTE_BISHOP | flags::PROMOTE_BISHOP_CAPTURE => write!(f, "b"),
            flags::PROMOTE_ROOK | flags::PROMOTE_ROOK_CAPTURE => write!(f, "r"),
            flags::PROMOTE_QUEEN | flags::PROMOTE_QUEEN_CAPTURE => write!(f, "q"),
            _ => Ok(()),
        }
    }
}

impl Move {
    #[inline(always)]
    pub fn new(from: u8, to: u8, flags: u8) -> Self {
        Self((from as u16) | ((to as u16) << 6) | ((flags as u16) << 12))
    }

    #[inline(always)]
    pub fn from(self) -> u8 {
        (self.0 & 0x3F) as u8
    }

    #[inline(always)]
    pub fn to(self) -> u8 {
        ((self.0 >> 6) & 0x3F) as u8
    }

    #[inline(always)]
    pub fn flags(self) -> u8 {
        (self.0 >> 12) as u8
    }

    #[inline(always)]
    pub fn raw(self) -> u16 {
        self.0
    }

    #[inline(always)]
    pub fn from_raw(raw: u16) -> Self {
        Self(raw)
    }
}

pub mod flags {
    pub const QUIET: u8 = 0x0;
    pub const DOUBLE_PAWN: u8 = 0x1;
    pub const KING_CASTLE: u8 = 0x2;
    pub const QUEEN_CASTLE: u8 = 0x3;
    pub const CAPTURE: u8 = 0x4;
    pub const EN_PASSANT: u8 = 0x5;
    
    // Promotions
    pub const PROMOTE_KNIGHT: u8 = 0x8;
    pub const PROMOTE_BISHOP: u8 = 0x9;
    pub const PROMOTE_ROOK: u8 = 0xA;
    pub const PROMOTE_QUEEN: u8 = 0xB;
    
    // Promotion Captures
    pub const PROMOTE_KNIGHT_CAPTURE: u8 = 0xC;
    pub const PROMOTE_BISHOP_CAPTURE: u8 = 0xD;
    pub const PROMOTE_ROOK_CAPTURE: u8 = 0xE;
    pub const PROMOTE_QUEEN_CAPTURE: u8 = 0xF;
}
