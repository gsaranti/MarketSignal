# Current session handoff

## What happened

**The two attempt-5 telemetry gaps landed and were pushed** (`94279d0` on `main`; reviewer verdict approve-with-nits, the one stale-comment nit fixed before commit).
Gap 1: the per-domain web-research source state had counted served pages only — the fetch's error path returned before the write — so attempt 5's failed-fetch rate (17 of 25 on PSX, mostly HTTP 401/403) existed nowhere per domain.
`web_source_state` now carries `failed_count` (every failed live attempt past the app's own guard) and `denied_count` (the HTTP 401/403 subset), keyed by the requested host; fetch failures carry a typed `FetchFailure` marker (`Policy` / `Http(status)`), policy refusals never count, and the profile / render-first derivation still reads served pages only.
Data-portability format moved to v6; the pre-release v2–v5 shapes are refused outright, retiring the v4 single-URL import rung.
Gap 2: research-loop retry events name the holding step, the topic, and the leg (`holding-PSX research narrative-sentiment synthesis`) instead of the bare holding step, so attempt 6 reads the Finding-5 parse-retry ↔ unreconciled-topic link per topic rather than as a per-holding co-occurrence.
No prompt, grade, or checkpoint stamp moved.
Docs: `web-research.md §Extraction telemetry` is canonical, with one-clause mirrors in `storage.md` / `data-portability.md`, the retry contract sentence in `local-models.md`, the watch set (wipe precondition, per-topic and per-domain reads), and the attempt-5 record's fix-landed notes on Findings 1 and 5.
Gates: `cargo test` 1438 passed, clippy clean, `npm run build` green.

## Current state

Working tree clean, `main` in sync with `origin/main`.
Nothing in flight.
**The dev store still holds attempt 5's four `portfolio-v34` holdings + checkpoint header**; attempt 6 must re-wipe per the standing ruling, and **the wipe must now also DROP `web_source_state`** — its two new NOT NULL columns sit on a create-if-absent table, so a row-delete wipe leaves the old shape and every telemetry write fails soft, silently, for the whole run.
`PRAGMA table_info(web_source_state)` at bring-up is the check (the watch-set intro carries it); the app recreates the table on start.
The attempt-6 debut stamp to confirm is still **`portfolio-v35`** (`checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v3` unchanged).
The old `web_source_state` counts from attempts 3–5 are lost on the drop — three partial runs' worth, read nowhere.
`BUILD.md` §What remains item 1 and an `INDEX.md` extraction-telemetry row were updated at the user's direction in the follow-up commit.

## Open questions

- **When to launch attempt 6** — the user's call, from a re-wiped store that now also drops `web_source_state`; don't propose it unprompted.
- Does the empty-body rate stay low at book scale, and does the `unreconciled_topics` rate fall under the v35 prompt — now readable per topic against the topic-labelled parse retries (watch line in `big-run-watch-set.md §The research loop`).
- What the per-domain `denied_count` share looks like at book scale — the render-tier / Connected-Sources scheduling evidence the watch set's extraction-telemetry line now reads.
- **Finding 3 oscillation** — still unmeasured; needs the reasoning stream (the thought-log sink, on by default in debug builds) across the book.
- The **permanent** SearXNG engine set — a post-run config call once a full run shows which engines serve under volume.
- Whether the 6d distillation prompts should get the same shape-showing treatment — deferred until attempt 6 reads the unreconciled-topic rate under v35.

## Where to start

Nothing is queued ahead of the run.
Wait for the user to name the attempt-6 session; then re-wipe the store **and drop `web_source_state`**, verify `PRAGMA table_info(web_source_state)` lists `failed_count` / `denied_count`, bring up infra per the OrbStack bring-up notes, confirm the `portfolio-v35` debut, and read `data-health` early.
Don't propose the run unprompted.
