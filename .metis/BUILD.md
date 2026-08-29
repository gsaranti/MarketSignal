# BUILD — Market Signal

*Architecture brief for the app: the load-bearing decisions and their rationale —
the durable shape future work builds on — not the construction history
(commit-by-commit detail lives in git; per-feature specifics live in `docs/`).
The body is as-built unless marked planned/designed; §What remains lists the
build queue.*

## What it is

Market Signal is a local-first macOS desktop app (Tauri 2 / Rust backend, Vue 3
frontend) that generates a **Market Signal Report on demand** — a professional,
evolving market *thesis* rather than reactive daily commentary. A deterministic
Rust pipeline gathers market data, macro data, and news; a constrained set of
LLM agents reason over a curated packet to produce a Markdown report; the app
renders it to HTML for display and PDF, and keeps long-term continuity through
vector memory. Everything runs on the user's machine except external API and
model calls. The full 18-step control flow is specified in
`docs/report-workflow.md`.

## The load-bearing decision: the app layer orchestrates; agents are pure stages

The boundary the rest of the architecture is most sensitive to is the line
between the deterministic Rust application layer and the agents. The app layer
owns the entire control flow, all I/O, all limits, and all persistence; agents
never touch the network, the database, or the filesystem. Each agent stage is a
pure function — structured input → schema-validated output — behind a Rust trait
(`MainAgent`, `AnalystAgent`, `HeadlineFilter`, `ResearchRouter`), swappable for
a deterministic stub. The model HTTP call is an implementation detail of the
adapter. **The trait methods are synchronous**: the blocking provider call
(`reqwest::blocking`) is offloaded via `spawn_blocking` at the Tauri-command
seam, so `tokio`/async lives only in app-layer I/O, never in an agent. Analyst
concurrency is likewise off `tokio` — the Bull/Bear/Balanced trio runs over the
same packet via scoped OS threads, holding the sync-trait discipline.

Three consequences fall out of this spine:

- **Research planning is the router's job, not the main agent's.** The fixed
  routing model emits the executable plan (Step 8); the app layer executes it
  (Step 9) and assembles the Step-11 condensed packet *deterministically*
  (`research_packet::build_condensed_packet`). By Step 11 the funnel (~500
  headlines → ~10 stories → ~5 routed topics → bounded evidence) has already
  condensed, so packet-building is plumbing, not reasoning — and it keeps faith
  with the pure-stage spine. The main agent gets no live tool loop.
- **Research execution is hard-bounded in the executor, not the model** — ≤50
  requests, ≤30 minutes, dynamic-branching depth ≤2 (`docs/report-workflow.md
  §Step 9`), polled at each request boundary against an injectable `Clock`.
  Dynamic follow-ups ship as deterministic delta-rules (`DeltaBranchPolicy`)
  keyed off the per-report change view, with thresholds time-normalized to the
  run's actual elapsed interval rather than an assumed week.
- **Failure posture splits by stage role.** The research half is *fully
  fail-soft* — a flaky news gather, headline filter, router, or executor call
  degrades to a thinner packet, and the run always reaches the agent with
  whatever landed; only the Step-3 coverage floor gates a run. The analyst layer
  is *deliberately fail-hard* — a failing or blank-summary review fails the run,
  because the analysts are fixed single-pass stages, not the loopable research
  phase. A degraded research run surfaces only through the run tracker's
  per-request rows, not a failed-job warning.

Why it's load-bearing: this boundary decides the module graph, the testing
strategy (agents become offline-stubbable pure functions), the data contracts
(the research packet and each analyst's output schema are the API between
halves), and the safety model (no unbounded agent I/O).

## Data model & storage

Three stores, by responsibility (`docs/storage.md`):

- **Filesystem** — canonical Markdown reports named
  `YYYY-MM-DD-market-signal-report-<id8>.md` (the `report_id` prefix keeps
  same-day reruns distinct; exports drop the suffix), plus the `/research-inbox`
  and `/research-archive` folders.
- **SQLite** — report records, metadata, job history, warning state, per-report
  baseline snapshots, and the vector-memory table. Structured blobs persist as
  serde_json text, and the crate's **`float_roundtrip` feature is load-bearing** —
  round-trips are bit-exact, so carried numerics compare exactly
  (a store test pins the guarantee against a silent dependency edit).
- **Vector memory** — one embedding per report summary and per durable learning
  (`text-embedding-3-large`), each an atomic unit (no chunking). It lives as a
  `vector_memory` table inside the same SQLite database, with exact brute-force
  cosine in Rust — a deliberate engine choice over LanceDB: at this corpus's
  scale (≤30 retained summaries plus low-thousands of learnings) an unindexed
  vector DB runs the same exhaustive scan, while LanceDB would cost a heavy,
  async-only dependency tree against the deliberately synchronous spine. Two
  seams contain the choice: the `vector_memory` module owns all store access,
  and the `embedding::Embedder` trait owns text→vector.

The **report-summary metadata** is a JSON object stored with each report: the
application owns the identity fields (`report_id`, `report_type`, `created_at`)
while the main agent authors the judgment fields — title, stance, header
bullets, and **`risk_posture`** / **`market_cycle`** as two **orthogonal axes**,
not a single regime field (full schema in `docs/storage.md §Report Summary
Metadata Schema`).

The **Step-3 baseline scan** produces an in-memory `BaselineMarketData` packet
(13 groups, indices through CFTC positioning) plus a **`gaps` missing-data
manifest**: partial failures degrade rather than abort — each series an adapter
can't resolve rides into the prompt as a tagged `DataGap`, so the model reasons
over what's absent rather than inferring it. The **single coverage floor** lives
in the app layer (`enforce_coverage`). Each run's baseline persists to
`baseline_snapshots`; the next run computes a deterministic, **cadence-honest
per-report change view** — level deltas anchored on the *actual* elapsed
interval since the prior snapshot, never an assumed week (positioning excluded —
it carries its own native week-over-week change).

