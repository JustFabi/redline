use std::sync::atomic::{AtomicU64, Ordering};
use crate::board::r#move::Move;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Exact,
    Alpha, // All-node
    Beta,  // Cut-node
}

#[derive(Clone, Copy)]
pub struct TTEntry {
    #[allow(dead_code)]
    pub key: u64,
    pub depth: u8,
    pub score: i32,
    pub node_type: NodeType,
    pub best_move: Option<Move>,
    #[allow(dead_code)]
    pub age: u8,
}

pub struct TranspositionTable {
    table: Vec<AtomicU64>,
    mask: usize,
}

impl TranspositionTable {
    pub fn new(mb: usize) -> Self {
        // 32 bytes per bucket (4 u64s)
        let bucket_count = (mb * 1024 * 1024 / 32).max(1).next_power_of_two();
        let mut table = Vec::with_capacity(bucket_count * 4);
        for _ in 0..bucket_count * 4 {
            table.push(AtomicU64::new(0));
        }
        Self { table, mask: bucket_count - 1 }
    }

    #[inline(always)]
    pub fn store(&self, key: u64, depth: u8, score: i32, node_type: NodeType, best_move: Option<Move>, age: u8) {
        let idx = (key as usize & self.mask) * 4;
        
        let mut data = 0u64;
        data |= (depth as u64) & 0xFF;
        data |= ((score as u64) & 0xFFFFFFFF) << 8;
        let type_val = match node_type {
            NodeType::Exact => 0,
            NodeType::Alpha => 1,
            NodeType::Beta => 2,
        };
        data |= (type_val << 40) & 0x30000000000;
        data |= ((age as u64) & 0x3F) << 42;
        if let Some(m) = best_move {
            data |= (m.raw() as u64) << 48;
        }

        let key_xor1 = self.table[idx].load(Ordering::Relaxed);
        let data1 = self.table[idx + 1].load(Ordering::Relaxed);
        let key1 = key_xor1 ^ data1;

        let key_xor2 = self.table[idx + 2].load(Ordering::Relaxed);
        let data2 = self.table[idx + 3].load(Ordering::Relaxed);
        let key2 = key_xor2 ^ data2;

        // Always update if same key
        if key1 == key {
            self.table[idx + 1].store(data, Ordering::Relaxed);
            self.table[idx].store(key ^ data, Ordering::Release);
            return;
        }
        if key2 == key {
            self.table[idx + 3].store(data, Ordering::Relaxed);
            self.table[idx + 2].store(key ^ data, Ordering::Release);
            return;
        }

        // Slot 2 is Depth-Preferred
        let existing_depth2 = (data2 & 0xFF) as u8;
        
        if depth >= existing_depth2 {
            self.table[idx + 3].store(data, Ordering::Relaxed);
            self.table[idx + 2].store(key ^ data, Ordering::Release);
        } else {
            // Slot 1 is Always-Replace
            self.table[idx + 1].store(data, Ordering::Relaxed);
            self.table[idx].store(key ^ data, Ordering::Release);
        }
    }

    #[inline(always)]
    pub fn probe(&self, key: u64) -> Option<TTEntry> {
        let idx = (key as usize & self.mask) * 4;
        
        let mut found_data = None;

        let key_xor1 = self.table[idx].load(Ordering::Acquire);
        let data1 = self.table[idx + 1].load(Ordering::Relaxed);
        if key_xor1 ^ data1 == key {
            found_data = Some(data1);
        } else {
            let key_xor2 = self.table[idx + 2].load(Ordering::Acquire);
            let data2 = self.table[idx + 3].load(Ordering::Relaxed);
            if key_xor2 ^ data2 == key {
                found_data = Some(data2);
            }
        }

        if let Some(data) = found_data {
            let depth = (data & 0xFF) as u8;
            let score = ((data >> 8) & 0xFFFFFFFF) as u32 as i32;
            let type_val = (data >> 40) & 0x3;
            let age = ((data >> 42) & 0x3F) as u8;
            let node_type = match type_val {
                0 => NodeType::Exact,
                1 => NodeType::Alpha,
                2 => NodeType::Beta,
                _ => NodeType::Exact,
            };
            let move_raw = (data >> 48) as u16;
            let best_move = if move_raw != 0 { Some(Move::from_raw(move_raw)) } else { None };

            return Some(TTEntry {
                key,
                depth,
                score,
                node_type,
                best_move,
                age,
            });
        }
        None
    }

    pub fn hashfull(&self) -> usize {
        let mut occupied = 0;
        let sample_size = (self.mask + 1).min(1000);
        if sample_size == 0 { return 0; }
        for i in 0..sample_size {
            // Count bucket as occupied if either slot is used (non-zero key_xor)
            if self.table[i * 4].load(Ordering::Relaxed) != 0 || self.table[i * 4 + 2].load(Ordering::Relaxed) != 0 {
                occupied += 1;
            }
        }
        (occupied * 1000) / sample_size
    }
}
