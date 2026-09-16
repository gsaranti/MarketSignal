# Current session handoff

## What happened

**Research request rows name their query or URL** (2026-09-15; `45e47b0` on `main`, pushed).
The user asked for the search query or page address on the Portfolio run log's web rows; both were already on the wire in the series id but never rendered.
A typed `RequestTarget { kind, text }` now rides the progress seam's two request events, set only by the research loop's search and fetch emitters; the series id stays the pairing key and is never parsed.
The tracker renders a tracked-caps SEARCH / FETCH token plus the text as the row's third segment, and the ok-row note ("12 hits", "served from document cache") before the status.
A research row is an explicit five-column grid, so the status marker holds the right edge down to the 620px minimum pane and the subject is what clips.
Codex's P2 made the subject a ghost-button disclosure — Enter, Space or click reveals the full text under the row — stripped to the row's metrics after the kit button's 1px border made research rows 2px taller.
Three rulings: a typed field, not a series-id parse; one line as its own column, not a second line; the ok-note shown.
Metis approve-with-nits (applied); the kit README's disclosure note is Codex-authored and kept by ruling.
Verified in an offscreen WKWebView harness at 1180 / 900 / 620px, light and dark — a compiled `swiftc` binary with the sandbox off and read access `/`; the interpreted `swift` script hangs under the sandbox and a page outside the granted directory is refused.
No stamp moved; the dev store was never touched.

## Current state

Working tree clean, `main` in sync with `origin/main` after this session-end commit.
Nothing in flight.
The dev store is still the clean debut wiped 2026-09-14 (`web_source_state` absent until the dev app's next fresh start recreates it).
Attempt 6 writes `job_runs` id 6; its debut stamp set to confirm is **`portfolio-v35` / `checkpoint-v9` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`**.
**Bring-up caution stands:** build the dev app fresh before the first start — a stale binary would recreate the old `web_source_state` shape and lack the call fences, the bounded panes and the row subjects.
Attempt 6 is the after-measurement of the prompt-clarity bundle, the first run under the unit and overlay guards, the live witness for the fences, the first long-run look at the bounded panes, and the first live look at the subject column at book scale; its holding files split on `^==== call`.

## Open questions

- **When to launch attempt 6** — the user's call; don't propose it.
- Does the per-call re-think volume drop under v35 — same segmentation as the v34 baseline over four holdings (synthesis 381 markers / action 40 / interpretation ~30 / gathering 0), now per fenced call.
- How many holdings the ADR guard, the null-`reportedCurrency` rule, and the overlay name screen remove at book scale — `reportedCurrency` is untested live.
- Whether 47 capped panes, and dozens of subject rows per holding, still read long on a full run — collapsing finished holdings was declined; revisit only on the user's word.
- Streaming the research gathering/synthesis turns — deferred; needs tool-call accumulation in the stream decoder and a live re-verification on the pinned Ollama; only after attempt 6.
- Distillation original-source allocation as a throughput/truncation watch; evidence selection is named follow-up work.
- Sampling A/B and non-thinking synthesis — deferred experiments, only if the bundle falls short.
- Carried: the per-domain `denied_count` share at book scale; the permanent SearXNG engine set; action-call oscillation measured on four holdings, not book scale.
- Post-release only: the quick-check state's own parameter stamp has no mismatch consumer — needs a policy before any shipped build.

## Where to start

Nothing is queued ahead of the run.
Wait for the user to name the attempt-6 session; then build the dev app fresh, start it, verify `PRAGMA table_info(web_source_state)` lists `failed_count` / `denied_count`, confirm zero `portfolio_runs` / `portfolio_checkpoints`, bring up infra per the OrbStack bring-up notes, confirm the `portfolio-v35` debut with the v9 / v5 / v4 stamps, read `data-health` early, count the unit and overlay exclusions against the watch set, open the first holding's thought-log file early to confirm the fences, and watch the first holding's research rows show their SEARCH / FETCH subjects and notes in the tracker.
Don't propose the run unprompted.