*Planned:* the paid FMP key unlocks three additive baseline signals — calendar
consensus+surprise, historical valuation percentile/band + performance trend,
and IPO/M&A froth — all engine-derived and outside the level-delta engine
(`docs/data-sources.md §Planned report enrichment`). True index breadth was
ruled out (FMP exposes no breadth metric), so the movers group stays the proxy.

**Retention** is deliberately asymmetric and must be honored in deletion code:
only the most recent **30 reports** are kept (deleting one cascades its Markdown,
metadata, and vector *summary* row together — there is no HTML leg, since HTML is
rendered on demand and never persisted), **but durable learnings survive report
deletion**, guaranteed by the row's `kind` rather than its `report_id`. Baseline
snapshots keep their own cap (14), decoupled from report retention.

The on-disk home for all stores is resolved from the Tauri app-data dir keyed by
the **bundle identifier**, so it is stable across versions; debug builds nest
under a `dev/` subdir, `MARKET_SIGNAL_DATA_DIR` overrides both. The **macOS
Keychain rail sits outside this split** — app-scoped, not data-dir-scoped, so
debug and release builds read the same Schwab entries; its ACL re-prompts on
ad-hoc rebuilds can block first paint, and a denied read errors the whole
local-config report (fail-softing that to not-connected is a named, unbuilt
candidate). One deliberate exception to *persisted config lives in SQLite*:
the Light/Dark appearance preference lives in webview `localStorage` — pure
presentation, read pre-mount to avoid a first-paint flash.

**Data portability (built — `portability.rs`).** A whole-corpus
backup/restore — distinct from per-report export — carrying the accumulated
analytical history as one structured, versioned, checksummed archive
(`docs/data-portability.md` owns the format, encryption, and flow details).
The load-bearing lines: **durable analytical data moves; secrets and
machine-local operational state stay behind** (the archive cannot leak a
credential); it is deliberately **not** a raw DB-file copy; and import
**validates everything before its destructive phase**. Both directions hold
the single run slot. Accepted residue: a mid-import I/O failure can leave
partial *files* — stage-and-swap is a named, unscheduled hardening.

## Module boundaries

- **`app` (Rust orchestrator)** — the pipeline, the bounded research executor,
  validation/gating, warning-state management, baseline-snapshot persistence and
  the deterministic per-report delta computation, and the `progress`
  run-observability + cancellation seam. This is where determinism lives.
- **`adapters` (Rust)** — `data_sources` (FMP/FRED/BLS/CFTC REST via `reqwest`;
  Tavily + GDELT + FMP Articles for news) and `models` (OpenAI + Anthropic
  HTTP); the full series catalog is in `docs/data-sources.md`. Provider tiering
  is live-verified and load-bearing: FMP's free tier gates the dollar index,
  oil, gas, and the economic calendar behind premium, so those moved to FRED,
  and the calendar carries **names + dates only** today (no API serves US analyst
  consensus *free*, so consensus reaches the report through the agents' research
  synthesis). Data honesty is a consistent stance: a stale FRED observation or
  an out-of-band FMP P/E aggregate **drops to a gap / `None`** rather than
  feeding a fabricated level into the baseline. **CFTC** (keyless, like BLS)
  adds Commitments-of-Traders positioning — the one signal the price /
  valuation / macro / credit groups can't give (how crowded the speculative
  cohort is) — as a fail-soft, additive group. Gated adapters share a bounded,
  `Retry-After`-aware retry/backoff, parameterized per provider — FMP rides a
  minute-crossing 429 ladder, every other adapter keeps the short default,
  and GDELT stays single-shot fail-soft (its IP lockout makes retrying
  harmful); the schedules live in `docs/data-sources.md` (intro retry
  paragraph). **Fixed
  internal models** are non-configurable and distinct from the four
  user-selectable agent models: GPT-5 mini (headline filtering), Claude Sonnet
  (research routing), `text-embedding-3-large` (embeddings). Inbox document
  parsing runs **no model** — it is deterministic excerpting, so a model summary
  can't omit or fabricate over the user's own source material.
- **`agents` (prompt + schema contracts)** — the main agent and the
  Bull/Bear/Balanced analysts (run concurrently, no ordering dependency), plus a
  **16-lens analytical skills library** supplied in full to both. Skills are
  **forcing-function-only**: each lens's verdict disciplines the report/review
  prose but is never parsed back or persisted (a rare keep-worthy verdict exits
  via a `durable_learning`). Analyst reviews are ephemeral — never persisted.
  The main agent's editorial posture is **conviction-first**: the thesis commits
  to a directional base case and weights the alternatives around it, so the
  report reads as a *call* rather than a summary of the packet. A `mixed` /
  `uncertain` `thesis_stance` is the earned exception, not a safe default; the
  base case carries forward across reports and pivots only when the evidence has
  materially changed (`docs/thesis-continuity.md`) — the conviction and the
  rare-pivot doctrine are the same stance, not opposites.
- **`frontend` (Vue 3)** — Latest Report View, the **Run Tracker**, the
  **shared-history sidebar**, Research Documents, the Persistent Warning
  Area, Settings, and the **Portfolio page** (`docs/interface.md`).
  Markdown→HTML rendering uses **markdown-it** on the webview side, on
  demand for display and PDF export, **never persisted** — agents never see
  HTML; PDF export is the webview's native print-to-PDF (the `@page`-margin
  constraint is canonical at `docs/export.md §PDF Export`). Embedded charts
  enter a report exactly one way: a fenced `chart` JSON block validated and
  rendered by `src/renderChart.ts`, falling back to the raw code block on
  anything malformed. All UI is built against
  `market-signal-design-system/`, which defines **two registers** — the
  report's reading register (serif, monochrome, unchanged) and the denser
  instrument-grade analytical register the local-suite surfaces adopt —
  bridged by shared chrome; all suite sorting/view controls are
  **display-only**, reordering already-computed fields (specifics in the
  design package and `docs/interface.md`).

