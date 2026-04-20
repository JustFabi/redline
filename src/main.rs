mod board;
mod movegen;
mod magic;
mod engine;
mod uci;
mod api;

use movegen::knight::init_knight_attacks;
use movegen::king::init_king_attacks;
use movegen::pawn::init_pawn_attacks;
use uci::Uci;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let msg = format!("Panic: {}\nBacktrace:\n{}", info, backtrace);
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(r"C:\xamp\htdocs\git\redline\log.txt") {
            let _ = std::io::Write::write_all(&mut file, msg.as_bytes());
        }
        eprintln!("{}", msg);
    }));

    // Initialize precomputed tables
    init_pawn_attacks();
    init_knight_attacks();
    init_king_attacks();
    magic::init_magics();

    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 && args[1] == "api" {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(api::run_server());
        return;
    }

    if args.len() > 1 && args[1] == "test_logs" {
        let mut uci = Uci::new();
        let contents = std::fs::read_to_string("logs.csv").unwrap();
        for line in contents.lines() {
            if line.starts_with("gui, ") {
                let cmd = line[5..].trim();
                if cmd == "quit" { break; }
                println!("-> {}", cmd);
                let res = uci.process_command(cmd);
                for r in res {
                    println!("<- {}", r);
                }
                if cmd.starts_with("go ") {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    uci.process_command("stop");
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
        return;
    }

    // Default to UCI mode
    let mut uci = Uci::new();
    uci.loop_communication();
}