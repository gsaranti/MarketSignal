# BUILD — Market Signal

*Architecture brief for the app: the load-bearing decisions and their rationale —
the durable shape future work builds on — not the construction history
(commit-by-commit detail lives in git; per-feature specifics live in `docs/`).
The body is as-built; the trailing section tracks planned work not yet
implemented.*

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
  baseline snapshots, and the vector-memory table.
- **Vector memory** — one embedding per report summary and per durable learning
  (`text-embedding-3-large`), each an atomic unit (no chunking). It lives as a
  `vector_memory` table inside the same SQLite database, with exact brute-force
  cosine in Rust — a deliberate engine choice over LanceDB: at this corpus's
  scale (≤30 retained summaries plus low-thousands of learnings) an unindexed
  vector DB runs the same exhaustive scan, while LanceDB would cost a heavy,
  async-only dependency tree against the deliberately synchronous spine. Two
  seams contain the choice: the `vector_memory` module owns all store access,
  and the `embedding::Embedder` trait owns text→vector.

The **report-summary metadata** is a JSON object stored with each report. The
application owns the **identity fields** — `report_id` (a minted UUID),
`report_type` (the constant `market_signal`), and `created_at` (an app-clock
timestamp) — while the main agent authors the **judgment fields**: a short
per-issue `title`, **`risk_posture`** ∈ {risk-on, risk-off, mixed},
**`market_cycle`** ∈ {late-cycle, recessionary, recovery}, `thesis_stance` ∈
{bullish, bearish, mixed, uncertain}, `header_summary_bullets` (3–6), plus the
optional `key_risks` / `unresolved_questions` / `forward_outlook_themes` arrays.
Risk posture and market cycle are two **orthogonal axes**, not a single regime
field (full schema in `docs/storage.md §Report Summary Metadata Schema`).

The **Step-3 baseline scan** produces an in-memory `BaselineMarketData` packet —
indices, internals, sectors, macro/labor levels, the release calendar,
multi-horizon index performance, equity-breadth movers and earnings, and the
valuation/rotation groups (sector and industry P/E, market risk premium), and
**CFTC futures positioning** (Commitments-of-Traders speculator nets on the
bellwether contracts) — plus a **`gaps` missing-data manifest**. Partial failures degrade rather than abort:
an adapter records each series it can't resolve as a tagged `DataGap` that rides
into the prompt, so the model reasons over what's absent rather than inferring
it. The **single coverage floor** lives in the app layer (`enforce_coverage`),
the one point a too-thin baseline fails the run. Each run's baseline persists to
a `baseline_snapshots` table; the next run reads the latest prior snapshot and
computes a deterministic, **cadence-honest per-report change view** — level
deltas over the level-bearing groups (positioning excluded — it carries its own
native week-over-week change), anchored on the *actual* elapsed interval
since the prior snapshot, never an assumed week — that rides into the prompt
alongside the live baseline.