## Runtime, observability & failure posture

Report generation is **on demand only** — there is no scheduler, timer, or tray.
The app is an ordinary windowed app; closing it quits it and nothing runs in the
background, so a report is never "due" while unattended. A run ends in one of
**four** terminal states (`docs/scheduling.md`): **successful**; **failed**
(unreachable provider, a too-thin baseline, or a stuck/failing model call —
recorded with a failed-job warning); **skipped** (a second concurrent run —
single workflow at a time); or **cancelled** (user-stopped from the run tracker
— no report and no warning, since it was intentional). Network reachability is
**not** a pre-run gate: an unreachable provider fails the run rather than
blocking it, since the user is present to see and retry.

The **execution gate** blocks any run until all four agent models are
configured, **both** OpenAI and Anthropic tokens exist (the fixed internal
stages span both providers), and the Tavily/FMP/FRED credentials are present.
Failures surface in the **Persistent Warning Area**, which has four
de-duplicating categories — missing agent configuration, missing API tokens,
missing provider credentials, failed jobs. Only the non-blocking failed-jobs
category is dismissible, and a dismiss targets the **rendered** failure identity
(echoed back to the command), not a click-time re-derived "current" one, so a
stale click can't silently hide a newer, unseen failure.

Run observability rides a **Tauri-free `progress` seam** so the deterministic
spine stays unit-testable: a `ProgressReporter` trait plus a per-run
`RunContext` threaded via `with_context` builders — **no trait signature
changes** for the seam. While a job runs the app streams per-step progress,
one **request row per actual HTTP call**, the main agent's report
token-by-token, and the models' extended-thinking reasoning (thoughts-only —
a review body or structured verdict never streams); the streamed tokens are a
side-channel that can't corrupt the report. A **debug-gated thought-log
sink** is the deliberate, bounded exception to reasoning staying ephemeral —
opt-in, best-effort, outside every store retention rule
(`docs/run-tracking.md §Thought-log capture`). The frontend renders this as
the run tracker on the **running job's own page** (latest-run-only).
Cancellation is cooperative — a shared flag polled at step/request boundaries
and mid-stream, never interrupting an in-flight request. Two load-bearing UI
invariants: a **run is never a report** (a cancel/fail removes nothing that
was shown — exception-free since the fresh-start slice removed the
degraded-run legacy;
`docs/verification/2026-08-17-fresh-start-legacy-removal.md`), and the
terminal `run-finished` event is emitted **before** any job-history write
error can propagate, so a DB failure can't strand the UI mid-run. The
read-time posture: a corrupt run is a loud skip — its own surface only, never
the history listing or the next run's baseline.
The full runtime contract is in `docs/run-tracking.md`.

## Testing approach

The spine makes the pipeline testable offline: agents and adapters are traits,
so the orchestrator runs end-to-end against deterministic stubs and fixture
packets with no live keys, coverage spanning every limit, retention, gating,
and failure-posture contract. The `progress` seam stays out of other tests via
a no-op `RunContext`; its own streaming logic is fixture-tested. Each gated
adapter has a test-only base-URL seam so a localhost mock exercises the full
URL-build → retry → parse → output path offline; live smokes are `#[ignore]`d.
The **frontend gate is two runners under `npm test`**: pure modules on Node's
runner and Vue SFC tests on Vitest mounting real components (the split is
specified in `CLAUDE.md`).

The same trait spine powers a **dev-only demo-run mode** (`src-tauri/src/demo.rs`,
behind a `demo-run` Cargo feature, out of `default`/`tauri build`): "Generate now"
drives the *real* `run_job` pipeline through the live GUI against paced streaming
stand-ins — run tracker and report rendering end-to-end with no network, keys, or
cost (`npm run tauri:demo`).

## Local analysis suite

A second capability set: two on-demand, **local-model-only**, deliberately
**prescriptive** features (grades, actions, targets — a departure from the
report's no-buy/sell stance). **Portfolio Analysis** grades the user's Schwab
holdings and recommends actions + price targets, typing a role/risk read where a
vehicle class is structurally unpriceable. **Trade Opportunities** (designed, not
built) researches new ideas across a 3×3 risk×horizon matrix. A future
**portfolio planner** — the whole-book reasoning the tunnel-vision ruling moved
out of Portfolio Analysis, reading its report beside the market report and
Trade Opportunities — is the suite's named fourth job, not yet designed. Full
design lives in
`docs/local-models.md`, `web-research.md`, `schwab-integration.md`,
`portfolio-analysis.md`, `portfolio-workflow.md` and `trade-opportunities.md`;
this section carries only the decisions a plan must not work against.

- **A local-only model layer, distinct from the cloud report (built).** A
  flexible local-model adapter calls one **user-installed, app-supervised**
  Ollama daemon over its native `/api/chat`, through the same
  `reqwest::blocking` / `spawn_blocking` seam the cloud agents use — **added
  rather than extending the closed cloud `AgentModel` enum**, so the roster
  changes through configuration rather than code. The app **bundles neither the
  daemon nor the models**; it makes setup turnkey *around* a user-installed
  Ollama. The suite gate holds the report's **presence-not-connectivity**
  posture: *presence* of config gates **proactively** (locked Run buttons + a
  persistent warning) while *connectivity* is checked only at the **run-gate**
  and on a manual Test Connection, **never at startup** — a
  config-set-but-daemon-down state is blind on re-open, the deliberate cost of
  no startup probe. A `LocalEmbedder` reuses the existing `Embedder` trait so
  `vector_memory` is unchanged. The roster default is settled: one frontier
  reasoner plus the embedder stay resident, that reasoner filling *every*
  reasoning role by thinking mode. Model identities, the serving path and the
  Ollama pin live in `docs/local-model-operations.md` — a version bump is a
  re-verification event, not a routine upgrade.
- **Per-job isolation (learnings only).** Each feature stores its own runs
  (last-N retention; run identity is **insertion order**, `id`-primary in every
  store query, so a stepped wall clock can never demote the just-persisted run
  from `latest` or shift the diff baseline — `created_at` is display data) and
  its own vector-memory partition; no job reads another's *learnings*. The
  Market Signal Report stays a read-only shared input, loaded deterministically
  (not vector-searched), additionally isolated by embedder dimensionality.
- **A cost-free web tool.** Self-hosted, keyless SearXNG for search plus a Rust
  fetch/readability-extract layer, Tavily as fallback; the orchestrator runs the
  tool, the model only requests it — holding the pure-stage boundary. SearXNG
  isn't bundled: the app *ships configuration, not the server*. Thin extraction
  trips a **selective rendered-retrieval tier reusing the already-embedded Tauri
  webview** — not a bundled browser or Python sidecar — gated on telemetry so
  rendering stays **measured, never blanket** (the tier is **deferred to its
  own slice** — ruled 2026-08-23 with the research loop; its gating telemetry
  landed). SearXNG sits **off the execution
  gate**: unreachable means a degraded run behind a pre-run notice, never a
  block. The per-item research loop is bounded and SSRF-guarded, every finding
  keeping its source URL + timestamp, and consolidation is one shared
  **distillation primitive** whose mode is chosen deterministically by the
  full consolidation input's size. Optional **Connected Sources** (in-app login → Keychain
  session, on the Schwab credential rails) enrich fetching and are **never part
  of the execution gate** (likewise deferred to their own slice).
