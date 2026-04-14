// This file contains low-level bitboard utilities.
#[inline(always)]
pub const fn bit(square: u8) -> u64 {
    1u64 << square
}

/// Pops the least significant 1-bit and returns its index.
/// Example:
/// bb = 0b101000 -> returns 3, bb becomes 0b100000
#[inline(always)]
pub fn pop_lsb(bb: &mut u64) -> u8 {
    let sq = bb.trailing_zeros() as u8;
    *bb &= *bb - 1; // clears lowest set bit
    sq
}

/// Counts number of set bits (number of pieces)
#[inline(always)]
pub fn count_bits(bb: u64) -> u32 {
    bb.count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit() {
        assert_eq!(bit(0), 1);
        assert_eq!(bit(1), 2);
        assert_eq!(bit(7), 128);
    }

    #[test]
    fn test_pop_lsb() {
        let mut bb = 0b101000;
        let sq = pop_lsb(&mut bb);
        assert_eq!(sq, 3);
        assert_eq!(bb, 0b100000);
    }

    #[test]
    fn test_count_bits() {
        assert_eq!(count_bits(0b1111), 4);
        assert_eq!(count_bits(0), 0);
    }
}