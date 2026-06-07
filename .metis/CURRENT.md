# Current session handoff

## What happened

**Shipped Step 7 ("Gather and Filter News") complete — the news funnel runs end-to-end in code.** Two slices, both pushed to `main`:

- **7a — news ingestion** (commit `5ee5ddb`): a `NewsSource` trait (stub + composite) with **Tavily** (`/search` per topic, fail-loud — gated provider) and **GDELT** (DOC 2.0 ArtList, keyless, fail-soft) adapters, plus the deterministic `dedupe_headlines` pre-pass and a `tavily_key()` accessor.
- **7b — headline filter** (commit `274a36b`): a pure `HeadlineFilter` stage (stub) + the real **GPT-5 mini** `ModelHeadlineFilter` (fixed internal model). The model returns clusters **by headline index, not echoed text**, and `envelope_to_clusters` enforces the funnel invariants *deterministically* (rank by relevance; dedup membership within/across clusters; drop out-of-range indices and blank-topic/summary clusters; cap at **~40 retained headlines across ≤10 clusters**). Added `openai_key()`; promoted `extract_openai_envelope` to `pub(crate)`.

**Load-bearing facts discovered this session:**
- **GDELT rate limit** — 1 req/5s + an *escalating* IP lockout + **User-Agent gating**. Fix: one consolidated OR query per gather (not per-topic) + descriptive UA + fail-soft. Don't reintroduce per-topic GDELT requests. (Memory: [[gdelt-doc-api-rate-limit]].)
- The **~40 retained-headline cap** was a plan-level miss (the plan modeled the ~10-cluster cap but treated the doc's "~40 relevant headlines" stage as model-internal); Codex caught it — now a deterministic backstop.

Reviews: metis-task-reviewer approved both; Codex ran 1 round on 7a + 3 on 7b (High→Low, all resolved). Neither slice is wired into `generate_report` — the clusters' consumer is Step 8.

## Current state

On **`main`** at **`274a36b`**, **pushed**, in sync with `origin/main`, **working tree clean. Nothing in flight.** Step 7 (gather + filter) is complete; both `#[ignore]` live smokes (`news_ingestion_smoke`, `headline_filter_funnel_smoke`) exist but were never run with keys. Verified: `cargo test` (138 lib + integration) + `cargo clippy` clean.

## Open questions

- **Live funnel never run against real providers** — the one remaining real unknown. Needs `TAVILY_API_KEY` + `OPENAI_API_KEY` and a **cool GDELT egress IP** (this session's testing put our IP in an extended lockout). Run the two `#[ignore]` smokes to close it.
- **Snippets omitted from the filter prompt** — `format_headlines` sends title + source only. Revisit against live output to see if snippets sharpen relevance/dedup.
- *(parked)* **retention-cascade enforcement** (30-report cap + cascade, durable-learning survival) and **step-5 auto-archive** — self-contained slices.
- *(deferred, paid source)* calendar `expected` consensus + a FOMC meeting schedule; *(low)* GDP `change_pct` not annualized; `change_pct` reads 0 when two latest readings are equal (candidate to retire).
- *(carried, low)* no Vue component-test harness; data-source floors via pure helpers not an HTTP mock (`wiremock` deferred); `cargo fmt` dirty repo-wide (pre-existing; not the gate).

## Where to start

Step 7 is done — the forward slice is **Step 8: research routing** (`/metis-plan-task`). The fixed **Claude Sonnet** routing model turns the 7b clusters (+ baseline, recent reports, vector memory, inbox, upcoming events) into a bounded research plan — and the clusters are its first consumer, which retires the "unwired" flag. Alternatives: run the live funnel smokes to close the unverified path, or take the parked retention-cascade slice.