- **Holdings & options ingestion (built).** Schwab Trader API via an OAuth
  loopback (a weekly re-login cadence), supplying holdings *and* live option
  chains, from which a deterministic put/call + IV/skew signal is computed —
  an activity proxy, not positioning truth, kept out of grade sub-scores
  until calibrated. **A connected Schwab account is required to run either
  local job**; manual CSV/paste import (designed, not built) only supplements
  holdings. Same-symbol rows across granted accounts **net at snapshot
  assembly** into one signed book-level position per symbol
  (`docs/schwab-integration.md §What is pulled`), and a net-short book-level
  equity takes the not-rated treatment. The surface is **read-only by
  construction** — the adapter implements only holdings / positions /
  option-chain `GET`s, a code-enforced guarantee rather than a token scope
  (the Trader API has no read-only scope), bounding a leaked credential's
  blast radius to in-account trades the app never issues. Tokens ride the
  Keychain and **never enter logs or the run tracker**; the loopback's
  one-shot rustls acceptor rides the stack outbound HTTP already uses.
- **Reuses the spine.** Each feature is a new Tauri command + job under the
  **single global run slot** (report + both local jobs are mutually exclusive,
  matching the latest-run-only tracker), reusing the `progress`/run-tracker seam
  and the `vector_memory` / `Embedder` modules. The slot is claimed **before
  any external fetch** — the SEC CIK map loads lazily inside it — so the
  local-only daemon probe is the one pre-slot check (2026-08-18; the eager
  load had also bailed on a stale cancel flag). Local-gate failures get their
  own warning categories, kept **off the cloud `validate` gate** — a
  disconnected account blocks only the local jobs, never the report. Both jobs
  are personalized by a **fixed default investor-profile preset** (user config
  deferred) that frames the prescription, never which holdings or ideas qualify
  — nor the intrinsic verdict, whose profile-independence is declared **and
  input-isolation-enforced**: the intrinsic prompt carries no profile, which
  reaches the model at the per-holding action call only.
- **Invariants governing the suite** (full specs in the docs; a plan must not
  work against these; each states its own reach):
  - **Deterministic finance, primary-source evidence** — a shared Rust engine
    over FMP + keyless SEC EDGAR / FINRA / CBOE (the FINRA short-interest
    leg and the CBOE venue-level backdrop are both built — CBOE serves
    venue-level data only, the per-stock options read being Schwab chains;
    Stooq removed 2026-08-12 — FMP dated-EOD is the only price rung)
    computes the engine arm for both jobs. Both are **two-arm**: the
    engine's values are the incorruptible baseline beside an **unrestricted
    model arm**, structurally validated only, the two scored head-to-head by a
    deterministic scoreboard. The per-job field schemas differ — BUILD does not
    restate them; they are enumerated once at `docs/local-models.md`. The
    boundary — **model-arm judgment values never alter or bind the engine
    baseline** — is single-homed per job at `docs/portfolio-analysis.md §The
    holding verdict` and `docs/trade-opportunities.md §The opportunity`. Both
    jobs hold an **evidence floor** that returns insufficient evidence over a
    low-conviction guess, but each specifies its own —
    `docs/portfolio-analysis.md §Evidence floor` and
    `docs/trade-opportunities.md §Evidence floor` — and their exit semantics
    differ, so read the one for the job in hand.
  - **Anti-reflexivity / no-double-count** — conviction is the model's own, so
    the cap-only since-flagged stance is prompt-side discipline, and the guard
    binds only where it has deterministic consumers: the confirmed-crossing
    validation over each job's own stored conditions, and the cheap
    re-derivation's tripwires. Which conditions each job stores is specified in
    that job's doc, not here. Trade Opportunities adds one rule of its own —
    re-entry is a fresh start, and the archive never promotes itself.
  - **Source quality informs conviction, never gates discovery** — tiers grade;
    only the explicit deny list drops.
  - **Only a deep re-evaluation can archive an opportunity; the cheap
    re-derivation never does** — it refreshes the quant read and raises a
    non-destructive attention warning. This one is Trade Opportunities' framing
    invariant. Portfolio holds the analog rather than the rule: its quick check
    borrows the same warn-don't-decide split, having no archive to write to.

### Seams a plan builds on

The suite's Rust modules sit under `src-tauri/src/portfolio/` — `pipeline` (the
job spine), `dossier` (per-symbol evidence assembly), `engine` (all
deterministic finance), plus `fund`, `listing`, `quick_check`,
`pre_profit`, `outcome`, `store` and `diff` — beside `local_model.rs`,
`market_clock.rs` and `loopback_https.rs` at the crate root.

