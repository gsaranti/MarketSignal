# Current session handoff

## What happened

**The dev store was wiped to a clean debut at the user's direction** (2026-09-14, no run launched; the attempt-5 telemetry gaps had landed and been recorded in the prior session — `94279d0`, BUILD + INDEX at `861e670`).
With the app not running, one sqlite3 transaction emptied every `portfolio_*` table (attempt 5's checkpoint header, its four holding rows, 17 research seeds), `holdings_pulls`, `price_bars`, and `web_documents` (34 pages from 2026-09-01, all inside the 28-day freshness window, so they would have served as cache hits), reset the portfolio autoincrement counters, and **dropped `web_source_state` whole** — its 23 rows were the old shape without the v6 `failed_count` / `denied_count` columns.
WAL checkpointed and vacuumed; integrity check clean.
Kept: 30 reports, 67 report-namespace vectors (38 learnings + 29 summaries), 14 baseline snapshots, `job_runs` 1–5, all 16 `app_settings` keys (the SearXNG endpoint included), the thought-log directory, and the SEC ticker cache.
The prod store was verified untouched by SHA-256 before and after.
The web-research store tests confirm `init_schema` recreates the table in the v6 shape.
No code, doc, or stamp changed; the memory notes record the wipe as done.

## Current state

Working tree clean, `main` in sync with `origin/main` at `861e670`.
Nothing in flight.
**The dev store is a clean debut** — zero portfolio runs, checkpoints, seeds, episodes, or quick checks, and `web_source_state` is absent until the dev app's next start recreates it.
Attempt 6 will write `job_runs` id 6; its debut stamp to confirm is still **`portfolio-v35`** (`checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v3` unchanged).
**Bring-up caution:** build the dev app fresh before the first start — a stale `target/debug` binary predating `94279d0` would recreate the old table shape, and the current binary's create-if-absent would then be a silent no-op, failing every telemetry write soft for the whole run.
The pre-wipe copy of attempt 5's four holdings lived only in the session scratchpad, so treat them as gone; the attempt-5 record already carries what was read from them.

## Open questions

- **When to launch attempt 6** — the user's call; the store is ready. Don't propose it unprompted.
- Does the empty-body rate stay low at book scale, and does the `unreconciled_topics` rate fall under the v35 prompt — now readable per topic against the topic-labelled parse retries (`big-run-watch-set.md §The research loop`).
- What the per-domain `denied_count` share looks like at book scale — the render-tier / Connected-Sources scheduling evidence the watch set's extraction-telemetry line reads.
- **Finding 3 oscillation** — still unmeasured; needs the reasoning stream (the thought-log sink, on by default in debug builds) across the book.
- The **permanent** SearXNG engine set — a post-run config call once a full run shows which engines serve under volume.
- Whether the 6d distillation prompts should get the same shape-showing treatment — deferred until attempt 6 reads the unreconciled-topic rate under v35.

## Where to start

Nothing is queued ahead of the run.
Wait for the user to name the attempt-6 session; then build the dev app fresh, start it, verify `PRAGMA table_info(web_source_state)` lists `failed_count` / `denied_count`, confirm the store is still a clean debut (zero `portfolio_runs` / `portfolio_checkpoints`), bring up infra per the OrbStack bring-up notes, confirm the `portfolio-v35` debut, and read `data-health` early.
Don't propose the run unprompted.
