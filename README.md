# Redline Chess Engine

Redline is a high-performance chess engine written in Rust, featuring bitboard representation, legal move generation, and an advanced search engine with Universal Chess Interface (UCI) support. It leverages modern search techniques like aspiration windows, null move pruning, late move reductions, and Lazy SMP for multi-threading.

## Features

- **Efficient Board Representation**: Uses 64-bit bitboards for all pieces and occupancy, allowing for fast move generation and position updates.
- **Full Legal Move Generation**: Correctly handles all standard chess rules, including:
  - Castling (with all king-safety and rook-path constraints).
  - En passant.
  - Pawn promotion.
  - Check and checkmate detection.
  - Draw conditions (stalemate, insufficient material, 50-move rule).
- **Advanced Search Engine**:
  - **Negamax** with **Alpha-Beta pruning**.
  - **Iterative Deepening** for flexible time management.
  - **Aspiration Windows** for narrower search ranges.
  - **Quiescence Search** to avoid the horizon effect on captures.
  - **Transposition Table (TT)**: Lock-less implementation using atomic operations.
  - **Multi-threading**: Efficient **Lazy SMP** support.
  - **Pruning & Reductions**: **Null Move Pruning (NMP)**, **Late Move Reduction (LMR)**, and **Mate Distance Pruning**.
  - **Move Ordering**: Optimized using **TT-moves**, **MVV-LVA** (Most Valuable Victim - Least Valuable Attacker), and **Killer Moves**.
- **Evaluation**:
  - Material counting.
  - **Piece-Square Tables (PST)** for midgame and endgame positional evaluation.
  - Specialized king safety and activity logic.
- **Universal Chess Interface (UCI)**: Standard protocol support for compatibility with chess GUIs.
- **Interactive CLI**: A command-line interface for manual play, testing, and debugging.
- **FEN Support**: Load and export positions using standard Forsyth-Edwards Notation.

## Technologies Used

- **Rust**: Leverages Rust's memory safety, performance, and modern toolchain.
- **Bitboards**: 64-bit integer masks for high-speed board state management.
- **UCI Protocol**: The industry-standard communication protocol for chess engines.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable version)

### Building

To build the project for optimal performance, use release mode:

```powershell
cargo build --release
```

The executable will be located at `target/release/redline.exe`.

## Usage

### Interactive CLI Mode

Run the engine without any arguments to enter the interactive CLI mode:

```powershell
cargo run
```

Commands in CLI mode:
- `startpos`: Resets the board to the starting position.
- `fen <fen_string>`: Loads a position from a FEN string.
- `move <e2e4>`: Makes a move in coordinate notation.
- `go`: Asks the engine to search and suggest the best move.
- `quit` or `exit`: Closes the engine.

### UCI Mode (for Chess GUIs)

To use Redline with a GUI like Arena, Cutechess, or BanksiaGUI, run it with the `uci` argument:

```powershell
.\target\release\redline.exe uci
```

Once in UCI mode, the engine follows the standard protocol. Supported commands include `uci`, `isready`, `ucinewgame`, `position`, `go`, and `quit`.

### Web API Mode

Redline can also be run as a web server, allowing you to send UCI commands via a REST API:

```powershell
cargo run -- api
```

The server listens on `http://127.0.0.1:3000`. You can interact with it using `curl` or any HTTP client:

**Example: Set position and search**
```bash
# Set position
curl -X POST http://127.0.0.1:3000/uci -H "Content-Type: application/json" -d '{"command": "position startpos moves e2e4"}'

# Get best move
curl -X POST http://127.0.0.1:3000/uci -H "Content-Type: application/json" -d '{"command": "go depth 6"}'
```

### Time Management

The engine dynamically allocates search time based on UCI parameters (`wtime`, `btime`, `winc`, `binc`, `movestogo`), ensuring it plays effectively within specified time controls.

## Testing

Redline includes a comprehensive test suite, including `perft` tests to verify the correctness of the move generator:

```powershell
cargo test
```

## License

This project is for educational and hobby purposes.
