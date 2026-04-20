use crate::board::bitboard::*;

// Maximum bits in the mask: Rooks need 12 bits (4096 permutations), Bishops need 9 (512).
static mut BISHOP_MASKS: [u64; 64] = [0; 64];
static mut ROOK_MASKS: [u64; 64] = [0; 64];
static mut BISHOP_MAGICS: [u64; 64] = [0; 64];
static mut ROOK_MAGICS: [u64; 64] = [0; 64];
static mut BISHOP_SHIFTS: [u32; 64] = [0; 64];
static mut ROOK_SHIFTS: [u32; 64] = [0; 64];

static mut BISHOP_ATTACKS: [[u64; 512]; 64] = [[0; 512]; 64];
static mut ROOK_ATTACKS: [[u64; 4096]; 64] = [[0; 4096]; 64];

pub static mut LINE_BB: [[u64; 64]; 64] = [[0; 64]; 64];
pub static mut BETWEEN_BB: [[u64; 64]; 64] = [[0; 64]; 64];

/// Fast lookup for Bishop attacks
#[inline(always)]
pub fn get_bishop_attacks(sq: u8, occ: u64) -> u64 {
    unsafe {
        let mask = BISHOP_MASKS[sq as usize];
        let magic = BISHOP_MAGICS[sq as usize];
        let shift = BISHOP_SHIFTS[sq as usize];
        let index = ((occ & mask).wrapping_mul(magic)) >> shift;
        BISHOP_ATTACKS[sq as usize][index as usize]
    }
}

/// Fast lookup for Rook attacks
#[inline(always)]
pub fn get_rook_attacks(sq: u8, occ: u64) -> u64 {
    unsafe {
        let mask = ROOK_MASKS[sq as usize];
        let magic = ROOK_MAGICS[sq as usize];
        let shift = ROOK_SHIFTS[sq as usize];
        let index = ((occ & mask).wrapping_mul(magic)) >> shift;
        ROOK_ATTACKS[sq as usize][index as usize]
    }
}

/// Fast lookup for Queen attacks
#[inline(always)]
pub fn get_queen_attacks(sq: u8, occ: u64) -> u64 {
    get_bishop_attacks(sq, occ) | get_rook_attacks(sq, occ)
}

#[inline(always)]
pub fn aligned(s1: u8, s2: u8, s3: u8) -> bool {
    unsafe { (LINE_BB[s1 as usize][s2 as usize] & bit(s3)) != 0 }
}

#[inline(always)]
pub fn between_bb(s1: u8, s2: u8) -> u64 {
    unsafe { BETWEEN_BB[s1 as usize][s2 as usize] }
}

/// Initialize Magic Bitboards. Call this exactly once at startup!
pub fn init_magics() {
    for sq in 0..64 {
        unsafe {
            BISHOP_MASKS[sq] = mask_bishop(sq as u8);
            ROOK_MASKS[sq] = mask_rook(sq as u8);

            BISHOP_SHIFTS[sq] = 64 - BISHOP_MASKS[sq].count_ones();
            ROOK_SHIFTS[sq] = 64 - ROOK_MASKS[sq].count_ones();

            BISHOP_MAGICS[sq] = find_magic(sq as u8, BISHOP_MASKS[sq].count_ones(), true);
            ROOK_MAGICS[sq] = find_magic(sq as u8, ROOK_MASKS[sq].count_ones(), false);
        }
    }
    
    // Initialize line and between boards
    compute_line_and_between();
}

// ==========================================
// INTERNAL HELPERS FOR INITIALIZATION
// ==========================================

/// Masks for rooks (exclude outer edges because they don't block visibility)
fn mask_rook(sq: u8) -> u64 {
    let mut attacks = 0u64;
    let r = (sq / 8) as i8;
    let c = (sq % 8) as i8;
    for nr in (r + 1)..7 { attacks |= bit((nr * 8 + c) as u8); } // Up
    for nr in 1..r { attacks |= bit((nr * 8 + c) as u8); }       // Down
    for nc in (c + 1)..7 { attacks |= bit((r * 8 + nc) as u8); } // Right
    for nc in 1..c { attacks |= bit((r * 8 + nc) as u8); }       // Left
    attacks
}

/// Masks for bishops (exclude outer edges)
fn mask_bishop(sq: u8) -> u64 {
    let mut attacks = 0u64;
    let r = (sq / 8) as i8;
    let c = (sq % 8) as i8;
    for &(dr, dc) in &[(1, 1), (1, -1), (-1, 1), (-1, -1)] {
        let mut nr = r + dr;
        let mut nc = c + dc;
        while nr > 0 && nr < 7 && nc > 0 && nc < 7 {
            attacks |= bit((nr * 8 + nc) as u8);
            nr += dr;
            nc += dc;
        }
    }
    attacks
}

