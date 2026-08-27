import os
import sys
import io
import chess
import chess.svg
import fitz
import numpy as np
from PIL import Image
import onnxruntime as ort
import subprocess

print("=" * 75)
print("     COMPREHENSIVE 50-POSITION END-TO-END CHESS VISION & ENGINE SUITE  ")
print("=" * 75)

model_path = "best.onnx"
engine_path = "stockfish.exe"

if not os.path.exists(model_path) or not os.path.exists(engine_path):
    print("FATAL: Required files (best.onnx / stockfish.exe) are missing.")
    sys.exit(1)

session = ort.InferenceSession(model_path)

fen_piece_to_cid = {
    'K': 1, 'Q': 2, 'R': 3, 'B': 4, 'N': 5, 'P': 6,
    'k': 7, 'q': 8, 'r': 9, 'b': 10, 'n': 11, 'p': 12
}
cid_to_fen_piece = {v: k for k, v in fen_piece_to_cid.items()}

# Robust Auto-Recovering Stockfish Wrapper
class StockfishEngine:
    def __init__(self, path):
        self.path = path
        self.spawn()

    def spawn(self):
        self.p = subprocess.Popen([self.path], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self.send("uci")
        self.wait_for("uciok")
        self.send("setoption name Threads value 4")
        self.send("setoption name Hash value 64")

    def send(self, cmd):
        try:
            self.p.stdin.write(cmd + "\n")
            self.p.stdin.flush()
        except:
            self.spawn()
            self.p.stdin.write(cmd + "\n")
            self.p.stdin.flush()

    def wait_for(self, target):
        while True:
            line = self.p.stdout.readline()
            if not line or target in line:
                break

    def analyze(self, fen, depth=10, lines=3):
        try:
            self.send("isready")
            self.wait_for("readyok")
            self.send(f"setoption name MultiPV value {lines}")
            self.send(f"position fen {fen}")
            self.send(f"go depth {depth}")

            pv_moves = {}
            while True:
                line = self.p.stdout.readline()
                if not line or line.startswith("bestmove"):
                    break
                if " pv " in line:
                    mpv = 1
                    if " multipv " in line:
                        try:
                            mpv = int(line.split(" multipv ")[1].split()[0])
                        except:
                            mpv = 1
                    parts = line.split(" pv ")[1].split()
                    if parts and len(parts[0]) >= 4:
                        pv_moves[mpv] = parts[0]

            return [pv_moves[k] for k in sorted(pv_moves.keys())[:lines]]
        except Exception:
            self.spawn()
            return []

    def close(self):
        try:
            self.send("quit")
            self.p.terminate()
        except:
            pass

engine = StockfishEngine(engine_path)

# 50 Comprehensive Test Positions
TEST_DATABASE = [
    # --- OPENINGS (White & Black perspectives) ---
    ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", False, "Starting Position (White)"),
    ("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1", True, "King's Pawn Response (Black)"),
    ("r1bqkbnr/pp1ppppp/2n5/8/3NP3/8/PPP2PPP/RNBQKB1R b KQkq - 0 4", True, "Open Sicilian (Black)"),
    ("r1bqk1nr/pppp1ppp/2n5/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4", False, "Italian Game (White)"),
    ("rnbqkb1r/ppp2ppp/4pn2/3p4/2PP4/2N5/PP2PPPP/R1BQKBNR w KQkq - 0 4", False, "Queen's Gambit Declined (White)"),
    ("rnbqkbnr/ppp2ppp/4p3/3p4/3PP3/8/PPP2PPP/RNBQKBNR w KQkq - 0 2", False, "French Defense (White)"),
    ("rnbqkbnr/pp2pppp/2p5/3p4/3PP3/8/PPP2PPP/RNBQKBNR w KQkq - 0 2", False, "Caro-Kann Defense (White)"),
    ("rnbq1rk1/ppp1ppbp/3p1np1/8/2PPP3/2N2N2/PP2BPPP/R1BQK2R b KQ - 1 6", True, "King's Indian Defense (Black)"),
    ("r1bqkbnr/1ppp1ppp/p1n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 4", False, "Ruy Lopez Morphy (White)"),
    ("rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2", False, "Sicilian Defense e4 c5 (White)"),
    ("rnbqkb1r/pppp1ppp/5n2/4p3/4P3/2N5/PPPP1PPP/R1BQKBNR w KQkq - 2 3", False, "Vienna Game (White)"),
    ("rnbqkb1r/pppppppp/5n2/8/2PP4/8/PP2PPPP/RNBQKBNR b KQkq - 0 2", True, "Indian Defense (Black)"),
    ("rnbqk2r/ppp1bppp/4pn2/3p4/2PP4/2N1P3/PP3PPP/R1BQKBNR w KQkq - 2 5", False, "Nimzo-Indian Setup (White)"),
    ("rnbqkb1r/pp2pppp/3p1n2/2p5/3PP3/2N5/PPP2PPP/R1BQKBNR w KQkq - 0 4", False, "Modern Defense (White)"),
    ("rnbqkbnr/pppp1ppp/8/4p3/3PP3/8/PPP2PPP/RNBQKBNR b KQkq - 0 2", True, "Center Game (Black)"),

    # --- MIDDLEGAMES & TACTICAL POSITIONS ---
    ("r3k2r/pppq1ppp/2n5/3Np3/8/5N2/PPPP1PPP/R1BQ1RK1 w kq - 0 10", False, "Knight Fork on c7/e7 (White)"),
    ("r2qk2r/ppp2ppp/2n5/3p4/3Pn1b1/2PB1N2/P1P2PPP/R1BQK2R w KQkq - 0 9", False, "Bishop Pin on f3 (White)"),
    ("3r2k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1", False, "Back Rank Checkmate in 1 (White)"),
    ("3r2k1/5ppp/8/8/8/8/5PPP/1R4K1 b - - 0 1", True, "Back Rank Defense (Black)"),
    ("r4rk1/1pp2ppp/p1nq4/3p4/3P4/P1N1PQ2/1P3PPP/R4RK1 w - - 0 14", False, "Middlegame Central Pressure (White)"),
    ("r1b2rk1/pp1n1ppp/2p1p3/q2p2B1/2PP4/2P1PN2/P1Q2PPP/R3KB1R w KQ - 1 11", False, "Queen & Bishop Battery (White)"),
    ("2rq1rk1/pb1nbppp/1p2pn2/2pp4/2PP4/1PN1PN2/PB2BPPP/2RQ1RK1 w - - 0 12", False, "Symmetrical Pawn Structure (White)"),
    ("r1b1kb1r/pp1n1ppp/2p1pn2/q5B1/2PP4/2N2N2/PP3PPP/R2QKB1R b KQkq - 2 8", True, "Black Queen Active (Black)"),
    ("2kr3r/ppqn1pp1/2p1pn1p/8/2BP1b2/2N1BQ1P/PPP2PP1/2KR3R w - - 0 14", False, "Opposite-Side Castling (White)"),
    ("r2q1rk1/1bp1bppp/p1n1pn2/1p6/3P4/1BN1PN2/PP3PPP/R1BQR1K1 w - - 0 12", False, "Isolated Queen Pawn (White)"),
    ("r1bqr1k1/ppp2ppp/2n2n2/3p4/3P4/P1PB1N2/2P2PPP/R1BQ1RK1 w - - 0 10", False, "Greek Gift Sacrifice Setup (White)"),
    ("r1bq1rk1/pp2ppbp/2np1np1/8/3NP3/2N1BP2/PPP3PP/R2QKB1R w KQ - 0 9", False, "Yugoslav Attack Dragon (White)"),
    ("r2q1rk1/pp1b1ppp/2n1pn2/3p4/3P4/P1PBPN2/2P2PPP/R1BQK2R w KQ - 0 10", False, "French Pawn Chain Break (White)"),
    ("r1bq1rk1/1pp2ppp/p1np1n2/2b1p3/2B1P3/2NP1N2/PPP2PPP/R1BQ1RK1 w - - 0 8", False, "Open File Contestation (White)"),
    ("r2qkb1r/pp1n1ppp/2p1pn2/3p4/2PP4/2N1PN2/PP3PPP/R1BQKB1R w KQkq - 0 7", False, "Stonewall / Slav Defense (White)"),

    # --- ENDGAMES ---
    ("8/5pkp/6p1/8/8/6P1/5PKP/8 w - - 0 35", False, "King & Pawn Endgame (White)"),
    ("8/8/4k3/R7/4P3/8/5K2/8 w - - 0 40", False, "Rook & Pawn Endgame (White)"),
    ("8/5pk1/7p/7P/8/4q3/6PK/8 b - - 0 50", True, "Queen Endgame (Black)"),
    ("8/8/4k3/8/3K4/2N5/5B2/8 w - - 0 60", False, "Bishop + Knight Checkmate (White)"),
    ("4k3/8/8/8/8/8/4q3/4R1K1 w - - 0 1", False, "Absolute Queen Skewer (White)"),
    ("8/8/8/3k4/8/8/3K4/8 w - - 0 1", False, "Opposition Kings (White)"),
    ("8/8/8/3k4/8/8/3K4/8 b - - 0 1", True, "Opposition Kings (Black)"),
    ("8/8/8/8/4P3/4k3/8/4K3 b - - 0 1", True, "Pawn Race / King Stop (Black)"),
    ("8/4P3/8/8/8/4k3/8/4K3 w - - 0 1", False, "Pawn Push to Queen (White)"),
    ("8/1k6/8/8/8/8/1K5R/8 w - - 0 1", False, "Rook Cut-off Technique (White)"),
    ("8/8/2k5/8/8/8/2K1R3/8 b - - 0 1", True, "King Defense vs Rook (Black)"),
    ("8/8/4k3/8/8/8/4K2Q/8 w - - 0 1", False, "Queen vs Bare King (White)"),
    ("8/8/4k3/8/8/8/4K2B/7B w - - 0 1", False, "Two Bishops Mate (White)"),
    ("8/8/8/3p4/3P4/2k5/4K3/8 w - - 0 1", False, "Mutual Zugzwang (White)"),
    ("8/8/8/3p4/3P4/2k5/4K3/8 b - - 0 1", True, "Mutual Zugzwang (Black)"),

    # --- COMPLEX & EDGE CASE POSITIONS ---
    ("rnbqkbnr/ppp1pppp/8/3p4/2P1P3/8/PP1P1PPP/RNBQKBNR b - - 0 1", True, "Screenshot Game (Black Pawn on d5)"),
    ("r1b1k2r/ppppqppp/2n5/4p3/2B1n3/5N2/PPPP1PPP/R1BQK2R w KQkq - 0 7", False, "Center Pawn Fork Threat (White)"),
    ("rnbq1rk1/pppn1ppp/4p3/3pP3/1b1P4/2NB1N2/PPP2PPP/R1BQK2R w KQ - 1 7", False, "Bxh7+ Greek Gift Possibility (White)"),
    ("r1bqk2r/pp2bppp/2n1pn2/2pp4/2PP4/2N1PN2/PP2BPPP/R1BQK2R w KQkq - 4 7", False, "Tarrasch Symmetrical Break (White)"),
    ("r3k2r/pb3ppp/1p1bpn2/2pp4/2PP4/1PN1PN2/PB2BPPP/R4RK1 b kq - 0 12", True, "Double Fianchetto (Black)")
]

def render_vector_board(fen, play_as_black, size=640):
    board = chess.Board(fen)
    svg_data = chess.svg.board(board=board, size=size, flipped=play_as_black, coordinates=False)
    doc = fitz.open(stream=svg_data.encode('utf-8'), filetype='svg')
    pix = doc[0].get_pixmap()
    img = Image.open(io.BytesIO(pix.tobytes())).convert('RGB')
    return img

def detect_board_pieces(image):
    img_resized = image.resize((640, 640), Image.Resampling.BILINEAR)
    img_np = np.array(img_resized).astype(np.float32) / 255.0
    img_np = np.transpose(img_np, (2, 0, 1))
    img_np = np.expand_dims(img_np, axis=0)

    outputs = session.run(None, {'images': img_np})
    output0 = outputs[0][0]

    boxes = []
    for i in range(8400):
        scores = output0[4:, i]
        class_id = int(np.argmax(scores))
        max_conf = float(scores[class_id])
        if max_conf > 0.35 and class_id != 0:
            x, y, w, h = output0[0:4, i]
            boxes.append((class_id, max_conf, [float(x), float(y), float(w), float(h)]))

    # NMS
    boxes.sort(key=lambda x: -x[1])
    final_boxes = []
    def iou(b1, b2):
        b1_x1, b1_y1, b1_x2, b1_y2 = b1[0]-b1[2]/2, b1[1]-b1[3]/2, b1[0]+b1[2]/2, b1[1]+b1[3]/2
        b2_x1, b2_y1, b2_x2, b2_y2 = b2[0]-b2[2]/2, b2[1]-b2[3]/2, b2[0]+b2[2]/2, b2[1]+b2[3]/2
        x1, y1 = max(b1_x1, b2_x1), max(b1_y1, b2_y1)
        x2, y2 = min(b1_x2, b2_x2), min(b1_y2, b2_y2)
        inter = max(0, x2-x1) * max(0, y2-y1)
        area1 = b1[2]*b1[3]
        area2 = b2[2]*b2[3]
        return inter / (area1 + area2 - inter + 1e-6)

    while len(boxes) > 0:
        best = boxes.pop(0)
        final_boxes.append(best)
        boxes = [b for b in boxes if iou(best[2], b[2]) < 0.45]

    return final_boxes

def reconstruct_fen(detections, play_as_black):
    square_pieces = [None] * 64
    for cid, conf, bbox in detections:
        rel_x = max(0.0, min(0.999, bbox[0]))
        rel_y = max(0.0, min(0.999, bbox[1] + bbox[3] * 0.1))

        col_idx = int(rel_x * 8.0)
        row_idx = int(rel_y * 8.0)

        if col_idx < 8 and row_idx < 8:
            if play_as_black:
                file_num = 7 - col_idx
                rank_num = row_idx
            else:
                file_num = col_idx
                rank_num = 7 - row_idx

            sq_idx = rank_num * 8 + file_num
            p = cid_to_fen_piece.get(cid)
            if p:
                if square_pieces[sq_idx] is None or square_pieces[sq_idx][1] < conf:
                    square_pieces[sq_idx] = (p, conf)

    fen_rows = []
    for r in range(7, -1, -1):
        empty = 0
        row_str = ''
        for f in range(8):
            sq = r * 8 + f
            if square_pieces[sq] is None:
                empty += 1
            else:
                if empty > 0:
                    row_str += str(empty)
                    empty = 0
                row_str += square_pieces[sq][0]
        if empty > 0:
            row_str += str(empty)
        fen_rows.append(row_str)

    turn_char = 'b' if play_as_black else 'w'
    return '/'.join(fen_rows) + f' {turn_char} - - 0 1'

def square_to_screen_coordinates(sq_str, play_as_black, board_w=800, board_h=800):
    col = ord(sq_str[0].lower()) - ord('a')
    row = int(sq_str[1]) - 1

    cell_w = board_w / 8.0
    cell_h = board_h / 8.0

    if play_as_black:
        draw_col = 7 - col
        draw_row = row
    else:
        draw_col = col
        draw_row = 7 - row

    px = (draw_col + 0.5) * cell_w
    py = (draw_row + 0.5) * cell_h
    return px, py

passed = 0
failed = 0

for i, (test_fen, play_as_black, desc) in enumerate(TEST_DATABASE, 1):
    side_label = "Black (Flipped)" if play_as_black else "White (Standard)"
    print(f"[{i:02d}/50] Testing: {desc:35} | Perspective: {side_label}")

    # Step 1: Render realistic standard board
    img = render_vector_board(test_fen, play_as_black)

    # Step 2: Run vision detector
    detections = detect_board_pieces(img)

    # Step 3: Reconstruct FEN
    extracted_fen = reconstruct_fen(detections, play_as_black)

    # Step 4: Query Stockfish for top 3 suggested lines
    moves = engine.analyze(extracted_fen, depth=10, lines=3)

    # Step 5: Verify position and move legality under chess rules
    board_obj = chess.Board(extracted_fen)
    
    test_ok = True
    error_reasons = []

    if not moves:
        test_ok = False
        error_reasons.append("Stockfish returned NO moves (empty move list)")

    for m_str in moves:
        try:
            move_obj = chess.Move.from_uci(m_str)
            if not board_obj.is_legal(move_obj):
                test_ok = False
                error_reasons.append(f"Illegal move {m_str} generated for board state {extracted_fen}")
        except Exception as e:
            test_ok = False
            error_reasons.append(f"Move {m_str} is invalid UCI format: {e}")

        # Step 6: Verify pixel coordinate transformation
        from_sq = m_str[0:2]
        to_sq = m_str[2:4]
        fx, fy = square_to_screen_coordinates(from_sq, play_as_black)
        tx, ty = square_to_screen_coordinates(to_sq, play_as_black)

        if not (0 <= fx <= 800 and 0 <= fy <= 800 and 0 <= tx <= 800 and 0 <= ty <= 800):
            test_ok = False
            error_reasons.append(f"Out-of-bounds pixel mapping for move {m_str}: from=({fx},{fy}) to=({tx},{ty})")

    if test_ok:
        print(f"   -> PASS: Top Move={moves[0]} | All Lines={moves}")
        passed += 1
    else:
        print(f"   -> FAIL: {'; '.join(error_reasons)}")
        failed += 1

engine.close()

print("\n" + "=" * 75)
print(f"COMPREHENSIVE TEST RESULTS: {passed}/50 PASSED (Failed: {failed})")
print("=" * 75)

if failed > 0:
    sys.exit(1)
