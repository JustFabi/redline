use crate::board::board::Board;
use crate::board::piece::Color;
use crate::board::r#move::{Move, flags};
use crate::movegen::king::get_king_attacks;
use crate::movegen::knight::get_knight_attacks;
use crate::movegen::pawn::get_pawn_attacks;
use crate::magic::{get_bishop_attacks, get_rook_attacks};
use crate::paths::{resolve_syzygy_path, syzygy_path_string};
use pyrrhic_rs::engine_adapter::{EngineAdapter, Piece as TbPiece};
use pyrrhic_rs::tablebases::{
    DtzProbeResult, DtzProbeValue, TableBases, TBError, WdlProbeResult,
};
use pyrrhic_rs::Color as TbColor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

pub const TB_VALUE: i32 = 20000;

#[derive(Clone, Copy)]
pub struct RedlineTbAdapter;

impl EngineAdapter for RedlineTbAdapter {
    fn pawn_attacks(color: TbColor, sq: u64) -> u64 {
        let c = if color == TbColor::Black {
            Color::Black
        } else {
            Color::White
        };
        get_pawn_attacks(sq as u8, c)
    }

    fn knight_attacks(sq: u64) -> u64 {
        get_knight_attacks(sq as u8)
    }

    fn bishop_attacks(sq: u64, occupied: u64) -> u64 {
        get_bishop_attacks(sq as u8, occupied)
    }

    fn rook_attacks(sq: u64, occupied: u64) -> u64 {
        get_rook_attacks(sq as u8, occupied)
    }

    fn queen_attacks(sq: u64, occupied: u64) -> u64 {
        get_bishop_attacks(sq as u8, occupied) | get_rook_attacks(sq as u8, occupied)
    }

    fn king_attacks(sq: u64) -> u64 {
        get_king_attacks(sq as u8)
    }
}

struct TbGlobals {
    bases: TableBases<RedlineTbAdapter>,
    max_pieces: u32,
}

static TB_STATE: OnceLock<Mutex<Option<TbGlobals>>> = OnceLock::new();
static TB_HITS: AtomicU64 = AtomicU64::new(0);

fn tb_mutex() -> &'static Mutex<Option<TbGlobals>> {
    TB_STATE.get_or_init(|| Mutex::new(None))
}

pub fn tb_hits() -> u64 {
    TB_HITS.load(Ordering::Relaxed)
}

pub fn reset_tb_hits() {
    TB_HITS.store(0, Ordering::Relaxed);
}

pub fn init(path_override: Option<&str>) -> bool {
    let Some(dir) = resolve_syzygy_path(path_override) else {
        eprintln!("info string Syzygy: no tablebase directory found");
        return false;
    };

    let path_str = syzygy_path_string(&dir);
    let mut guard = tb_mutex().lock().unwrap();
    if guard.is_some() {
        return true;
    }

    match TableBases::<RedlineTbAdapter>::new(&path_str) {
        Ok(bases) => {
            let max_pieces = bases.max_pieces();
            eprintln!(
                "info string Syzygy: loaded from {} (max {} pieces)",
                path_str, max_pieces
            );
            *guard = Some(TbGlobals { bases, max_pieces });
            true
        }
        Err(TBError::AlreadyInitialized) => true,
        Err(e) => {
            eprintln!("info string Syzygy: init failed ({:?})", e);
            false
        }
    }
}

pub fn is_loaded() -> bool {
    tb_mutex().lock().unwrap().is_some()
}

pub fn configured_path() -> Option<PathBuf> {
    resolve_syzygy_path(None)
}

fn board_tb_args(board: &Board) -> (u64, u64, u64, u64, u64, u64, u64, u64, u32, bool) {
    let w = Color::White.idx();
    let b = Color::Black.idx();
    let ep = board.en_passant_square.map(|s| s as u32).unwrap_or(0);
    let turn = board.side_to_move == Color::White;
    (
        board.occupancy[w],
        board.occupancy[b],
        board.kings[w] | board.kings[b],
        board.queens[w] | board.queens[b],
        board.rooks[w] | board.rooks[b],
        board.bishops[w] | board.bishops[b],
        board.knights[w] | board.knights[b],
        board.pawns[w] | board.pawns[b],
        ep,
        turn,
    )
}

pub fn can_probe(board: &Board) -> bool {
    let guard = tb_mutex().lock().unwrap();
    let Some(tb) = guard.as_ref() else {
        return false;
    };
    if board.castling_rights != 0 {
        return false;
    }
    let pc = crate::board::bitboard::count_bits(board.all_occupancy);
    pc <= tb.max_pieces
}

fn with_bases<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&TableBases<RedlineTbAdapter>) -> Option<R>,
{
    let guard = tb_mutex().lock().unwrap();
    let tb = guard.as_ref()?;
    f(&tb.bases)
}

pub fn probe_wdl(board: &Board) -> Option<WdlProbeResult> {
    if !can_probe(board) {
        return None;
    }
    let (white, black, kings, queens, rooks, bishops, knights, pawns, ep, turn) =
        board_tb_args(board);
    with_bases(|bases| {
        match bases.probe_wdl(
            white, black, kings, queens, rooks, bishops, knights, pawns, ep, turn,
        ) {
            Ok(r) => {
                TB_HITS.fetch_add(1, Ordering::Relaxed);
                Some(r)
            }
            Err(_) => None,
        }
    })
}