**Planned (paid-FMP report enrichment, spec'd not built).** Upgrading the shared
FMP key to paid unlocks three additive baseline signals (`docs/data-sources.md
§Planned report enrichment`): economic-calendar **consensus + realized surprise**
(layered onto FRED's release schedule via a curated release→event map, fail-soft
to today's names+dates), **historical sector/industry valuation + performance**
(trailing-window P/E percentile + band, plus a cumulative-return trend accumulated
from the performance endpoint's daily `averageChange`), and **IPO/M&A froth**
(issuance/deal pace + a native recent-vs-prior trend, the way positioning carries
its own change — a new baseline group, so the Step-3 group count moves 12→13). All three hold the spine: the engine derives every
number, only the compact derived read persists (raw series discarded), new fields
carry `#[serde(default)]`, and none joins the level-delta engine (set-valued /
trailing-window, like positioning). The only existing logic that changes is the
**calendar builder** (FRED-only → FRED+FMP join, fail-soft) and one **main-agent
prompt** instruction that currently forbids over-time valuation reads (must be
revised). Live-verified on the paid-key checkpoint. **True index breadth** — the
fourth enrichment candidate — was evaluated and **ruled out**: FMP exposes
constituent *lists* but no breadth metric, so a real breadth read would need a
heavy per-constituent price fan-out; the movers group stays the breadth proxy.

**Retention** is deliberately asymmetric and must be honored in deletion code:
only the most recent **30 reports** are kept (deleting one cascades its Markdown,
metadata, and vector *summary* row together — there is no HTML leg, since HTML is
rendered on demand and never persisted), **but durable learnings survive report
deletion**, guaranteed by the row's `kind` rather than its `report_id`. Baseline
snapshots keep their own cap (14), decoupled from report retention.

The on-disk home for all stores is resolved from the Tauri app-data dir keyed by
the **bundle identifier**, so it is stable across versions (rebuilding or
replacing the installed app reads the same store). Debug builds nest under a
`dev/` subdir so a development session never touches production data;
`MARKET_SIGNAL_DATA_DIR` overrides both. One deliberate exception to *persisted
config lives in SQLite*: the Light/Dark appearance preference lives in webview
`localStorage` — pure presentation with no backend consumer, read synchronously
pre-mount to avoid a first-paint flash.

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
  synthesis — which the planned paid-FMP enrichment above narrows to a fallback
  where FMP carries no estimate). Data honesty is a consistent stance: a stale FRED
  observation or an out-of-band FMP P/E aggregate **drops to a gap / `None`**
  rather than feeding a fabricated level into the baseline. The newest source,
  **CFTC** (keyless, like BLS), adds **Commitments-of-Traders positioning** — the
  one signal the price / valuation / macro / credit groups can't give (how crowded
  the speculative cohort is), as a fail-soft, additive group. Gated adapters share
  a bounded, `Retry-After`-aware retry/backoff; GDELT is excluded — its
  escalating IP lockout makes retrying harmful, so it stays single-shot
  fail-soft. **Fixed internal models** are non-configurable and distinct from the
  four user-selectable agent models: GPT-5 mini (headline filtering), Claude
  Sonnet (research routing), `text-embedding-3-large` (embeddings). Inbox
  document parsing runs **no model** — it is deterministic excerpting, so a
  model summary can't omit or fabricate over the user's own source material.
- **`agents` (prompt + schema contracts)** — the main agent and the
  Bull/Bear/Balanced analysts (run concurrently, no ordering dependency), plus a
  **16-lens analytical skills library** supplied in full to both the main agent
  and the analysts. Skills are **forcing-function-only**: each lens's verdict
  disciplines the report/review prose but is never parsed back or persisted (the
  report prose is the output; a rare keep-worthy verdict exits via a
  `durable_learning`). Analyst reviews are ephemeral — never persisted. The main
  agent's editorial posture is **conviction-first**: the thesis commits to a
  directional base case — the most-probable path and the reasons for it — and weights
  the alternatives around it rather than presenting co-equal either/or branches, so the
  report reads as a *call* rather than a summary of the packet. A `mixed` / `uncertain`
  `thesis_stance` is the earned exception (genuinely two-sided, or evidence too degraded
  for a directional read), not a safe default; the base case carries forward across
  reports and pivots only when the evidence has materially changed
  (`docs/thesis-continuity.md`) — the conviction and the rare-pivot doctrine are the
  same stance, not opposites.
- **`frontend` (Vue 3)** — Latest Report View, the **Run Tracker** (live
  per-step/per-request progress with streamed agent output), Recent Reports
  Sidebar, Research Documents, the Persistent Warning Area, and Settings
  (`docs/interface.md`). Markdown→HTML rendering uses **markdown-it** (JS), so
  HTML generation lives on the webview side, rendered on demand for display and
  PDF export and **never persisted** — agents never see HTML. PDF export uses the
  webview's native print-to-PDF, where the page margin comes from the report
  article's **padding**, not `@page`: a non-zero `@page` margin makes WebKit
  silently drop content that spills onto an added page, so `@page` stays 0 (the
  cost — interior pages get no top/bottom margin — is a WebKit limitation, not a
  choice). Embedded charts
  ride the same seam: the agent emits a fenced `chart` JSON block as part of its
  Markdown and `src/renderChart.ts` is the authoritative validator that renders
  it to restrained inline SVG (line/bar/area), falling back to the raw code
  block on anything malformed. The `chart` block is the *only* way a chart enters
  a report — the app layer never injects one — keeping faith with the
  agents-emit-Markdown / frontend-renders spine. All UI is built against the
  design system in `market-signal-design-system/`.

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
`RunContext` (reporter, shared cancel flag, sequence) are threaded into
`generate_report` and the real adapters/agents via `with_context` builders, so
**no trait signature changes** for the seam. While a job runs the app streams to
an open window: per-step progress, one **request row per actual HTTP call**, the
main agent's report **token-by-token**, and the agent models' **extended-thinking
reasoning** — the main agent on its own channel and each analyst per-posture
(thoughts-only for analysts; the review body never streams). The streamed
report tokens are a side-channel that can't corrupt the report — the full
envelope is accumulated and parsed exactly as the non-streaming path. The
frontend renders this as the run tracker (replacing the report pane while a run
is in flight; latest-run-only). Cancellation is cooperative — a shared flag
polled at step/request boundaries and mid-stream, never interrupting an in-flight
request. Two load-bearing UI invariants: a **run is never a report** (a row
appears only on persisted success, so a cancel/fail removes nothing), and the
terminal `run-finished` event is emitted **before** any job-history write error
can propagate, so a DB failure can't strand the UI mid-run. The full runtime
contract is in `docs/run-tracking.md`.

## Testing approach

The spine makes the pipeline testable offline: because agents and data adapters
are traits, the orchestrator runs end-to-end against deterministic stubs and
fixture packets with no live keys. Coverage spans the bounded executor's three
limits; the 30-report retention cascade *and* durable-learning survival;
near-duplicate learning dedup; the validation-gate pass/block matrix; the Step-3
coverage-floor matrix; the failed-vs-skipped-vs-cancelled state transitions;
fail-soft inbox parsing; the baseline delta engine and its cadence-honest
elapsed pass-through; and the analyst layer's fail-hard contract (a single
failing analyst aborts the run with no report persisted). The `progress` seam
stays out of every other test via a no-op `RunContext`; its own logic — the
resumable streamed-token decoder and the SSE delta/envelope reconstruction for
both provider dialects and both stream roles — is unit-tested against fixtures.
Each gated adapter carries a test-only base-URL injection seam so a localhost
mock exercises the full URL-build → retry → parse → domain-output wire path
offline, where a live key was previously the only coverage; live-provider smokes
are `#[ignore]`d. The **frontend unit gate is two runners under `npm test`**,
split by file extension: pure modules (`tests/**/*.test.ts`) on Node's built-in
runner via TypeScript type-stripping (no build step), and Vue **SFC component
tests** (`tests/**/*.spec.ts`) on **Vitest** (`@vitejs/plugin-vue` + happy-dom +
`@vue/test-utils`), mounting real components to assert behavior and accessibility
against the design system.

