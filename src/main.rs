mod board;
mod movegen;
mod magic;
mod engine;
mod uci;
mod api;

use movegen::knight::init_knight_attacks;
use movegen::king::init_king_attacks;
use uci::Uci;

fn main() {
    // Initialize precomputed tables
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

    // Default to UCI mode
    let mut uci = Uci::new();
    uci.loop_communication();
}