# Play mode audit

The six selectable modes are Engine, Human, Book, Aggressive, Gambit, and Endurance. These are mutually exclusive modes saved by Save Settings.

| Mode | Actual behavior |
| --- | --- |
| Engine | Disables UCI strength limiting, sets Skill Level 20, and returns ranked Stockfish principal variations within the configured time and depth limits. |
| Human | Enables UCI strength limiting with a target UCI_Elo of 1800 and uses Stockfish's selected bestmove first. This is weakened Stockfish, not a separately trained human model. |
| Book | Looks up a small built-in opening table, returning legal candidates before truncating to the requested count. Missing positions fall back to full-strength Stockfish. |
| Aggressive | Searches at least four Stockfish candidates and reorders them using bonuses for checks, captures, promotions, and forward moves. This is a tactical heuristic; it does not guarantee sound attacks or a different move on every position. |
| Gambit | Searches 6–12 candidates depending on the time budget and favors immediate material offers only when Stockfish rates them within 50 centipawns of its best candidate. Accounts for captures and recaptures; preserves engine-reported mate priority. |
| Endurance | Searches 4–12 candidates depending on the time budget. It compares exact evaluations from the same completed depth (at least 8), accepts playable near-equal continuations even when a quick win exists, and stays near the best defense when worse. It excludes checkmates from all arrows, avoids other immediate endings whenever a continuing legal move exists, and searches all legal moves when the engine shortlist contains only endings. It measures material imbalance directly while favoring quiet play and pawn locks. It also penalizes immediate, roughly equal exchanges of knights, bishops, rooks, and queens that the opponent can initiate; pawn exchanges retain their existing lighter treatment. If scores are incomplete or shallow, it filters Stockfish's best move through the same ending rules and falls back to a legal non-ending move if needed. |

## Fixes

- Corrected six capture entries containing invalid UCI notation (`x`), including sole-candidate Scotch and Open Sicilian entries that previously produced no arrow.
- Validate opening moves against the full position before selecting suggestions, with engine fallback when no legal book candidates remain.
- Discard completed search results if the selected mode changed or analysis stopped during the search.
- Discard completed searches when depth, line count, time budget, side, or board region changes. Search-setting changes reuse the settled board and trigger a fresh analysis on the next scan.
- Skip YOLO inference when a previously analyzed board frame is pixel identical; benchmark CUDA against CPU at startup and display the selected provider.
- Corrected the Human target and removed the literal zero-millisecond Book claim in UI/docs.
- Replaced a silently skipped engine test with an explicitly ignored integration test that requires Stockfish when selected.

## Verification

`cargo test --locked -- --include-ignored --test-threads=1` and `cargo test --release --locked -- --include-ignored --test-threads=1`: all 63 tests passed in each profile with the bundled Stockfish executable and vision model. Coverage includes saved settings, Endurance ranking and exact-score parsing, avoidable and necessary exchanges, live Endurance searches at three budgets, checkmate and stalemate exclusion, material balancing, shallow-search fallback, book legality, legal moves across all six modes, repeated searches, overlay geometry, recovery behavior, and a vision-model load/inference smoke test.

Source tracing confirms mode clicks update shared configuration and invalidate the worker's position cache. Full interactive screen-capture/UI operation has not been exercised. The engine and model used for testing are local and git-ignored.
