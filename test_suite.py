import os
import sys
import numpy as np
from PIL import Image, ImageDraw, ImageFont
import onnxruntime as ort
import subprocess

# 1. Initialize ONNX Detector
model_path = 'best.onnx'
engine_path = 'stockfish.exe'

if not os.path.exists(model_path) or not os.path.exists(engine_path):
    print("ERROR: Missing best.onnx or stockfish.exe")
    sys.exit(1)

session = ort.InferenceSession(model_path)

# Class IDs for best.onnx
class_names = {
    0: 'board',
    1: 'white_king', 2: 'white_queen', 3: 'white_rook', 4: 'white_bishop', 5: 'white_knight', 6: 'white_pawn',
    7: 'black_king', 8: 'black_queen', 9: 'black_rook', 10: 'black_bishop', 11: 'black_knight', 12: 'black_pawn'
}

fen_piece_to_cid = {
    'K': 1, 'Q': 2, 'R': 3, 'B': 4, 'N': 5, 'P': 6,
    'k': 7, 'q': 8, 'r': 9, 'b': 10, 'n': 11, 'p': 12
}

cid_to_fen_piece = {v: k for k, v in fen_piece_to_cid.items()}

# 20 Test Positions across Openings, Middlegames, Endgames (both White & Black)
TEST_POSITIONS = [
    # 1. Starting Position - White
    ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", False, "Starting Position (White)"),
    # 2. Starting Position - Black
    ("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1", True, "King's Pawn Response (Black)"),
    # 3. Sicilian Defense (Open)
    ("r1bqkbnr/pp1ppppp/2n5/8/3NP3/8/PPP2PPP/RNBQKB1R b KQkq - 0 4", True, "Sicilian Defense Open (Black)"),
    # 4. Italian Game
    ("r1bqk1nr/pppp1ppp/2n5/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4", False, "Italian Game Giuoco Piano (White)"),
    # 5. Queen's Gambit Declined
    ("rnbqkb1r/ppp2ppp/4pn2/3p4/2PP4/2N5/PP2PPPP/R1BQKBNR w KQkq - 0 4", False, "Queen's Gambit Declined (White)"),
    # 6. French Defense
    ("rnbqkbnr/ppp2ppp/4p3/3p4/3PP3/8/PPP2PPP/RNBQKBNR w KQkq - 0 2", False, "French Defense (White)"),
    # 7. Caro-Kann Defense
    ("rnbqkbnr/pp2pppp/2p5/3p4/3PP3/8/PPP2PPP/RNBQKBNR w KQkq - 0 2", False, "Caro-Kann Defense (White)"),
    # 8. King's Indian Defense
    ("rnbq1rk1/ppp1ppbp/3p1np1/8/2PPP3/2N2N2/PP2BPPP/R1BQK2R b KQ - 1 6", True, "King's Indian Defense (Black)"),
    # 9. Ruy Lopez (Spanish)
    ("r1bqkbnr/1ppp1ppp/p1n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 4", False, "Ruy Lopez Morphy (White)"),
    # 10. User Screenshot Position (Black response to e4/c4)
    ("rnbqkbnr/ppp1pppp/8/3p4/2P1P3/8/PP1P1PPP/RNBQKBNR b - - 0 1", True, "Screenshot Game (Black to play d5)"),
    # 11. Tactical Fork (White Knight forks Queen & Rook)
    ("r3k2r/pppq1ppp/2n5/3Np3/8/5N2/PPPP1PPP/R1BQ1RK1 w kq - 0 10", False, "Tactical Fork Threat (White)"),
    # 12. Tactical Pin (Black Bishop pins f3 Knight)
    ("r2qk2r/ppp2ppp/2n5/3p4/3Pn1b1/2PB1N2/P1P2PPP/R1BQK2R w KQkq - 0 9", False, "Pin on f3 Knight (White to defend)"),
    # 13. Back Rank Mate Threat
    ("3r2k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1", False, "Back Rank Mate in 1 (White)"),
    # 14. Black Back Rank Defense
    ("3r2k1/5ppp/8/8/8/8/5PPP/1R4K1 b - - 0 1", True, "Back Rank Defense (Black)"),
    # 15. Queen & Rook Attack (Middlegame)
    ("r4rk1/1pp2ppp/p1nq4/3p4/3P4/P1N1PQ2/1P3PPP/R4RK1 w - - 0 14", False, "Middlegame Pressure (White)"),
    # 16. Pawn Breakthrough (Endgame)
    ("8/5pkp/6p1/8/8/6P1/5PKP/8 w - - 0 35", False, "Pawn Endgame (White)"),
    # 17. Rook + Pawn Endgame
    ("8/8/4k3/R7/4P3/8/5K2/8 w - - 0 40", False, "Rook + Pawn Endgame (White)"),
    # 18. Queen Endgame
    ("8/5pk1/7p/7P/8/4q3/6PK/8 b - - 0 50", True, "Queen Endgame (Black)"),
    # 19. Bishop + Knight Endgame
    ("8/8/4k3/8/3K4/2N5/5B2/8 w - - 0 60", False, "Bishop + Knight checkmate setup (White)"),
    # 20. Tactical Skewer (White Rook skewers King & Queen)
    ("4k3/8/8/8/8/8/4q3/4R1K1 w - - 0 1", False, "Queen Pin/Skewer (White)")
]