Shared entry points exist so a slice computes nothing twice. Reach for these
rather than re-deriving:

- `engine::compute_metrics`, `engine::resolve_series`,
  `engine::evaluate_ledger_conditions[_gated]` and
  `engine::reanchor_scenarios` — every metric, series resolution, ledger
  evaluation and closed-form re-anchor.
- `engine::canonicalize_statements`, applied in place at the single
  `dossier::apply_ttm_statement_basis` choke point, so every statement-consuming
  read sees the same sorted, period-deduped vectors and a served-twice
  restatement resolves to the latest filing rather than wire order.
- `market_clock::et_session_date` / `et_date_of` — the ET dating seam. The
  frontend mirror `src/etDate.ts` is a separate implementation (a hand-rolled
  RFC3339 regex plus calendar validation against Chrono's strict parser),
  **behaviorally equivalent on the pinned contract** rather than a port; its two
  known divergences are unreachable. Change one side and re-pin both case
  tables.
- `quick_check::sweep_tail` — the subset-capable sweep, built so later slices
  reuse the quick-check core instead of forking it.
- `pre_profit::clamp_conviction` — the overlay's engine-arm ceiling.
- `engine::implied_expectations` and `engine::narrative_vs_reality` — the
  conviction-layer reads over the one shared `scenario_multiples` derivation
  and the stored prior-run comparator; Trade Opportunities' Step-5c forms
  reuse them when built, never a second implementation.
- `store::load_episodes` / `prune_matured_episodes` — episode identity and
  lifecycle.

BUILD cites version constants rather than duplicating their current values,
so this brief cannot go stale as they move:
`portfolio::PROMPT_VERSION`, `engine::GRADE_PARAMETER_VERSION`,
`engine::SCENARIO_TARGET_PARAMETER_VERSION`,
`pre_profit::PRE_PROFIT_PARAMETER_VERSION`, `engine::EVIDENCE_FLOOR_VERSION`
and `portability::FORMAT_VERSION`.
Persisted records carry the stamp they were written under, so a recalibration
stays attributable and old rows never silently re-grade.

### Standing constraints

Each is easy to break by accident, so a plan should say how it honors them:

- **Every session-keyed date reads the ET session** through `market_clock`.
  Fetch-range *upper bounds* deliberately stay UTC, annotated where they occur.
- **Identity-or-lifecycle selections read insertion order**, never a wall clock
  — `id`-primary store queries. This is deliberately narrower than "every
  selection": everything reading reports *as dated documents* stays
  `created_at`-ordered, so retention and display differ on *which* records,
  never on how many.
- **`portfolio_quick_checks` is deliberately never `portfolio_runs`** — a sweep
  must not contaminate run history, `latest_run`, or the diff baseline.
- **One `num_ctx` per model.** Changing it reloads the resident Ollama runner
  despite `keep_alive`, so context pressure is answered by compressing digests,
  never by raising `num_ctx`.
- **Pre-profit observation rows enter only through the research loop's
  fetched-page lineage.** The producer is active — the loop discharged both
  recorded obligations (holding-identity + source-text validation, ISO period
  normalization) — and an unevidenced call rejects every candidate.
- **Model-arm values never bind the engine baseline.** The typed validated
  channels are the only model→engine inputs: the ledger's conditions, the
  pre-profit observations, and a distilled claim's `related_condition_id`
  tie — the source-backed leg a qualitative ledger trip needs, carried only
  across a verbatim re-emission so a prior tie never becomes fresh support —
  all live. The 2026-08-24 rulings bound the rest —
  the research forward assumption is **shadow-only** (a would-have audit line,
  never a write-back; `engine_output` is immutable past Step 6b), the
  research-fed fraud claim is **advisory** (the hard-forensic state reads the
  item-classified filing kinds alone), and a leading indicator suppresses the
  narrative cap only through an app-verified ledger `driver_id`
  (`docs/verification/2026-08-24-research-loop-rulings.md`).
- **The Schwab adapter implements no order or trading endpoint.** The read-only
  boundary is code-enforced, not scope-enforced, so it survives a token scope
  change.
- **Local-model transport deadlines derive from the request's reservations.**
  Every chat call's deadline is `num_ctx` over a prefill floor plus, on the
  non-streaming path, `num_predict` over a decode floor (`DeadlinePolicy`),
  never a fixed backstop — a fixed ten minutes cut thinking chains at a third
  of their reservation and failed the run. The floors are drafted from the
  pinned serving path's measured throughput and re-verify with it
  (`docs/local-models.md §The local-model adapter seam`).
- **Local-model failures retry once, whitelisted, never nested.** The hard
  model-call posture is hard-after-one-bounded-retry: a typed transient class
  re-attempts exactly once per issued call, while deadline trips, length
  stops, cancellation, and anything unclassified fail on first occurrence,
  and each call path wraps exactly one retry layer. Fired retries are
  evidence — a tracker row plus data-health `model_retries` events — never
  silent. A resumed run's read spans both processes: each holding's
  telemetry rides its checkpoint row, so the trail's telemetry membership is
  its row membership (ruled 2026-08-28). The contract is canonical at
  `docs/local-models.md §The local-model adapter seam`.
- **No distillation prompt issues over its model's budget.** The adapter seam
  sizes every 6d call's rendered prompt before a request exists — within the
  fast tier's budget it issues there, over it on the resident reasoner (a
  model choice, never a `num_ctx` change), over the widest budget it is
  refused as an unclassified failure — and the rendered single-pass and
  tier-1 prompts fall to the next smaller shape only where that guard would
  refuse, so a prompt the reasoner can serve never spends the
  sub-distillation cap. The budget is a chars-per-token estimate, so
  data-health's likely-front-truncation read stays the runtime witness.
  Canonical at `docs/local-models.md §The local-model adapter seam` (ruled
  2026-08-28 off the 2026-08-24 review's reduce-prompt minor).
- **Stored price-denominated values never compare against fresh prices
  without the split-adjustment bridge.** FMP dated EOD re-bases
  retroactively, so a slice that stores a price (a threshold, target, entry,
  or comparator) and later reads it beside a fresh print routes through the
  bridge contract — canonical at `docs/portfolio-analysis.md §Starting
  parameters` (Split-adjustment bridge).
- **A grade-stamp bump describes itself.** `engine::GRADE_PARAMETER_VERSION`
  moves only with a row appended to `engine::GRADE_PARAMETER_HISTORY` naming
  what the bump changed on each branch (a test pins the last row to the
  current stamp), because the what-changed delta row and the continuity NOTE
  read a prior's boundary from that history — on the prior record's own
  branch, only over a priced prior, silent where nothing changed — and a
  generic "letters can move" row is citable evidence the validator accepts
  (ruled 2026-08-27, off the fund-momentum slice's Codex rounds; canonical at
  `docs/portfolio-analysis.md §Starting parameters`).
- **A price or NAV is usable only when finite and strictly positive.** The
  evidence floor tests usability, never presence — at the FMP quote parse
  (the one seam every quote consumer rides, the quick check's price refresh
  and the commodity quote included) and again at both engine floors, an
  unusable fund quote falling to a usable NAV — and the floor rule is
  stamped (`engine::EVIDENCE_FLOOR_VERSION`) on the checkpoint header and
  every audit record, so a resume never crosses a floor-rule change and a
  pre-stamp record reads as the presence floor (ruled 2026-08-28 off the
  review's Codex I1; canonical at `docs/portfolio-analysis.md §Evidence
  floor`).
- **A research-fed observation row is admitted on syntax, never on meaning.**
  The pre-profit producer's typed rows — the one model→engine channel that
  feeds a deterministic rule from prose — enter only through a verbatim,
  persisted `source_excerpt` the app verifies in the fetched page: the value
  at its printed sign inside the quote (read with the page's own neighbours,
  so a quote trimmed to the digits sheds nothing), the metric's own language,
  and exactly one number, every digit counting, a guidance-low/high row alone
  quoting a range's endpoints — so a compound sentence can never lend a stem
  to another clause's number. The filter is a syntactic admission test that
  cannot tell what the one number it admits means, and it loses an
  untrimmable row rather than admitting a wrong one; the contract rides the
  prompt stamp (`portfolio::PROMPT_VERSION`), and its residual is the
  review's queued I19 (ruled 2026-08-28 off Codex I3 across five Codex
  rounds, the semantic classifiers of the first two rounds each leaking by
  ordering; canonical at `docs/portfolio-workflow.md §Step 6e`).

### What each built slice left for the next

The ledger and sweep supply the selective machinery's triggering surface —
validated conditions with eval state and cadence tags, app-stamped monitor
bands, and the acknowledgment transition. Selective re-analysis adds the seams
later slices consume: per-holding vintages (`effective_vintage`), the persisted
`action_source` vocabulary, and the subset sweep. Outcome learning adds the
episode store and the `HoldingAudit.hurdle` snapshot the calibration-proposal
slice will consume.

Deliberate reductions in the quick check, surfaced rather than latent so they
are not mistaken for defects: FMP quote plus dated-EOD only (never the shared
price-bar cache), no cash-flow re-pull, no breadth-flip sub-leg, material
filings are the 10-K/10-Q/8-K prefix, and the FINRA leg is structurally
unreachable from the closed ledger series surface.

**Portfolio Analysis — built in full.** Built: the
per-holding spine and fund path (the closed-end leg included), the persisted
**thesis ledger** with machine-evaluable falsifiers over a closed engine
series surface, the engine-only **quick check**, **selective re-analysis**
with vintage-stamped carries, the **pre-profit overlay** (producer active;
§Standing constraints), **outcome learning**, the **metric-level 6g
validator**, **Step-6a semantic recall**, **per-holding checkpoint/resume**,
the per-holding **action call**, the **live research loop**, and the
**two-arm verdict** across all of it. The job is **tunnel-vision by contract** (`portfolio-v9`, ruled
2026-08-14; `docs/verification/2026-08-14-tunnel-vision-slice.md`): it never
compares holdings — the construction stage is removed whole, each action a
rung + one-line rationale from the finished verdict, the holding's own
evidence, and the investor profile (the profile's only entry point; 6f
interpretation stays profile-blind) — and whole-book questions are the
future portfolio planner's. Under the **2026-08-16 badge ruling** a
selective run analyzes **strictly the selection** — safety triggers surface
as non-blocking card badges, never force-includes — while a selective
request with no readable prior run runs the whole book (ruled 2026-08-18;
`docs/verification/2026-08-16-selective-badges-ruling.md`). The intrinsic
verdict is a **discriminated union** — `priced`, and `role_risk_only` for
structurally unpriceable vehicle classes, so no fabricated number rides an
unpriceable fund — and stays separated from the portfolio action
(`docs/portfolio-analysis.md §Intrinsic verdict`). The **live research loop**
landed as the job's final slice
(`docs/verification/2026-08-24-research-loop-rulings.md`; the held-name
refresh lane was retired by the badge ruling).

**Trade Opportunities — designed, not built** (`docs/trade-opportunities.md`,
`trade-opportunities-workflow.md`). Discovery runs through three feeders —
model-led hypothesis research (the edge: hypothesis cards scored *before any
ticker*), stratified structured feeders (stratification IS the breadth
mechanism), and a persisted opportunity-graph watchlist — with per-candidate
validation under an archetype lens, a mandatory bear case, and a
leading-metric hard gate. It runs as two jobs sharing one page (Discover /
Audit) under **one `trade_opportunities` job identity**, every run record
mode-labeled. Judgment fields carry in the same **two arms** as Portfolio;
**admission is either-arm** (both gate vectors persisted), the evidence floor
and forensic hard triggers binding absolutely on both arms; placement is
**model-authored** (the placement ruling — the engine's derivations stay the
baseline and the gate's shared legs, so the model never sets its own
admission bar). Deterministic outcome labels on prior picks plus a shadow
scorecard over every turned-away name feed a **propose-only, never
auto-applied** calibration; persistence separates six structures.

## What remains

The queue is governed by the **pre-run completion bar** (standing user
decision; widened 2026-08-14 from the original locked pre-test block, which was
fully built, and again 2026-08-20 to fold the live research loop inside it):
no further live runs until **the entire Portfolio Analysis job is built** —
every designed leg except work gated on realized-outcome evidence (grade
normalization, the calibration proposals, the derive-reads strata), which can
only come from a run — after which one **single big confirmation run** banks
every stacked runtime confirmation at once.

### Built

- **The cloud Market Signal Report** — the full 18-step pipeline, the agent
  layer, vector memory and continuity, the run tracker, and whole-corpus data
  portability.
- **The local suite's shared substrate** — the local-model layer and its
  Settings section, Schwab OAuth with holdings and options ingestion, the
  deterministic holdings-snapshot diff, the Portfolio page, and the
  Portfolio-runs history.
- **Portfolio Analysis** — the per-holding spine, the fund path (the CEF
  closed-end leg included), the thesis ledger, the quick check, selective
  re-analysis, the pre-profit overlay, outcome learning, the per-holding
  action call (tunnel vision — the construction stage is removed), the
  metric-level 6g validator, Step-6a semantic recall + per-holding summary
  embeddings, per-holding checkpoint/resume, and the two-arm verdict.
- **The calibration tier** — adapter options wiring, the target function, the
  grade bands, and the interpretation-prompt contract, each tuned against the
  first live run's persisted dataset.
- **The pre-run correctness program** — the conformance walks and ruling
  rounds (the tunnel-vision doc↔code walk is
  `docs/verification/2026-08-15-tunnel-vision-conformance-walk.md`), the
  per-doc sweeps, the logic-flow as-built walk, and the closing doc/code
  audit (21 findings, all addressed;
  `docs/verification/2026-08-18-portfolio-analysis-doc-code-audit.md`).
- **The progress step-ownership contract** — request events stamped with
  their owning step at the seam's single choke point, the tracker attaching
  rows by the stamp; every FMP suite row carries its shaped
  ok / empty / malformed outcome with the cause (`docs/run-tracking.md`).
- **The Stooq-removal slice** — FMP dated-EOD is the suite's only deep-price
  rung and `^GSPC` the market benchmark; the decision, evidence, and removal
  inventory are `docs/verification/2026-08-12-stooq-removal-decision.md`, the
  identity table at `docs/data-sources.md §Financial Modeling Prep`.
- **The evidence-legs slice** — the four remaining dossier evidence legs:
  the FINRA short-interest leg, the implied-expectations range, the
  narrative-vs-reality read (its hype cap the engine arm's soft Medium
  ceiling), and the same-underlying option overlay; each contract is
  single-homed in the docs (`data-sources.md` §FINRA and the chains row,
  `portfolio-analysis.md` §Starting parameters), and the quick check's
  FINRA sweep leg stays dormant.
- **The run-evidence slice** (`portfolio-v10`) — the Step-5 run-level
  context loads with their consumers: commodity / metals / gold context into
  dossier evidence, CFTC positioning into the fund read, the
  sector-benchmark legs, the CBOE venue-level put/call backdrop
  (`docs/data-sources.md §CBOE` is canonical), the technology-event
  pre-flag, and the hard-forensic producer with its consumer seam (the
  research `forensic_event` channel joins with the research loop).
- **The Infrastructure slice** (`portfolio-v11`) — the metric-level 6g
  validator, Step-6a semantic retrieval + per-holding summary embeddings,
  and per-holding checkpoint/resume; the contracts, 2026-08-21 rulings, and
  accepted residue are single-homed at `docs/portfolio-workflow.md` §Step 6a
  / §Step 6g, `docs/portfolio-analysis.md` §Failure posture, and
  `docs/storage.md §Local Vector Memory`.
- **The fund-depth slice** — the flat-driver fund target form ruled the
  settled design (closing conformance-walk R27; a scenario-differentiated
  formula returns only on realized-outcome evidence), N-PORT still
  deferred, and the CEF leg built as detection + the gap-honest
  price-vs-NAV seam; the rulings, probe findings, and six review rounds are
  `docs/verification/2026-08-21-fund-depth-rulings.md`, the contract
  canonical at `portfolio-analysis.md §Asset eligibility`.
- **The research-loop slice** (`portfolio-v12`) — the live per-holding web
  research: the SSRF-guarded web tool (SearXNG-primary, Tavily fallback,
  source registry + tiers), the 6c per-topic pass loop with always-run
  seed-and-merge caching, schema-constrained 6d with the typed side-channels,
  the 6e overlay finalization (the pre-profit producer activated), Settings
  §Web Research + the pre-run notice, and portability format v4. The
  2026-08-24 rulings bound the channels — forward assumption shadow-only,
  research-fed fraud advisory, the indicator anchor driver-id-gated
  (§Standing constraints); rendered retrieval and Connected Sources are
  deferred to their own slices (gating telemetry landed); the record and
  eight review rounds are
  `docs/verification/2026-08-24-research-loop-rulings.md`.

### Remaining, in order

1. **The single big confirmation run** — the queue's next item now the
   Portfolio Analysis job is built in full (the pre-run bar is met). By the
   2026-08-27 ruling the run waits until **every finding** in the 2026-08-24
   large-scale review
   (`docs/verification/2026-08-24-portfolio-analysis-large-scale-review.md`
   §Disposition owns the list) is handled: the pre-run majors (C1, F3
   (`portfolio-v13`), F1, F2, A1–A4, the hard-after-one-bounded-retry
   posture) and the Priority-1/-2/-3 minors are resolved; Codex's I5–I13 and
   I15–I19 (I1–I4 and I14 resolved 2026-08-28; I18 and I19 ruling
   items) and the §A4 seed edge sit ahead of the run, one finding per slice;
   and the user names the launch session at its start. Its checklist is
   `docs/verification/big-run-watch-set.md` (its two retired Stooq lines are
   now the FMP quota-consumption and 429-ladder watches), **revised to the v9
   shape 2026-08-18** (construction / lean / sizing watches removed, the
   prompt-fit watch re-homed to the per-holding prompts), revised again
   2026-08-24 with the research-loop, ruling, pre-profit-activation, and
   Schwab-CEF-typing watches folded in, and 2026-08-27 with the fired-retry
   watch; read `data-health` early, since several items resolve off that
   surface alone. Attempts 1 and 2 failed in the
   since-removed construction stage (their dated records live under
   `docs/verification/`); attempt 3 exercises the v9 shape as a full run.
   What stays open behind the run is owned by the attempt records'
   §Disposition, not this brief.
2. **Trade Opportunities** — designed, not built, waiting behind the entire
   Portfolio job and its confirmation run. The design is settled against
   live-verified paid FMP shapes and grounded end-to-end by the 2026-08-19
   program — the rewritten
   `logic-flow-docs/trade-opportunities-logic-flow.md`, the placement ruling
   (tier / horizon / runway two-arm; the model's authored tier × horizon
   places the card), and the full-corpus documentation audit with its seven
   rulings — every contract single-homed in the TO docs and indexed; the
   record is
   `docs/verification/2026-08-19-trade-opportunities-documentation-audit.md`.
   The not-yet-drafted constants (screener floors, archetype weight vectors,
   per-sector factor bands, the commodity-turn threshold, the
   diversity-allocation mechanics, the `illiquid` / event-exposure tier
   predicates) are marked inline in the logic-flow doc for the
   implementation plan to sweep. Five hard-trigger acceptance cases are
   parked for this slice and have no other home: a carried pick with a deep
   hard trigger archives with no shadow entry; a name arriving identically
   through all three deep-pass routes; a cheap-pass hard signal raises a
   warning only; a debut hard trigger becomes a shadow rejection; and a soft
   trigger caps the stand-in while preserving conviction, with no forced
   archival.

### Owned by no slice

These ride the queue rather than any one slice. They are collected here because
each is unbuilt work that no scheduled slice will pick up on its own.

- **Configurable investor profiles** — user config for the profile preset,
  deferred.
- **Paid-FMP baseline enrichment** — three additive report signals the paid key
  unlocks (calendar consensus + surprise, historical valuation percentile/band +
  performance trend, IPO/M&A froth), all engine-derived and outside the
  level-delta engine.
- **Keychain fail-soft** — a denied Keychain read currently errors the whole
  local-config report, which the frontend fail-safes to locked triggers with no
  local warning categories for that session. Fail-softing a failed token read to
  not-connected is a named, unbuilt candidate.
- **Stage-and-swap import** — a mid-import I/O failure can leave partial files
  (the row transaction holds, and the intact archive is the retry path). The
  hardening is named and unscheduled.
- **The local-suite guided-setup follow-up** — the Settings deferrals: in-app
  `ollama pull` with run-tracker progress (daemon start/stop stays the user's —
  ruled 2026-08-27), an
  Install-Ollama deep link (needs an opener capability), reflecting the run-gate
  connectivity check in the Settings daemon indicator (today it reflects manual
  tests only — an accepted, recorded deviation from `interface.md §Connection
  status`), and embedder re-embed-from-content (today an identity change clears
  the local namespaces atomically).

### Awaiting a ruling

Recorded rather than absorbed, each needing a decision before it becomes work:

- **Structured warning items** — emitting missing credentials as structured
  items from both gates instead of composed prose; a `WarningCategory` contract
  change.
- **The Settings tree's completeness gap** — `interface.md` omits two built
  panels while listing three designed-and-unbuilt ones.
- **Carried-audit data-health mixing** — carried audits mix prior-run retrieval
  outcomes into the run-level counts on selective runs, so one stale
  multiple-carry audit re-trips attention every run.
- **Unannotated off-scale model-arm renders** — model-arm sub-scores and targets
  are grammar-unbounded and render unannotated when off-scale; the inverted-band
  case got a tag, off-scale values did not.
- **`rate_prints.fetched_at`** — stamped with the run's `created_at` though the
  FRED fetch precedes the per-holding loop; consumed only by a last-resort
  fallback.
- **A reliability rule for model sub-scores and conviction** — recorded on
  the episode snapshot but unscored (no per-episode ground truth exists);
  extending the built target-band / outlook scoreboard to them is a
  calibration-tier question neither job has settled. TO's authored placement
  reads ride the same recorded-unscored treatment; a pooled
  placement-divergence rate was declined 2026-08-21
  (`trade-opportunities.md §Starting parameters`).

### Deferred by decision

Manual CSV/paste import supplements holdings but is not built. The
**sector-aware grade normalization slice** was retired by ruling (2026-08-13,
off attempt 2's letters): the no-A distribution is honest — quality and
valuation sub-scores anticorrelate structurally — so normalization returns
only on realized-outcome evidence, never on a letter distribution. An
**allocation optimizer** is deferred, not adopted — sizing and the optimizer
question are the portfolio planner's domain since the tunnel-vision ruling. The FINRA and CBOE evidence legs' deferral was reversed 2026-08-14 — they
landed with the Portfolio completion block (§Built). Trade Opportunities' blind-first
diagnostic is reserved diagnostic-only, its execution deliberately unspecified
until built. The fund slice's remaining drafted constants — the coverage and US
guards, tier premiums, add floors, and CIK-cache staleness — stay pinned until
the run supplies evidence to move them. The **engine stand-in arm** rides the
same rule: its outlook windows and flat thresholds, the conviction
degradation-count mapping, the action rung rule, and the scoreboard's
outlook-window mapping are all drafted, calibratable, and none yet calibrated
against live evidence.
