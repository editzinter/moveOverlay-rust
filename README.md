# MoveOverlay-Rust ♟️🤖

A high-performance, AI-powered chess move overlay built in Rust. It utilizes real-time computer vision to detect board states and integrates the world-class Stockfish engine to provide instant move suggestions directly on your screen.

![Chess Overlay Preview](https://github.com/editzinter/moveOverlay-rust/raw/main/preview.png) *(Note: Placeholder for actual preview image)*

## 🚀 Features

- **Transparent Fullscreen Overlay**: High-quality arrows rendered directly on top of your chess board without interfering with game interaction.
- **AI Vision**: YOLOv8-based piece detection using ONNX Runtime with **GPU acceleration** (DirectML/CUDA).
- **Stockfish Integration**: Optimized CPU-based analysis providing the top 3 best moves.
- **Smart Selection Tool**: Easily define the chessboard region with a simple drag-and-drop tool.
- **Multi-Window Interface**: Interactive settings panel separate from the visual overlay for seamless control.
- **Instant Toggles**: Use global hotkeys to switch between White and Black move suggestions on the fly.

## 🛠️ Installation

### Option 1: Download the Bundle (Recommended)
Grab the latest **[Release](https://github.com/editzinter/moveOverlay-rust/releases/latest)** which includes the pre-compiled executable, the trained vision model (`best.onnx`), and the optimized Stockfish engine.

### Option 2: Build from Source
Ensure you have [Rust](https://rustup.rs/) installed.

1. **Clone the repository:**
   ```bash
   git clone https://github.com/editzinter/moveOverlay-rust.git
   cd moveOverlay-rust
   ```

2. **Prepare Binaries:**
   Place your `best.onnx` and `stockfish.exe` in the project root.

3. **Build and Run:**
   ```bash
   cargo run --release
   ```

## 📖 Usage Instructions

1. **Launch**: Start `redo-man.exe`.
2. **Select Region**: Click **"📐 Select Board Region"** in the Settings window (or press `R`). Drag your mouse over the chessboard on your screen.
3. **Configure**: Adjust Stockfish depth, number of lines, and scan FPS in the settings menu.
4. **Start**: Click **"▶ START"**. Arrows will begin appearing on the board.
5. **Toggle Side**: Press **`B`** to instantly switch between showing White and Black moves.

## ⚡ Hardware Acceleration

The application is optimized for modern hardware:
- **Vision Model**: Runs on your **GPU** (NVIDIA RTX/AMD/Intel) via DirectML for near-zero latency detection.
- **Stockfish**: Runs on your **CPU** using 8 optimized threads and 256MB of hash memory.

## ⚠️ Safety & Ethics

**Important Disclaimer:**
- This tool is intended for **analysis, study, and educational purposes only**.
- **Do not use this tool on competitive platforms** (like Chess.com or Lichess) during ranked matches. Most platforms consider this cheating, and it will result in a permanent ban.
- The creators are not responsible for any misuse of this software.

## 📜 License

This project is open-source. See the [LICENSE](LICENSE) file for details. (AGPL-3.0 for the model, as per Ultralytics).