fn wdl_rank(wdl: WdlProbeResult) -> i32 {
    match wdl {
        WdlProbeResult::Loss => 0,
        WdlProbeResult::BlessedLoss => 1,
        WdlProbeResult::Draw => 2,
        WdlProbeResult::CursedWin => 3,
        WdlProbeResult::Win => 4,
    }
}

pub fn wdl_to_score(wdl: WdlProbeResult, ply: u32) -> i32 {
    let base = match wdl {
        WdlProbeResult::Loss => -TB_VALUE,
        WdlProbeResult::BlessedLoss => -TB_VALUE + 2,
        WdlProbeResult::Draw => 0,
        WdlProbeResult::CursedWin => TB_VALUE - 2,
        WdlProbeResult::Win => TB_VALUE,
    };
    if base.abs() >= TB_VALUE - 100 {
        if base > 0 {
            base - ply as i32
        } else {
            base + ply as i32
        }
    } else {
        base
    }
}

pub fn probe_wdl_score(board: &Board, ply: u32) -> Option<i32> {
    let wdl = probe_wdl(board)?;
    Some(wdl_to_score(wdl, ply))
}

fn tb_piece_to_promo_flag(p: TbPiece, capture: bool) -> u8 {
    let base = match p {
        TbPiece::Knight => flags::PROMOTE_KNIGHT,
        TbPiece::Bishop => flags::PROMOTE_BISHOP,
        TbPiece::Rook => flags::PROMOTE_ROOK,
        TbPiece::Queen => flags::PROMOTE_QUEEN,
        _ => return flags::QUIET,
    };
    if capture {
        base | flags::CAPTURE
    } else {
        base
    }
}

fn dtz_to_move(board: &Board, from: u8, to: u8, promo: TbPiece, ep: bool) -> Option<Move> {
    let legal = crate::movegen::generate_legal_moves(board);
    for m in legal {
        if m.from() != from || m.to() != to {
            continue;
        }
        if promo == TbPiece::Pawn {
            if ep && m.flags() == flags::EN_PASSANT {
                return Some(m);
            }
            if m.flags() < flags::PROMOTE_KNIGHT {
                return Some(m);
            }
        } else {
            let want = tb_piece_to_promo_flag(promo, (m.flags() & flags::CAPTURE) != 0);
            if m.flags() == want {
                return Some(m);
            }
        }
    }
    None
}

fn pick_best_dtz_move(board: &Board, result: &DtzProbeResult) -> Option<(Move, i32)> {
    match result.root {
        DtzProbeValue::Stalemate => return Some((Move::from_raw(0), 0)),
        DtzProbeValue::Checkmate => {
            return Some((Move::from_raw(0), -TB_VALUE));
        }
        DtzProbeValue::Failed => return None,
        DtzProbeValue::DtzResult(root) => {
            let mut best_move = dtz_to_move(
                board,
                root.from_square,
                root.to_square,
                root.promotion,
                root.ep,
            )?;
            let mut best_rank = wdl_rank(root.wdl);
            let mut best_dtz = root.dtz;

            for i in 0..result.num_moves.min(256) {
                let DtzProbeValue::DtzResult(mv) = result.moves[i] else {
                    continue;
                };
                let Some(candidate) = dtz_to_move(
                    board,
                    mv.from_square,
                    mv.to_square,
                    mv.promotion,
                    mv.ep,
                ) else {
                    continue;
                };
                let rank = wdl_rank(mv.wdl);
                let better = rank > best_rank
                    || (rank == best_rank && rank >= 2 && mv.dtz < best_dtz)
                    || (rank == best_rank && rank < 2 && mv.dtz > best_dtz);
                if better {
                    best_rank = rank;
                    best_dtz = mv.dtz;
                    best_move = candidate;
                }
            }

            let score = wdl_to_score(
                match result.root {
                    DtzProbeValue::DtzResult(r) => r.wdl,
                    _ => WdlProbeResult::Draw,
                },
                0,
            );
            Some((best_move, score))
        }
    }
}

/// Root DTZ probe: pick the best tablebase move among legal moves.
pub fn probe_root(board: &Board) -> Option<(Move, i32)> {
    if !can_probe(board) {
        return None;
    }
    let (white, black, kings, queens, rooks, bishops, knights, pawns, ep, turn) =
        board_tb_args(board);
    let rule50 = board.halfmove_clock as u32;

    let result: DtzProbeResult = with_bases(|bases| {
        match bases.probe_root(
            white,
            black,
            kings,
            queens,
            rooks,
            bishops,
            knights,
            pawns,
            rule50,
            ep,
            turn,
        ) {
            Ok(r) => {
                TB_HITS.fetch_add(1, Ordering::Relaxed);
                Some(r)
            }
            Err(_) => None,
        }
    })?;

    pick_best_dtz_move(board, &result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;

    #[test]
    fn test_kpvk_wdl_win() {
        if !init(Some("syzygy")) {
            eprintln!("Skipping TB test: syzygy/ not loaded");
            return;
        }
        let board = Board::from_fen("6k1/8/8/3P4/4K3/8/8/8 w - - 0 1").unwrap();
        assert!(can_probe(&board));
        let wdl = probe_wdl(&board).expect("WDL probe");
        assert_eq!(wdl, WdlProbeResult::Win);
        let (mv, score) = probe_root(&board).expect("DTZ root");
        assert!(score > 0);
        assert!(mv.from() != mv.to());
    }
}
