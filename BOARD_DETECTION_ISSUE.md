# Board detection recovery incident

## Report

During a long Chess.com bot game, MoveOverlay displayed “Recovering: Board detection is unstable” and stopped drawing move arrows. Restarting the app and selecting the board again twice did not restore suggestions. The supplied screenshot shows the warning panel, but not the captured chessboard.

## What the code showed

- The warning was emitted after three frames for which piece detections could not be assembled into a board with exactly one white king and one black king. It was not caused by an elapsed-time limit.
- Selecting the same rectangle again did not invalidate the worker's cached source because the rectangle coordinates were unchanged.
- The selection grid was drawn over the board while the detector kept scanning, and bad frames could be counted across otherwise recognizable frames.
- Screen capture silently clipped a board rectangle crossing a display edge, changing piece coordinates without reporting a capture error.
- The old warning did not say which king was missing. The screenshot alone cannot establish whether a popup, board layout change, confidence drop, or another visual obstruction caused the original detection failure.
- Anti-Capture Stealth was off in the available configuration, so the overlay could have appeared in captured board frames. The warning screenshot does not show whether an arrow actually obscured a king.

## Recovery changes

- Pause scanning while the selection grid is visible and reset detection after every completed selection, even when the rectangle is unchanged.
- Require the full selected rectangle on one display; report a capture error instead of analyzing a cropped board.
- Retry weak king detections with a slightly lower confidence threshold while retaining the configured threshold for other pieces. A board still needs two matching frames and a legal chess position before analysis.
- Count invalid frames consecutively and identify missing kings in the recovery message.

## Verification and remaining evidence

All 58 Rust tests passed, including the bundled model and engine smoke tests. The bundled model reconstructed all 50 synthetic test boards exactly at confidence 0.35; 20 endgame samples were also exact at the app's default 0.5. The end-to-end script now checks the reconstructed board against ground truth. These are rendered boards, not a screenshot of the failing Chess.com game. The specific visual trigger cannot be confirmed until a full-board screenshot or captured frame from a failure is available.