/// Your original slow attack logic, used *only* during startup to populate the tables.
fn slow_bishop_attacks(sq: u8, occ: u64) -> u64 {
    let mut attacks = 0u64;
    let r = (sq / 8) as i8;
    let c = (sq % 8) as i8;
    for &(dr, dc) in &[(1, 1), (1, -1), (-1, 1), (-1, -1)] {
        let mut nr = r + dr;
        let mut nc = c + dc;
        while nr >= 0 && nr < 8 && nc >= 0 && nc < 8 {
            let nsq = (nr * 8 + nc) as u8;
            attacks |= bit(nsq);
            if (bit(nsq) & occ) != 0 { break; }
            nr += dr;
            nc += dc;
        }
    }
    attacks
}

/// Your original slow attack logic, used *only* during startup to populate the tables.
fn slow_rook_attacks(sq: u8, occ: u64) -> u64 {
    let mut attacks = 0u64;
    let r = (sq / 8) as i8;
    let c = (sq % 8) as i8;
    for &(dr, dc) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let mut nr = r + dr;
        let mut nc = c + dc;
        while nr >= 0 && nr < 8 && nc >= 0 && nc < 8 {
            let nsq = (nr * 8 + nc) as u8;
            attacks |= bit(nsq);
            if (bit(nsq) & occ) != 0 { break; }
            nr += dr;
            nc += dc;
        }
    }
    attacks
}

fn compute_line_and_between() {
    for s1 in 0..64 {
        for s2 in 0..64 {
            if s1 == s2 { continue; }
            
            let r1 = (s1 / 8) as i8;
            let c1 = (s1 % 8) as i8;
            let r2 = (s2 / 8) as i8;
            let c2 = (s2 % 8) as i8;
            
            let dr = r2 - r1;
            let dc = c2 - c1;
            
            if dr == 0 || dc == 0 || dr.abs() == dc.abs() {
                // They are aligned
                let step_r = dr.signum();
                let step_c = dc.signum();
                
                // Full line
                let mut line = 0u64;
                let mut r = r1;
                let mut c = c1;
                // Go backwards to start of line
                while r - step_r >= 0 && r - step_r < 8 && c - step_c >= 0 && c - step_c < 8 {
                    r -= step_r;
                    c -= step_c;
                }
                // Trace the whole line
                while r >= 0 && r < 8 && c >= 0 && c < 8 {
                    line |= bit((r * 8 + c) as u8);
                    r += step_r;
                    c += step_c;
                }
                
                // Between
                let mut between = 0u64;
                r = r1 + step_r;
                c = c1 + step_c;
                while r != r2 || c != c2 {
                    between |= bit((r * 8 + c) as u8);
                    r += step_r;
                    c += step_c;
                }
                
                unsafe {
                    LINE_BB[s1 as usize][s2 as usize] = line;
                    BETWEEN_BB[s1 as usize][s2 as usize] = between;
                }
            }
        }
    }
}

/// Maps an index (e.g. 0 to 4095) to a specific board occupancy layout
fn set_occupancy(index: usize, bits_in_mask: u32, mut mask: u64) -> u64 {
    let mut occ = 0u64;
    for i in 0..bits_in_mask {
        let sq = mask.trailing_zeros() as u8;
        mask &= mask - 1; // clear lsb
        if (index & (1 << i)) != 0 {
            occ |= bit(sq);
        }
    }
    occ
}

// Very fast XOR-Shift RNG to generate candidate magics
struct XorShift64 { state: u64 }
impl XorShift64 {
    fn new(seed: u64) -> Self { Self { state: seed } }
    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
    // Sparse randoms work best for finding magics
    fn next_fewbits(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

/// Bruteforces a magic number for a square and piece type
fn find_magic(sq: u8, bits: u32, is_bishop: bool) -> u64 {
    let mask = if is_bishop { mask_bishop(sq) } else { mask_rook(sq) };
    let num_occupancies = 1 << bits;
    let mut occupancies = vec![0u64; num_occupancies];
    let mut attacks = vec![0u64; num_occupancies];

    for i in 0..num_occupancies {
        occupancies[i] = set_occupancy(i, bits, mask);
        attacks[i] = if is_bishop { slow_bishop_attacks(sq, occupancies[i]) }
                     else { slow_rook_attacks(sq, occupancies[i]) };
    }

    let mut rng = XorShift64::new(123456789); // Fixed seed ensures fast, deterministic boot

    loop {
        let magic = rng.next_fewbits();
        // Skip bad magics early
        if (mask.wrapping_mul(magic) & 0xFF00000000000000).count_ones() < 6 { continue; }

        let mut used = vec![0xFFFF_FFFF_FFFF_FFFF; num_occupancies];
        let mut success = true;

        for i in 0..num_occupancies {
            let index = (occupancies[i].wrapping_mul(magic)) >> (64 - bits);
            if used[index as usize] == 0xFFFF_FFFF_FFFF_FFFF {
                used[index as usize] = attacks[i];
            } else if used[index as usize] != attacks[i] {
                // Collision!
                success = false;
                break;
            }
        }

        if success {
            unsafe {
                for i in 0..num_occupancies {
                    let index = (occupancies[i].wrapping_mul(magic)) >> (64 - bits);
                    if is_bishop {
                        BISHOP_ATTACKS[sq as usize][index as usize] = attacks[i];
                    } else {
                        ROOK_ATTACKS[sq as usize][index as usize] = attacks[i];
                    }
                }
            }
            return magic;
        }
    }
}