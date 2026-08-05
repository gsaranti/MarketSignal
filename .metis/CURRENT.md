# Current session handoff

## What happened

**Pre-big-run review piece 2 (of 3) COMPLETE** — the code-vs-docs conformance walk of the Portfolio Analysis job.
Seven parallel passes over the full doc surface; **39 verified divergences** (every one re-verified against code before batching), ruled in three batches:
**A — 5 code fixes applied** (`462aaa5`: investor profile removed from the 6f intrinsic prompt — input isolation, test-pinned; the house-view one-week freshness gate built with the typed `DataHealth.house_view_omitted` record; negative-netted-basis dollar gain restored in the sort, exact zero ruled wire-ambiguous; job-named local gate-block messages + the Test Connection pointer; kind-aware tracker labels);
**C — 20 doc corrections applied** (same commit + `c2d963d`), the big one the **as-built/designed marker sweep** over the unbuilt conviction/positioning layer, Step-5 context loads, street opinions, 6a surface, SEC fill-only merge, and the endpoint tables;
**B — 14 design rulings deliberately deferred** to the user, untouched in code and docs.
**Two Codex rounds to approval** — round 1's five doc residuals and round 2's audit-record lead-framing + five stale timing references all confirmed and fixed; Codex endorsed B1–B14 as valid open decisions.
Everything is durably captured: `docs/verification/2026-08-04-piece2-conformance-walk.md` (method, dispositions, the full B list with evidence + options), its INDEX row, and two BUILD stamps (profile-independence now input-isolation-enforced; FINRA/CBOE marked designed legs).
Verified: cargo 902 lib + 32 integration / 0 fail, clippy 0, npm build, 40 node + 206 vitest.
Both commits pushed to origin/main.

## Current state

**No capture debt** — the record, INDEX row, and BUILD stamps landed in-session; this session-end commit carries them.
Queue: **the B rulings (user decision batch), then review piece 3, then the single big confirmation run.**
B3 (the absent listing-resolution guard — wrong-issuer FMP mapping would grade the wrong company, invisible to the run's own checks) is the one B item that could justify a pre-big-run build slice; B5 (vector-continuity lane) and B6 (options method) are the other weighty rulings; most of the rest resolve to doc markers.
Piece 3 = the correctness walk of the deterministic value chain (statement canonicalization → TTM basis → sub-scores → targets/hurdle → feasible set → construction merge → outcome labels), own session per the user's earlier decision.
The big run now additionally live-exercises two of this session's fixes: the profile-free 6f prompt and the house-view freshness gate.

## Open questions

- **The 14 B rulings** — full list with per-item evidence and options in `docs/verification/2026-08-04-piece2-conformance-walk.md §The open B rulings`; decide build vs mark-designed per item (B3 pre-run-relevant).
- **New big-run watches (construction leg)** — lean-divergence / engine-bar / carried-stale-lean rates at 47-position scale; construction-prompt fit in the shared 131k `num_ctx` (settled: compress digests, never `num_ctx`); overlay classification against real Schwab OCC rows; the 7b sizing-only decided-range movement rate. Live 122B construction behavior wholly unexercised.
- **Research-loop activation obligation** — identity + source-text validation + period normalization before the pre-profit producer activates (now also doc-recorded at workflow §Step 6e).
- **Standing** — unchanged carried list: live-run calibration watches (STI-reads-zero, YoY contiguity, outcome-leg watches), no A letters under grade-v2, big-run checklist, reasoning-pane DOM weight, encrypted portability round-trip, step-17 embedding, 600 s stress, scorecard display, dev-store residue, Keychain fail-soft, stage-and-swap import, chain both-maps invariant, four-part verdict bound, §1 open drafts, fraud-producer posture (+ the `hard_forensic_bar` consumer seam, now recorded in the piece-2 record), fund-slice drafted constants, checkpoint/resume + the 6g input-delta validator.

## Where to start

**Rule the B batch first** — open `docs/verification/2026-08-04-piece2-conformance-walk.md §The open B rulings` and reply with per-item dispositions; doc-marker outcomes are quick edits, build outcomes become planned slices (B3's ruling decides whether one slice precedes the big run).
Then run review piece 3 in its own session, then the big confirmation run per the locked plan.
