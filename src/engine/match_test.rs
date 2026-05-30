use crate::board::board::Board;
use crate::board::piece::Color;
use crate::paths::{resolve_syzygy_path, syzygy_path_string};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Duration;

const MAX_PLIES: usize = 500;

struct UciEngine {
    name: String,
    child: Child,
    reader: BufReader<ChildStdout>,
}

impl UciEngine {
    fn spawn(exe: &Path, name: &str, options: &[(&str, &str)]) -> std::io::Result<Self> {
        let mut child = Command::new(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdout = child.stdout.take().expect("stdout");
        let mut engine = Self {
            name: name.to_string(),
            child,
            reader: BufReader::new(stdout),
        };

        engine.cmd("uci")?;
        engine.wait_token("uciok", Duration::from_secs(10))?;

        for (opt, val) in options {
            engine.cmd(&format!("setoption name {} value {}", opt, val))?;
        }
        engine.cmd("isready")?;
        engine.wait_token("readyok", Duration::from_secs(30))?;
        Ok(engine)
    }

    fn cmd(&mut self, line: &str) -> std::io::Result<()> {
        let stdin = self.child.stdin.as_mut().expect("stdin");
        writeln!(stdin, "{}", line)?;
        stdin.flush()?;
        Ok(())
    }

    fn wait_token(&mut self, token: &str, timeout: Duration) -> std::io::Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        let mut line_buf = String::new();
        loop {
            if std::time::Instant::now() > deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("{}: timeout waiting for {}", self.name, token),
                ));
            }
            line_buf.clear();
            let n = self.reader.read_line(&mut line_buf)?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("{}: EOF", self.name),
                ));
            }
            if line_buf.contains(token) {
                return Ok(());
            }
        }
    }

    fn go_and_bestmove(&mut self, go_cmd: &str) -> std::io::Result<String> {
        self.cmd(go_cmd)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        let mut line_buf = String::new();
        loop {
            if std::time::Instant::now() > deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("{}: timeout waiting for bestmove", self.name),
                ));
            }
            line_buf.clear();
            self.reader.read_line(&mut line_buf)?;
            if let Some(rest) = line_buf.strip_prefix("bestmove ") {
                let mv = rest.split_whitespace().next().unwrap_or("").trim();
                if mv != "(none)" && !mv.is_empty() {
                    return Ok(mv.to_string());
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{}: no legal move", self.name),
                ));
            }
        }
    }

    fn quit(&mut self) {
        let _ = self.cmd("quit");
        let _ = self.child.wait();
    }
}

fn adjudicate_result(board: &Board) -> f64 {
    let stm = board.side_to_move;
    let legal = crate::movegen::generate_legal_moves(board);
    if !legal.is_empty() {
        return 0.5;
    }
    if board.is_in_check(stm) {
        if stm == Color::White {
            0.0
        } else {
            1.0
        }
    } else {
        0.5
    }
}

