# MoveOverlay-Rust: AI-Powered Chess Analysis

MoveOverlay-Rust is a high-performance tool designed to provide real-time chess move suggestions. By combining computer vision with the Stockfish chess engine, it detects the current state of a board on your screen and overlays suggested moves as arrows directly on top of the board.

## Features

- **Transparent Fullscreen Overlay**: High-quality arrows are rendered on a transparent layer, allowing you to interact with your chess game without interruption.
- **AI-Driven Detection**: Uses a YOLOv8-based vision model via ONNX Runtime, leveraging GPU acceleration (DirectML/CUDA) for near-instant piece detection.
- **Integrated Analysis**: Powered by the Stockfish 17.1 engine, providing depth-based analysis for the top three move variations.
- **Intuitive Selection Tool**: A draggable selection interface allows you to quickly define the chessboard area on any screen.
- **Responsive Interface**: A separate, non-transparent settings window ensures the controls remain interactive even while the main overlay is in "click-through" mode.
- **Global Hotkeys**: Effortlessly toggle between White and Black move suggestions using the 'B' key.

## Installation and Setup

### Download the Complete Bundle
The easiest way to get started is to download the latest **[Release](https://github.com/editzinter/moveOverlay-rust/releases/latest)**. This ZIP file contains the pre-compiled application, the trained AI model (`best.onnx`), and the optimized Stockfish engine.

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
3. **Configure Settings**: Use the settings window to adjust Stockfish depth, the number of suggested lines, and scan frequency.
4. **Start Analysis**: Click the **START** button. The application will begin scanning the board and drawing arrows for the best moves.
5. **Toggle Side**: Press the **B** key at any time to switch between analysis for White and Black pieces.

## Technical Performance

The system is designed to maximize your hardware's potential:
- **Vision Inference**: Offloaded to the **GPU** via DirectML, ensuring the scan does not slow down your system.
- **Engine Calculation**: Stockfish is configured to use 8 CPU threads and 256MB of hash memory for fast, accurate evaluations.

## Safety and Fair Play

**Important Disclaimer:**
This software is developed strictly for **analysis, study, and educational purposes**.

Most online chess platforms (such as Chess.com and Lichess) strictly prohibit the use of external assistance or "engines" during competitive play. Using this tool during ranked or tournament matches constitutes cheating and will likely result in a permanent ban of your account. The developers assume no responsibility for any misuse of this software.

## License
This project is open-source. Please refer to the LICENSE file for more information.