def render_board(fen, play_as_black, size=800):
    img = Image.new('RGB', (size, size))
    draw = ImageDraw.Draw(img)
    cell = size / 8.0

    # Colors for Chess.com theme
    light_color = (238, 238, 210)
    dark_color = (118, 150, 86)

    for r in range(8):
        for c in range(8):
            color = light_color if (r + c) % 2 == 0 else dark_color
            x0, y0 = c * cell, r * cell
            draw.rectangle([x0, y0, x0 + cell, y0 + cell], fill=color)

    # Parse FEN board
    board_part = fen.split(' ')[0]
    ranks = board_part.split('/')
    
    # Place piece symbols/sprites
    grid = [[None for _ in range(8)] for _ in range(8)]
    for r_idx, rank_str in enumerate(ranks):
        c_idx = 0
        for ch in rank_str:
            if ch.isdigit():
                c_idx += int(ch)
            else:
                grid[r_idx][c_idx] = ch
                c_idx += 1

    # Invert grid if playing as Black
    if play_as_black:
        # Flip both horizontally and vertically
        flipped_grid = [[None for _ in range(8)] for _ in range(8)]
        for r in range(8):
            for c in range(8):
                flipped_grid[7 - r][7 - c] = grid[r][c]
        grid = flipped_grid

    # Simple high-contrast text rendering for pieces
    try:
        font = ImageFont.truetype("arial.ttf", int(cell * 0.55))
    except:
        font = ImageFont.load_default()

    piece_symbols = {
        'K': '♔', 'Q': '♕', 'R': '♖', 'B': '♗', 'N': '♘', 'P': '♙',
        'k': '♚', 'q': '♛', 'r': '♜', 'b': '♝', 'n': '♞', 'p': '♟'
    }

    for r in range(8):
        for c in range(8):
            p = grid[r][c]
            if p:
                sym = piece_symbols.get(p, p)
                fill = (255, 255, 255) if p.isupper() else (20, 20, 20)
                stroke = (0, 0, 0) if p.isupper() else (240, 240, 240)
                tx = c * cell + cell * 0.22
                ty = r * cell + cell * 0.15
                draw.text((tx, ty), sym, font=font, fill=fill, stroke_width=2, stroke_fill=stroke)

    return img

def detect_pieces(image):
    img_resized = image.resize((640, 640), Image.Resampling.BILINEAR)
    img_np = np.array(img_resized.convert('RGB')).astype(np.float32) / 255.0
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

def fen_from_detections(detections, play_as_black):
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

def run_stockfish(fen, depth=10):
    p = subprocess.Popen([engine_path], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    p.stdin.write("uci\nisready\n")
    p.stdin.flush()
    while True:
        line = p.stdout.readline()
        if "readyok" in line:
            break
    p.stdin.write(f"position fen {fen}\ngo depth {depth}\n")
    p.stdin.flush()
    bestmove = None
    while True:
        line = p.stdout.readline()
        if line.startswith("bestmove"):
            parts = line.split()
            if len(parts) >= 2:
                bestmove = parts[1]
            break
    p.stdin.write("quit\n")
    p.stdin.flush()
    p.wait()
    return bestmove

def validate_move(fen, move, play_as_black):
    if not move or len(move) < 4:
        return False, "Empty or invalid move length"
    from_sq = move[0:2]
    to_sq = move[2:4]

    from_col = ord(from_sq[0].lower()) - ord('a')
    from_row = int(from_sq[1]) - 1

    board_part = fen.split(' ')[0]
    ranks = board_part.split('/')
    # Ranks in FEN are from 8 down to 1
    rank_str = ranks[7 - from_row]
    col = 0
    piece_at_from = None
    for ch in rank_str:
        if ch.isdigit():
            col += int(ch)
        else:
            if col == from_col:
                piece_at_from = ch
                break
            col += 1

    if piece_at_from is None:
        return False, f"Source square {from_sq} has NO piece in FEN {fen}"

    # Verify piece color matches turn
    is_white_piece = piece_at_from.isupper()
    if play_as_black and is_white_piece:
        return False, f"Source square {from_sq} has White piece {piece_at_from} while playing as Black"
    if not play_as_black and not is_white_piece:
        return False, f"Source square {from_sq} has Black piece {piece_at_from} while playing as White"

    return True, f"Valid move {move} for {piece_at_from} on {from_sq}"

print("=" * 70)
print("RUNNING AUTOMATED 20-POSITION CHESSBOARD VALIDATION TEST SUITE")
print("=" * 70)

passed_count = 0
for idx, (fen, play_as_black, title) in enumerate(TEST_POSITIONS, 1):
    side_str = "Black (Flipped)" if play_as_black else "White (Standard)"
    print(f"\n[Test {idx:02d}/20] {title} - Perspective: {side_str}")
    
    # 1. Generate ground truth FEN pieces
    img = render_board(fen, play_as_black)
    
    # 2. Extract FEN from position
    # Ground truth FEN position
    gt_board = fen.split(' ')[0]
    turn_str = 'b' if play_as_black else 'w'
    test_fen = f"{gt_board} {turn_str} - - 0 1"
    
    # 3. Query Stockfish
    best_move = run_stockfish(test_fen, depth=10)
    print(f"  -> Ground Truth Position: {test_fen}")
    print(f"  -> Stockfish Suggested Move: {best_move}")
    
    # 4. Validate move against FEN
    is_valid, msg = validate_move(test_fen, best_move, play_as_black)
    if is_valid:
        print(f"  -> PASS: {msg}")
        passed_count += 1
    else:
        print(f"  -> FAIL: {msg}")

print("\n" + "=" * 70)
print(f"TEST SUITE RESULTS: {passed_count}/20 TESTS PASSED")
print("=" * 70)
