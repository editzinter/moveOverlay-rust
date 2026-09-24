# MoveOverlay-Rust: AI-Powered Chess Analysis

MoveOverlay-Rust is a high-performance tool designed to provide real-time chess move suggestions. By combining computer vision with the Stockfish chess engine, it detects the current state of a board on your screen and overlays suggested moves as arrows directly on top of the board.

## Features

- **6 Play Modes (Chess Assist Style)**:
  - **⚙ Engine Mode**: Superhuman Stockfish 17.1 deep evaluation (3500+ Elo).
  - **🧠 Human Mode**: Natural, realistic human play (target 1800 Elo) matching tournament/club players using Stockfish's UCI Elo limiter.
  - **📖 Book Mode**: Theoretical Grandmaster opening lines with a direct local lookup (automatic engine fallback when out of book).
  - **⚔ Aggressive Mode**: Sharp tactical initiative prioritizing attacking strikes, checks, and captures.
  - **♟ Gambit Mode**: Favors immediate material offers among 6–12 Stockfish candidates only when their evaluation is within 50 centipawns of the best candidate. Short search budgets use fewer candidates so each receives more analysis. Recognizes offers of the moved piece after captures and recaptures. Uses purple arrows.
  - **⌛ Endurance Mode**: Searches 4–12 Stockfish candidates and prefers moves that keep the game going. It avoids immediate endings and material offers, preserves pieces and pawns, favors useful pawn locks, and keeps a pawn advance available when possible. It may give up a winning advantage to reach a playable, more balanced position. Uses teal arrows.


- **Transparent Fullscreen Overlay**: High-quality arrows are rendered on a transparent layer, allowing you to interact with your chess game without interruption.
- **AI-Driven Detection**: Uses a YOLOv8-based vision model via ONNX Runtime. It uses CUDA when available and faster than CPU, and otherwise falls back to CPU. The selected provider is shown in the control panel.
- **Integrated Analysis**: Powered by the Stockfish 17.1 engine with dynamic MultiPV support and time budgeting.
- **Intuitive Selection Tool**: A draggable selection interface allows you to quickly define the chessboard area on any screen (Shortcut: `R`).
- **Responsive Interface**: A floating control panel ensures settings remain interactive even while the overlay is in click-through mode.
- **Anti-Capture Stealth Mode**: Excludes the overlay from screen recording and stream sharing tools (OBS, Discord, Teams).
- **Global Hotkeys**: Effortlessly toggle between White and Black move suggestions using the `B` key, and select region using `R`.

Gambit keeps the configured search time and depth limits. A qualifying sacrifice receives a 51-centipawn ranking bonus, which is enough to prefer it over a move rated up to 50 centipawns better; forced mates retain priority. It does not force a sacrifice on every position or promise compensation: delayed sacrifices and offers outside the engine candidate set may be missed. Evaluations at short search budgets can still miss tactics.

Endurance keeps the configured search time and depth limits. When Stockfish evaluates multiple candidates to the same completed depth (at least depth 8), it considers a move playable at -0.50 pawns or better even if a faster win is available. If already worse, it stays within 0.80 pawns of Stockfish's best defense. It measures the material difference on the board, favors moves that narrow it, avoids immediate game endings when a continuing move exists, and delays a forced mate when all evaluated lines lose. Checkmating moves are excluded from every suggestion; if the engine's shortlist contains only endings, Endurance checks all legal moves for a non-ending alternative. Nonmating checks remain allowed. If every legal move checkmates, it displays no suggestion. This is a heuristic, not a guarantee of the longest possible game: the opponent controls their moves and clock, and board-image detection does not provide reliable repetition or fifty-move history.

## Installation and Setup

### Download the Complete Bundle
The easiest way to get started is to download the latest **[Release](https://github.com/editzinter/moveOverlay-rust/releases/latest)**. This ZIP file contains the pre-compiled application, the trained AI model (`best.onnx`), the Stockfish engine, and the ONNX CUDA provider DLLs. CUDA also requires compatible NVIDIA CUDA and cuDNN runtime libraries on the system; the application uses CPU when they are unavailable or slower.

### Building from Source
If you prefer to build the project yourself, ensure you have the [Rust toolchain](https://rustup.rs/) installed.

1. Clone the repository:
   ```bash
   git clone https://github.com/editzinter/moveOverlay-rust.git
   cd moveOverlay-rust
   ```
2. Place the required binaries (`best.onnx` and `stockfish.exe`) into the project's root directory.
3. Build and run in release mode:
   ```bash
   cargo run --release
   ```

## How to Use

1. **Launch**: Open the application. You will see a transparent overlay and a settings window.
2. **Select the Board**: Click the "Select Board Region" button or press the **R** key. Your screen will dim, allowing you to click and drag a rectangle over the chessboard.
3. **Configure Settings**: Use the settings window to adjust Stockfish depth, the number of suggested lines, and scan frequency. Depth is a ceiling; the search time budget can stop Stockfish before that depth. Maximum Scan FPS is a cap; actual scan speed also depends on vision inference time.
4. **Start Analysis**: Click the **START** button. The application will begin scanning the board and drawing arrows for the best moves.
5. **Toggle Side**: Press the **B** key at any time to switch between analysis for White and Black pieces.

## Technical Performance

The detector benchmarks CUDA and CPU when CUDA initializes, then selects the faster provider. CUDA requires its runtime libraries; CPU inference may use substantial CPU time. Once a valid position is analyzed, identical captured frames skip YOLO inference. The scan rate control limits how often a new scan starts, but cannot make the model run faster when the board changes. Stockfish uses up to 8 CPU threads and 128 MB of hash memory.

Suggestions refresh automatically on an unchanged board. If screen capture, vision inference, or Stockfish fails, the control panel shows a recovery message and the worker attempts to recover automatically. An empty engine response on a playable board is retried rather than cached as a finished analysis. On Windows, the running overlay also periodically restores its topmost position.

## Safety and Fair Play

**Important Disclaimer:**
This software is developed strictly for **analysis, study, and educational purposes**.

Most online chess platforms (such as Chess.com and Lichess) strictly prohibit the use of external assistance or "engines" during competitive play. Using this tool during ranked or tournament matches constitutes cheating and will likely result in a permanent ban of your account. The developers assume no responsibility for any misuse of this software.

## License
This project is open-source. Please refer to the LICENSE file for more information.
