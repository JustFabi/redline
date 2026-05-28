use crate::board::board::Board;
use crate::board::r#move::Move;
use crate::movegen;
use crate::movegen::move_list::MoveList;
use crate::engine::search::Searcher;

#[derive(PartialEq, Clone, Copy)]
enum Stage {
    TT,
    GenerateCaptures,
    GoodCaptures,
    Killers,
    Countermove,
    GenerateQuiets,
    Quiets,
    BadCaptures,
}

pub struct MovePicker {
    stage: Stage,
    tt_move: Option<Move>,
    moves: MoveList,
    scores: [i32; 256],
    bad_captures: MoveList,
    index: usize,
    is_qsearch: bool,
    excluded_move: Option<Move>,
    in_check: bool,
    pub skip_quiets: bool,
    killers_to_search: [Move; 2],
    killers_count: usize,
    countermove_to_search: Option<Move>,
    original_killers: [Option<Move>; 2],
    original_countermove: Option<Move>,
}
impl MovePicker {
    #[inline(always)]
    pub fn new(tt_move: Option<Move>, is_qsearch: bool, in_check: bool, killers: [Option<Move>; 2], countermove: Option<Move>) -> Self {
        let mut picker = Self {
            stage: Stage::TT,
            tt_move,
            moves: MoveList::new(),
            scores: [0; 256],
            bad_captures: MoveList::new(),
            index: 0,
            is_qsearch,
            excluded_move: None,
            in_check,
            skip_quiets: false,
            killers_to_search: [Move::from_raw(0); 2],
            killers_count: 0,
            countermove_to_search: None,
            original_killers: killers,
            original_countermove: countermove,
        };
        if let Some(m) = killers[0] {
            if Some(m) != tt_move {
                picker.killers_to_search[picker.killers_count] = m;
                picker.killers_count += 1;
            }
        }
        if let Some(m) = killers[1] {
            if Some(m) != tt_move && killers[0] != Some(m) {
                picker.killers_to_search[picker.killers_count] = m;
                picker.killers_count += 1;
            }
        }
        if let Some(m) = countermove {
            if Some(m) != tt_move && killers[0] != Some(m) && killers[1] != Some(m) {
                picker.countermove_to_search = Some(m);
            }
        }
        picker
    }

    #[inline(always)]
    pub fn with_excluded(tt_move: Option<Move>, excluded: Option<Move>, in_check: bool) -> Self {
        Self {
            stage: Stage::TT,
            tt_move,
            moves: MoveList::new(),
            scores: [0; 256],
            bad_captures: MoveList::new(),
            index: 0,
            is_qsearch: false,
            excluded_move: excluded,
            in_check,
            skip_quiets: false,
            killers_to_search: [Move::from_raw(0); 2],
            killers_count: 0,
            countermove_to_search: None,
            original_killers: [None; 2],
            original_countermove: None,
        }
    }

    #[inline(always)]
    pub fn next(&mut self, searcher: &Searcher, board: &Board, ply: u32) -> Option<Move> {
        loop {
            match self.stage {
                Stage::TT => {
                    self.stage = Stage::GenerateCaptures;
                    if let Some(m) = self.tt_move {
                        if Some(m) == self.excluded_move { continue; }
                        if board.is_pseudo_legal(m) {
                            return Some(m);
                        }
                    }
                }
                Stage::GenerateCaptures => {
                    self.moves = if self.in_check {
                        movegen::generate_evasions(board)
                    } else {
                        movegen::generate_captures(board)
                    };
                    
                    for i in 0..self.moves.len() {
                        let m = self.moves.get(i);
                        let score = searcher.score_move(m, board, self.tt_move, ply, self.is_qsearch);
                        self.scores[i] = score;
                    }
                    self.index = 0;
                    self.stage = Stage::GoodCaptures;
                }
                Stage::GoodCaptures => {
                    while self.index < self.moves.len() {
                        searcher.pick_move(self.moves.as_mut_slice(), &mut self.scores, self.index);
                        let m = self.moves.get(self.index);
                        let score = self.scores[self.index];
                        self.index += 1;
                        if Some(m) == self.tt_move || Some(m) == self.excluded_move { continue; }

                        if self.in_check {
                            return Some(m);
                        }

                        let margin = -score / 18;
                        if board.see(m) >= margin {
                            return Some(m);
                        } else {
                            self.bad_captures.push(m);
                        }
                    }
                    
                    if self.in_check || self.is_qsearch {
                        return None;
                    }
                    
                    self.stage = Stage::Killers;
                }
                Stage::Killers => {
                    if self.killers_count > 0 {
                        self.killers_count -= 1;
                        let m = self.killers_to_search[self.killers_count];
                        if board.is_pseudo_legal(m) {
                            return Some(m);
                        }
                        continue;
                    }
                    self.stage = Stage::Countermove;
                }
                Stage::Countermove => {
                    self.stage = Stage::GenerateQuiets;
                    if let Some(m) = self.countermove_to_search {
                        if board.is_pseudo_legal(m) {
                            return Some(m);
                        }
                    }
                }
                Stage::GenerateQuiets => {
                    if self.skip_quiets {
                        self.stage = Stage::BadCaptures;
                        self.index = 0;
                        continue;
                    }
                    self.moves = movegen::generate_quiets(board);
                    for i in 0..self.moves.len() {
                        let m = self.moves.get(i);
                        let score = searcher.score_move(m, board, self.tt_move, ply, self.is_qsearch);
                        self.scores[i] = score;
                    }
                    self.index = 0;
                    self.stage = Stage::Quiets;
                }
                Stage::Quiets => {
                    while self.index < self.moves.len() {
                        searcher.pick_move(self.moves.as_mut_slice(), &mut self.scores, self.index);
                        let m = self.moves.get(self.index);
                        self.index += 1;
                        if Some(m) == self.tt_move || Some(m) == self.excluded_move || Some(m) == self.original_killers[0] || Some(m) == self.original_killers[1] || Some(m) == self.original_countermove { continue; }
                        return Some(m);
                    }
                    self.stage = Stage::BadCaptures;
                    self.index = 0;
                }
                Stage::BadCaptures => {
                    if self.index < self.bad_captures.len() {
                        let m = self.bad_captures.get(self.index);
                        self.index += 1;
                        if Some(m) == self.excluded_move { continue; }
                        return Some(m);
                    }
                    return None;
                }
            }
        }
    }
}
