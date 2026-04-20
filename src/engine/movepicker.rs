use crate::board::board::Board;
use crate::board::r#move::{Move, flags};
use crate::movegen::{self, GenType};
use crate::movegen::move_list::MoveList;
use crate::engine::search::Searcher;

#[derive(PartialEq, Clone, Copy)]
enum Stage {
    TT,
    GenerateCaptures,
    GoodCaptures,
    Killers,
    GenerateQuiets,
    Quiets,
    BadCaptures,
}

pub struct MovePicker {
    stage: Stage,
    tt_move: Option<Move>,
    moves: MoveList,
    scores: Vec<i32>,
    bad_captures: MoveList,
    index: usize,
    killers: [Option<Move>; 2],
    is_qsearch: bool,
}

impl MovePicker {
    pub fn new(tt_move: Option<Move>, killers: [Option<Move>; 2], is_qsearch: bool) -> Self {
        Self {
            stage: Stage::TT,
            tt_move,
            moves: MoveList::new(),
            scores: Vec::new(),
            bad_captures: MoveList::new(),
            index: 0,
            killers,
            is_qsearch,
        }
    }

    pub fn next(&mut self, searcher: &Searcher, board: &Board, ply: u32) -> Option<Move> {
        loop {
            match self.stage {
                Stage::TT => {
                    self.stage = Stage::GenerateCaptures;
                    if let Some(m) = self.tt_move {
                        // Assuming TT move is pseudo-legal
                        return Some(m);
                    }
                }
                Stage::GenerateCaptures => {
                    self.moves = if board.is_in_check(board.side_to_move) {
                        movegen::generate_evasions(board) // If in check, generate all evasions
                    } else {
                        movegen::generate_captures(board)
                    };
                    
                    self.scores.clear();
                    for i in 0..self.moves.len() {
                        let m = self.moves.get(i);
                        // Score captures using MVV-LVA / SEE
                        let score = searcher.score_move(m, board, self.tt_move, ply, self.is_qsearch);
                        self.scores.push(score);
                    }
                    self.index = 0;
                    self.stage = Stage::GoodCaptures;
                }
                Stage::GoodCaptures => {
                    while self.index < self.moves.len() {
                        searcher.pick_move(self.moves.as_mut_slice(), &mut self.scores, self.index);
                        let m = self.moves.get(self.index);
                        self.index += 1;
                        if Some(m) == self.tt_move { continue; }

                        if board.is_in_check(board.side_to_move) {
                            return Some(m); // Evasions are just yielded in order
                        }

                        let see = board.see(m);
                        if see >= 0 {
                            return Some(m);
                        } else {
                            self.bad_captures.push(m);
                        }
                    }
                    
                    if board.is_in_check(board.side_to_move) || self.is_qsearch {
                        return None; // Done if qsearch or evasions
                    }
                    
                    self.stage = Stage::Killers;
                    self.index = 0;
                }
                Stage::Killers => {
                    // For simplicity, we skip killers stage and just generate quiets, 
                    // then prioritize killers during the Quiets stage. 
                    // This avoids complex pseudo-legality checks for killers.
                    self.stage = Stage::GenerateQuiets;
                }
                Stage::GenerateQuiets => {
                    self.moves = movegen::generate_quiets(board);
                    self.scores.clear();
                    for i in 0..self.moves.len() {
                        let m = self.moves.get(i);
                        let score = searcher.score_move(m, board, self.tt_move, ply, self.is_qsearch);
                        self.scores.push(score);
                    }
                    self.index = 0;
                    self.stage = Stage::Quiets;
                }
                Stage::Quiets => {
                    while self.index < self.moves.len() {
                        searcher.pick_move(self.moves.as_mut_slice(), &mut self.scores, self.index);
                        let m = self.moves.get(self.index);
                        self.index += 1;
                        if Some(m) == self.tt_move { continue; }
                        return Some(m);
                    }
                    self.stage = Stage::BadCaptures;
                    self.index = 0;
                }
                Stage::BadCaptures => {
                    if self.index < self.bad_captures.len() {
                        let m = self.bad_captures.get(self.index);
                        self.index += 1;
                        return Some(m);
                    }
                    return None;
                }
            }
        }
    }
}
