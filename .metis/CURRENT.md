# Current session handoff

## What happened

**No code changed — a verification run plus a design decision.** (1) Ran the live `fmp_baseline_smoke` now that the FMP free-tier daily quota had reset (`source ~/.config/market-signal/keys.env && cargo test … fmp_baseline_smoke -- --ignored --nocapture`): **green, all nine groups resolved** with real data — indices (4), internals (3: VIX/gold/silver), sectors (11), index_performance (4), movers (27), earnings (3), sector_pe (22: NASDAQ+NYSE), industries (40), market_risk_premium (1). Exchange labeling and the per-symbol degradation policy both behaved; ~3s, no retry stalls, so not quota-starved. **One data-quality note:** FMP's free `industries` P/E feed carries implausible wire values (e.g. NYSE Oil&Gas Energy pe=461, REIT-Industrial 0.16) — upstream noise passing faithfully through the parse, *not* a bug, but worth caution in how the agent reasons over industry P/E. (2) **Decided the GPT-5-mini inbox extraction stage stays deferred** (reaffirmed, not rejected) — captured in memory (`extraction-stage-deferred`) and, this session, in `.metis/BUILD.md`'s adapters line.

## Current state

HEAD before this session-end was **`970ef46`**, clean. This session-end leaves **uncommitted edits to `.metis/CURRENT.md` and `.metis/BUILD.md` only** (the deferral decision) — no source touched, nothing in flight. The extraction-stage resolution: **defer**; if oversized docs become a problem the first lever is raising `document_parser`'s `PER_DOC_CHAR_CAP` (12k) / `TOTAL_CHAR_BUDGET` (40k) — not a model stage; build the LLM stage only after **truncation telemetry** (a new *accumulating* SQLite table on the `baseline_snapshots` model — one row per truncated doc: `report_id`/`captured_at`/name/format/`original_chars`/`kept_chars`, written best-effort in the persist step beside `record_parse_failures`) shows overflow is common.

## Open questions

- *(new)* Truncation telemetry table not built — it is the prerequisite to ever revisiting the extraction stage with evidence rather than a guess. Small, in-spine, low priority.
- *(new, low)* FMP free `industries`-P/E wire noise — decide whether to clamp/flag implausible P/E values or leave them to the agent's judgment.
- *(carried)* **Per-adapter offline round-trips** behind a base-URL injection seam — the deferred half of the http_retry coverage thread (each adapter's `interpret_response` is already fixture-covered; the live wires remain the only coverage of the adapter→retry→interpret wiring).
- *(carried, low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide; esbuild/vite advisory.

## Where to start

Nothing owed. First, decide whether to **commit the session-end `BUILD.md`/`CURRENT.md` edits**. Then pick a next slice: build the **truncation telemetry table** (small, in-spine — earns the extraction-stage decision); or **per-adapter offline round-trips** (needs the base-URL injection seam); or a carried low-priority item.
