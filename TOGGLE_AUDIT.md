# Play mode audit

The six selectable modes are Engine, Human, Book, Aggressive, Gambit, and Endurance. These are mutually exclusive modes saved by Save Settings.

| Mode | Actual behavior |
| --- | --- |
| Engine | Disables UCI strength limiting, sets Skill Level 20, and returns ranked Stockfish principal variations within the configured time and depth limits. |
| Human | Enables UCI strength limiting with a target UCI_Elo of 1800 and uses Stockfish's selected bestmove first. This is weakened Stockfish, not a separately trained human model. |
| Book | Looks up a small built-in opening table, returning legal candidates before truncating to the requested count. Missing positions fall back to full-strength Stockfish. |
| Aggressive | Searches at least four Stockfish candidates and reorders them using bonuses for checks, captures, promotions, and forward moves. This is a tactical heuristic; it does not guarantee sound attacks or a different move on every position. |
| Gambit | Searches 6–12 candidates depending on the time budget and favors immediate material offers only when Stockfish rates them within 50 centipawns of its best candidate. Accounts for captures and recaptures; preserves engine-reported mate priority. |
| Endurance | Searches 4–12 candidates depending on the time budget. It compares exact evaluations from the same completed depth (at least 8), accepts playable near-equal continuations even when a quick win exists, and stays near the best defense when worse. It avoids immediate endings and material offers, favors quiet material-preserving play and pawn locks, and keeps a pawn advance in reserve when possible. If scores are incomplete or shallow, it uses Stockfish's best move. |

## Fixes

- Corrected six capture entries containing invalid UCI notation (`x`), including sole-candidate Scotch and Open Sicilian entries that previously produced no arrow.
- Validate opening moves against the full position before selecting suggestions, with engine fallback when no legal book candidates remain.
- Discard completed search results if the selected mode changed or analysis stopped during the search.
- Discard completed searches when depth, line count, time budget, side, or board region changes. Search-setting changes reuse the settled board and trigger a fresh analysis on the next scan.
- Skip YOLO inference when a previously analyzed board frame is pixel identical; benchmark CUDA against CPU at startup and display the selected provider.
- Corrected the Human target and removed the literal zero-millisecond Book claim in UI/docs.
- Replaced a silently skipped engine test with an explicitly ignored integration test that requires Stockfish when selected.

## Verification

`cargo test --locked -- --include-ignored`: all 51 tests passed with the bundled Stockfish executable and vision model. Coverage includes saved settings, Endurance ranking and exact-score parsing, a live Endurance search, book legality, legal moves across all six modes, repeated searches, overlay geometry, recovery behavior, and a vision-model load/inference smoke test.

Source tracing confirms mode clicks update shared configuration and invalidate the worker's position cache. Full interactive screen-capture/UI operation has not been exercised. The engine and model used for testing are local and git-ignored.
