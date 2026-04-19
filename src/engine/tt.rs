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
}

pub struct TranspositionTable {
    table: Vec<AtomicU64>,
    size: usize,
}

impl TranspositionTable {
    pub fn new(mb: usize) -> Self {
        let size = mb * 1024 * 1024 / 16;
        let mut table = Vec::with_capacity(size * 2);
        for _ in 0..size * 2 {
            table.push(AtomicU64::new(0));
        }
        Self { table, size }
    }

    pub fn store(&self, key: u64, depth: u8, score: i32, node_type: NodeType, best_move: Option<Move>) {
        let idx = (key as usize % self.size) * 2;
        
        let mut data = 0u64;
        data |= (depth as u64) & 0xFF;
        data |= ((score as u64) & 0xFFFFFFFF) << 8;
        let type_val = match node_type {
            NodeType::Exact => 0,
            NodeType::Alpha => 1,
            NodeType::Beta => 2,
        };
        data |= (type_val << 40) & 0xFF0000000000;
        if let Some(m) = best_move {
            data |= (m.raw() as u64) << 48;
        }

        // Write key second to ensure we don't read partial data for a different key
        self.table[idx + 1].store(data, Ordering::Relaxed);
        self.table[idx].store(key, Ordering::Release);
    }

    pub fn probe(&self, key: u64) -> Option<TTEntry> {
        let idx = (key as usize % self.size) * 2;
        
        let stored_key = self.table[idx].load(Ordering::Acquire);
        if stored_key != key {
            return None;
        }
        
        let data = self.table[idx + 1].load(Ordering::Relaxed);
        
        let depth = (data & 0xFF) as u8;
        let score = (data >> 8) as i32;
        let type_val = (data >> 40) & 0x3;
        let node_type = match type_val {
            0 => NodeType::Exact,
            1 => NodeType::Alpha,
            2 => NodeType::Beta,
            _ => NodeType::Exact,
        };
        let move_raw = (data >> 48) as u16;
        let best_move = if move_raw != 0 { Some(Move::from_raw(move_raw)) } else { None };

        Some(TTEntry {
            key,
            depth,
            score,
            node_type,
            best_move,
        })
    }

    pub fn hashfull(&self) -> usize {
        let mut occupied = 0;
        let sample_size = self.size.min(1000);
        if sample_size == 0 { return 0; }
        for i in 0..sample_size {
            if self.table[i * 2].load(Ordering::Relaxed) != 0 {
                occupied += 1;
            }
        }
        (occupied * 1000) / sample_size
    }
}