The same trait spine powers a **dev-only demo-run mode** (`src-tauri/src/demo.rs`,
behind a `demo-run` Cargo feature): hitting "Generate now" drives the *real*
`run_job` pipeline through the live GUI against paced, streaming stand-ins that
emit per-request rows and stream tokens/thinking, then delegate to the offline
stubs for return data — so the run tracker and report rendering are exercised
end-to-end with no network, keys, or cost. The feature is not in `default` and so
is compiled out of `tauri build`; it's the cost-free way to verify UI/report
changes (`npm run tauri:demo`).

## Local analysis suite (substrate + narrow Portfolio slice built; features in progress)

A second capability set: two on-demand, **local-model-only** features —
**Portfolio Analysis** (grades the user's Charles Schwab holdings and recommends
actions + price targets) and **Trade Opportunities** (researches new ideas across a
3×3 risk×horizon matrix). The **shared substrate is built and merged** (PR #44),
and a **narrow single-equity Portfolio slice** ships the first per-feature pipeline
end-to-end against a **fixture Schwab source** (offline) plus FMP + SEC + the local
models (PR #45) — **full Portfolio (funds) and Trade Opportunities remain planned**.
Full design lives in `docs/local-models.md`, `web-research.md`,
`schwab-integration.md`, `portfolio-analysis.md`, `portfolio-workflow.md`, and
`trade-opportunities.md`. The
load-bearing decisions (the model layer and the narrow Portfolio pipeline are
as-built; the rest remain planned):

- **A local-only model layer, distinct from the cloud report (built).** A flexible
  local-model adapter (`local_model.rs`) parameterized by `{endpoint, model_id,
  messages, tools, format_schema, options}` calls one app-supervised Ollama daemon
  over its native `/api/chat` (grammar-constrained `format` for schema-valid output;
  token / reasoning streaming on the existing `progress` seam), through the same
  `reqwest::blocking` / `spawn_blocking` seam the cloud agents use — **added rather
  than extending the closed cloud `AgentModel` enum**, so the roster changes through
  configuration. Daemon supervision (`health_check` + roster probe) feeds an
  **independent local-suite gate** (its own `WarningKind::LocalModels`, separate from
  the cloud `validate` gate), and a `LocalEmbedder` reuses the existing `Embedder`
  trait so `vector_memory` is unchanged. The roster default is **settled**: one frontier reasoner
  (Qwen3.5-122B-A10B) **plus the embedder stay resident**, and the 122B fills *every*
  reasoning role — research/interpretation in thinking mode, distillation in
  non-thinking — so co-residency of a second large model is sidestepped and a
  holding's research passes then single distillation pay no model-swap cost. The fast tier
  (Qwen3.5-35B-A3B) is **demoted to a benchmark-gated option**, reintroduced only if
  distillation wall-clock is a measured bottleneck *and* a 122B+35B+embedder set
  co-resides cleanly on-device. Roster is configurable; the roster never runs more
  than one large reasoner.
- **Per-job isolation (learnings only).** Each feature stores its own runs
  (last-N retention) and its own vector-memory partition; no job reads another's
  *learnings*. The Market Signal Report stays a read-only shared input, loaded
  deterministically (not vector-searched). The report is additionally isolated by
  embedder dimensionality.
- **A cost-free web tool.** Self-hosted, keyless SearXNG for search plus a Rust
  fetch/readability-extract layer, with the existing Tavily as fallback; the
  orchestrator runs the tool, the model only requests it — holding the pure-stage
  boundary. The per-item research loop is worked **one agenda topic at a time** —
  each topic ≤3 research passes (root + ≤2 app-governed follow-ups, depth ≤2; a pass
  is itself a bounded multi-turn tool loop) under a **per-item fetch+wall-clock budget**
  that binds first (bounding the raw turns/fetches) and is spent in topic-priority
  order, fail-soft on exhaustion. Model-chosen fetches are SSRF-guarded (no
  private/loopback hosts,
  bounded size/redirects, untrusted content) and every finding keeps its source
  URL + timestamp.
- **Holdings & options ingestion.** Schwab Trader API via an OAuth loopback
  (30-min access / 7-day refresh → a weekly re-login); it supplies holdings *and*
  live option chains, from which a deterministic put/call + IV/skew signal is
  computed — an activity proxy, not positioning truth, kept out of grade
  sub-scores until calibrated (CBOE gives a Cboe venue-level put/call backdrop). **A connected Schwab
  account is required to run either local job** — manual CSV/paste import only
  supplements holdings and does not clear the gate, so both jobs block at each
  re-auth. FMP/SEC stay the financial sources; tokens live in the macOS Keychain;
  non-equity positions (options, bonds, cash) are marked not-rated. Schwab
  developer-app approval (a few days) is the external long pole.
- **Reuses the spine.** Each feature is a new Tauri command + job under a
  **single global run slot** (report + both local jobs are mutually exclusive,
  matching the latest-run-only tracker), reusing the `progress`/run-tracker seam
  and the `vector_memory` / `Embedder` modules; local-job gate failures get their
  own warning categories. The cloud report is unchanged. Build order: substrate
  (**done** — PR #44) → narrow single-equity Portfolio slice (**done** — PR #45:
  fixture Schwab + FMP + SEC + local models, offline-verified; the engine computes
  every number and the model only interprets; per-job `vector_memory` namespace
  partition added; live verdict-quality/runtime + FMP-tier validation is
  hardware-gated on the M5) → **next: wire live Schwab OAuth** (+ deterministic
  holdings-snapshot diff: prior-run snapshot vs current pull → per-position
  new/increased/decreased/unchanged delta into each dossier, exited names surfaced in
  the roll-up) → full Portfolio (funds) → Opportunities (archetype-aware two-mode
  discovery on FMP's paid tier; ticker→CIK now an optional cross-check, not a blocker).
- **Personalized & screened.** Both local jobs are personalized by an investor
  profile that is, **for now, a fixed default preset** (long-term horizon, profit-max
  objective, medium-to-high risk, cash treated as always available, no tax adjustment;
  user configuration deferred) — it frames the prescription, never which holdings or
  opportunities qualify. Trade
  Opportunities **discovers** candidates by running research (a funnel: top-down
  theme/event scans + bottom-up screens — estimate-revisions, surprises,
  commodity-price turns, keyless positioning — *find* names; per-candidate validation
  scores them), hunting in two modes (**early** = a leading operating metric inflecting
  before price/earnings/multiple; **continuation** = demand-visibility licensing a late
  entry) through a first-class **archetype** lens (secular-compounder / ai-infra /
  commodity-cyclical / disruptor / quality-compounder) that selects the signal weights +
  valuation lens, gated by a mandatory bear case, a narrative-vs-reality
  (revisions-vs-multiple, with an operating-reality-vs-price fallback when coverage is too thin to read revisions) check, a **cross-lens contradiction check** folded into distillation+scoring (no extra model call; high-severity contradictions capped deterministically), and a forensic risk gate. A cell may return nothing.
  Its **research method is itself load-bearing** (`trade-opportunities.md §The research
  method`): worldview-first (a regime backbone reused from the house view + a forward
  thematic map, traced *economically* — margin capture / bargaining power / capacity /
  pricing power, not mere exposure), then five lenses — quant composite, value-creation, macro-thematic-fit,
  investor-judgment, and case-study pattern — reconciled as **two tracks** (proven- vs
  emerging-economics) through one moat/management/price-asymmetry gate, with an inflecting
  **leading-metric hard gate** plus a valuation-vs-forward red-flag as the spine, and
  breadth across **all market caps** (protected at the discovery funnel's waist by
  deliberate diversity quotas — cap / feeder / archetype / theme). Each run also computes
  deterministic **outcome labels** on prior picks (return vs sector/market, drawdown,
  leading-metric continuation, a decision-tree failure mode) as durable learnings, feeding
  a forward-staged archetype-weight/gate **calibration** (early runs stay shadow/calibration).
  Model residency keeps
  the **122B + embedder** resident (the 122B fills every reasoning role by mode); a
  second small model (35B) is the benchmark-gated option, not a default — never two
  large reasoners co-resident (see the model-layer bullet above).
- **Deterministic finance, primary-source evidence.** Quantitative outputs —
  sub-scores, risk-tier assignment, valuation/quality/momentum/risk metrics, and
  scenario price targets (methodology exposed) — are computed by a Rust
  financial-analysis engine over **FMP plus keyless SEC EDGAR** (10-K/Q/8-K +
  XBRL company facts); the model interprets, never invents numbers. High-volume
  **price history** stays on **keyless Stooq** (multi-decade depth — the input the
  engine's price-action confirmer and momentum/volatility reads need); live **quotes**
  come from FMP, and the dispersal is load-relief on the **paid** shared FMP key, not
  free-cap avoidance.
  **Trade Opportunities widens the signal set on FMP's paid tier** (the suite's one
  paid dependency; **one shared FMP key for the report + both jobs, upgraded to paid —
  the report's data-source logic is unchanged**, former free-tier gates no longer bind): fundamentals/ratios/segments, the revision
  signal (estimates + `grades-historical` + price-targets + upgrades/downgrades +
  surprises), **`financial-scores` (Altman Z + Piotroski) for the forensic gate**,
  symbol-keyed **positioning** (insider, 13F, **congressional**), and screener/peers/**bulk**
  endpoints driving the discovery funnel. **Short interest** keyless on FINRA; commodity
  prices on FRED+Stooq; **SEC EDGAR retained as authoritative cross-check** (so ticker→CIK
  is a non-blocking enhancement, not a prerequisite). An **engine-computed price-action
  confirmer** (relative strength / multi-year base breakout from Stooq history) adjusts
  conviction — a confirmer, not a trigger. DRAM/NAND ASPs + supply discipline have no feed
  and ride the research loop.
  With FMP kept for its low-volume niche (movers, earnings calendar, screener,
  sector/industry P/E). An **evidence
  floor** returns `insufficient-evidence` (not a low-conviction guess) when data
  is missing/stale/conflicting. Long per-holding jobs **checkpoint and resume**,
  and early runs are treated as **shadow/calibration** before outputs are trusted.
  Portfolio's per-holding design is now fully specified (`portfolio-workflow.md` is the
  Type-tagged control flow): a **three-layer engine** (grade core from fundamentals +
  price + forensics; a conviction layer of revision / narrative-vs-reality; positioning
  context held out of the grade until calibrated), forward targets **refinable
  post-research only via a typed, sourced `research_forward_assumption`** the engine
  recomputes (sub-scores never move), and a **what-changed audit** attributing every
  verdict move to a deterministic input-delta (external) or a flagged **self-correction**
  — app-validated so it can't be faked. Funds take a reduced compute path (look-through
  concentration from `etf/holdings`) with a fund-analog evidence floor; the house view is
  dropped past a **one-week freshness** window. Portfolio points the paid per-symbol feed
  at *held* names plus its own adds (segment revenue, earnings-call transcripts, FINRA
  short interest, dividends, ETF/fund endpoints).

Both features are deliberately **prescriptive** (grades, actions, targets) — a
departure from the report's no-buy/sell stance — applying the report's house view
to the user's specific positions and to new ideas.
