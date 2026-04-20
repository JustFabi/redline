use crate::board::r#move::Move;

pub const MAX_MOVES: usize = 256;

pub struct MoveList {
    moves: [Move; MAX_MOVES],
    count: usize,
}

impl MoveList {
    pub const fn new() -> Self {
        Self {
            moves: [Move::from_raw(0); MAX_MOVES],
            count: 0,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, m: Move) {
        if self.count < MAX_MOVES {
            self.moves[self.count] = m;
            self.count += 1;
        }
    }

    #[inline(always)]
    pub fn swap(&mut self, i: usize, j: usize) {
        self.moves.swap(i, j);
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline(always)]
    pub fn get(&self, index: usize) -> Move {
        self.moves[index]
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.count]
    }

    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [Move] {
        &mut self.moves[..self.count]
    }
}

impl IntoIterator for MoveList {
    type Item = Move;
    type IntoIter = MoveListIter;

    fn into_iter(self) -> Self::IntoIter {
        MoveListIter {
            move_list: self,
            index: 0,
        }
    }
}

pub struct MoveListIter {
    move_list: MoveList,
    index: usize,
}

impl Iterator for MoveListIter {
    type Item = Move;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.move_list.count {
            let m = self.move_list.moves[self.index];
            self.index += 1;
            Some(m)
        } else {
            None
        }
    }
}