/// Run an Elo test: Redline vs Stockfish via UCI.
/// `redline_exe` — path to redline binary; `stockfish_exe` — path to Stockfish.
/// Score is from Redline's perspective (1 = Redline win, 0 = loss, 0.5 = draw).
pub fn run_stockfish_match(
    redline_exe: &Path,
    stockfish_exe: &Path,
    num_games: usize,
    movetime_ms: u64,
    hash_mb: u32,
    syzygy_path: Option<&str>,
) {
    if !redline_exe.is_file() {
        eprintln!("Redline executable not found: {}", redline_exe.display());
        return;
    }
    if !stockfish_exe.is_file() {
        eprintln!("Stockfish executable not found: {}", stockfish_exe.display());
        eprintln!("Set STOCKFISH_PATH or pass path as 4th argument.");
        return;
    }

    let syzygy_opt = syzygy_path
        .map(|s| s.to_string())
        .or_else(|| resolve_syzygy_path(None).map(|p| syzygy_path_string(&p)));

    let mut r_opts: Vec<(&str, String)> = vec![("Hash", hash_mb.to_string())];
    let mut sf_opts: Vec<(&str, String)> = vec![("Hash", hash_mb.to_string())];
    if let Some(ref path) = syzygy_opt {
        r_opts.push(("SyzygyPath", path.clone()));
        sf_opts.push(("SyzygyPath", path.clone()));
    }

    let r_refs: Vec<(&str, &str)> = r_opts.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let sf_refs: Vec<(&str, &str)> = sf_opts.iter().map(|(k, v)| (*k, v.as_str())).collect();

    println!("=== Redline vs Stockfish 11 ===");
    println!("Games: {}", num_games);
    println!("Time: {} ms/move", movetime_ms);
    println!("Redline: {}", redline_exe.display());
    println!("Stockfish: {}", stockfish_exe.display());
    if let Some(ref p) = syzygy_opt {
        println!("Syzygy: {}", p);
    }
    println!();

    let mut redline = match UciEngine::spawn(redline_exe, "Redline", &r_refs) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to start Redline: {}", e);
            return;
        }
    };
    let mut stockfish = match UciEngine::spawn(stockfish_exe, "Stockfish", &sf_refs) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to start Stockfish: {}", e);
            redline.quit();
            return;
        }
    };

    let mut redline_score = 0.0f64;
    let test_start = std::time::Instant::now();

    for game in 0..num_games {
        let redline_white = game % 2 == 0;
        let _ = redline.cmd("ucinewgame");
        let _ = stockfish.cmd("ucinewgame");
        let _ = redline.cmd("isready");
        let _ = redline.wait_token("readyok", Duration::from_secs(10));
        let _ = stockfish.cmd("isready");
        let _ = stockfish.wait_token("readyok", Duration::from_secs(10));

        let mut board = Board::startpos();
        let mut plies = 0usize;
        let go_cmd = format!("go movetime {}", movetime_ms);

        loop {
            let redline_turn =
                (board.side_to_move == Color::White) == redline_white;
            let uci_move = if redline_turn {
                redline.go_and_bestmove(&go_cmd)
            } else {
                stockfish.go_and_bestmove(&go_cmd)
            };

            let uci_move = match uci_move {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Game {}: engine error: {}", game + 1, e);
                    break;
                }
            };

            let Some(m) = board.parse_move(&uci_move) else {
                eprintln!(
                    "Game {}: illegal move {} in position {}",
                    game + 1,
                    uci_move,
                    board.to_fen()
                );
                break;
            };

            board.make_move(m);
            plies += 1;

            if plies >= MAX_PLIES
                || board.halfmove_clock >= 100
                || board.is_repetition()
            {
                break;
            }

            let legal = crate::movegen::generate_legal_moves(&board);
            if legal.is_empty() {
                break;
            }
        }

        let result = adjudicate_result(&board);
        let redline_result = if redline_white {
            result
        } else {
            1.0 - result
        };
        redline_score += redline_result;

        let wdl = if redline_result >= 1.0 {
            "Win"
        } else if redline_result <= 0.0 {
            "Loss"
        } else {
            "Draw"
        };

        let games_done = (game + 1) as f64;
        let win_rate = redline_score / games_done;
        let elo = if win_rate <= 0.0 {
            -800.0
        } else if win_rate >= 1.0 {
            800.0
        } else {
            -400.0 * (1.0 / win_rate - 1.0).log10()
        };

        println!(
            "Game {}/{}: {} ({} plies, fen: {}) | Redline {:.2}/{} | Elo {:+.0}",
            game + 1,
            num_games,
            wdl,
            plies,
            board.to_fen(),
            redline_score,
            games_done,
            elo
        );
    }

    redline.quit();
    stockfish.quit();

    let win_rate = redline_score / num_games as f64;
    let elo = if win_rate <= 0.0 {
        -800.0
    } else if win_rate >= 1.0 {
        800.0
    } else {
        -400.0 * (1.0 / win_rate - 1.0).log10()
    };

    println!();
    println!("=== Final ({} games, {:.1}s) ===", num_games, test_start.elapsed().as_secs_f64());
    println!("Redline score: {:.1} / {}", redline_score, num_games);
    println!("Performance: {:.1}%", win_rate * 100.0);
    println!("Elo vs Stockfish: {:+.0}", elo);
}

pub fn resolve_redline_exe() -> PathBuf {
    if let Ok(p) = std::env::var("REDLINE_EXE") {
        return PathBuf::from(p);
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("redline"))
}
