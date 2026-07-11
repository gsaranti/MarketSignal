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
the **bundle identifier**, so it is stable across versions. Debug builds nest
under a `dev/` subdir so a development session never touches production data;
`MARKET_SIGNAL_DATA_DIR` overrides both. The **macOS Keychain rail sits outside
this split** — the keyring service (`market-signal-schwab`) is app-scoped, not
data-dir-scoped, so debug and release builds read the same Schwab entries. The
startup Keychain reads are synchronous main-thread calls and macOS re-prompts
its ACL for every ad-hoc-signed rebuild, so a fresh binary's first launch can
stack prompts that **block the webview's first paint**; a denied read errors the
whole local-config report, which the frontend fail-safes to locked triggers with
no local warning categories for that session (fail-softing a failed token read
to not-connected is a named, unbuilt candidate). One deliberate exception to
*persisted config lives in SQLite*: the Light/Dark appearance preference lives
in webview `localStorage` — pure presentation with no backend consumer, read
synchronously pre-mount to avoid a first-paint flash.

**Data portability (built — PRs #53/#54, `portability.rs`).** A whole-corpus
backup/restore — distinct from per-report export — that carries a machine's
accumulated analytical history to new hardware as one archive
(`docs/data-portability.md`). The load-bearing line: **durable analytical data
moves; secrets and machine-local operational state stay behind** —
`app_settings` and the Keychain are never serialized, so the archive cannot leak
a credential. It is a structured, versioned, checksummed zip — deliberately
**not** a raw DB-file copy (WAL sidecars; secrets can't be stripped from a
binary copy; no DB schema-version marker) — and import **validates everything
before its destructive phase**, so a bad archive can only abort while the store
is untouched. Optional encryption is AES-256-GCM over an Argon2id key with the
**KDF cost parameters frozen in code, never the crate's defaults** (an inherited
default shift on a dependency bump would strand every archive as "wrong
passphrase"; raising costs means a new `ENC_MAGIC`). Both directions hold the
single run slot. Accepted residue: a mid-import I/O failure can leave partial
*files* (the row transaction holds; the intact archive is the retry path) —
stage-and-swap is a named, unscheduled hardening.

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
  `Retry-After`-aware retry/backoff; GDELT is excluded — its escalating IP
  lockout makes retrying harmful, so it stays single-shot fail-soft. **Fixed
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
- **`frontend` (Vue 3)** — Latest Report View, the **Run Tracker**, Recent
  Reports Sidebar, Research Documents, the Persistent Warning Area, Settings,
  and the **Portfolio page** (the first analytical-register surface)
  (`docs/interface.md`). Markdown→HTML rendering uses **markdown-it** on the
  webview side, on demand for display and PDF export, **never persisted** —
  agents never see HTML. PDF export uses the webview's native print-to-PDF,
  where the page margin comes from the report article's **padding**, not
  `@page`: a non-zero `@page` margin makes WebKit silently drop content that
  spills onto an added page, so `@page` stays 0 (the cost — interior pages get
  no top/bottom margin — is a WebKit limitation, not a choice). Embedded charts
  ride the same seam: the agent emits a fenced `chart` JSON block in its
  Markdown and `src/renderChart.ts` is the authoritative validator rendering it
  to restrained inline SVG, falling back to the raw code block on anything
  malformed — the *only* way a chart enters a report. All UI is built against
  `market-signal-design-system/`, which defines **two registers** — the report's
  **reading register** (serif, monochrome, unchanged) and a denser,
  instrument-grade **analytical register** the local-suite surfaces adopt as
  they are built — bridged by shared chrome, which now includes the package's
  confirmation dialog and the keyboard-operable sort-bar / sortable-grid-head /
  view-toggle controls (the first two are built into the Portfolio page; the
  view toggle ships with Trade Opportunities). All suite sorting/view controls
  are **display-only**, reordering already-computed fields; specifics live in
  the design package and `docs/interface.md`.

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
frontend renders this as the run tracker — one shared component placed on the
**running job's own page** (a report run replaces the report pane, a portfolio
run the Portfolio page; latest-run-only), the report's fixed /8 progress
fraction applying to report runs only. Cancellation is cooperative — a shared flag
polled at step/request boundaries and mid-stream, never interrupting an in-flight
request. Two load-bearing UI invariants: a **run is never a report** (a row
appears only on persisted success, so a cancel/fail removes nothing), and the
terminal `run-finished` event is emitted **before** any job-history write error
can propagate, so a DB failure can't strand the UI mid-run. The full runtime
contract is in `docs/run-tracking.md`.

## Testing approach

The spine makes the pipeline testable offline: agents and adapters are traits, so
the orchestrator runs end-to-end against deterministic stubs and fixture packets
with no live keys. Coverage spans the executor's three limits, the 30-report
retention cascade + durable-learning survival, learning dedup, the validation-gate
and Step-3 coverage-floor matrices, the failed/skipped/cancelled transitions,
fail-soft inbox parsing, the cadence-honest baseline delta engine, and the analyst
layer's fail-hard contract. The `progress` seam stays out of other tests via a
no-op `RunContext`; its own logic (the resumable streamed-token decoder + SSE
reconstruction for both provider dialects and stream roles) is fixture-tested. Each
gated adapter has a test-only base-URL seam so a localhost mock exercises the full
URL-build → retry → parse → output path offline; live smokes are `#[ignore]`d. The
**frontend gate is two runners under `npm test`**: pure modules (`tests/**/*.test.ts`)
on Node's runner via type-stripping, and Vue **SFC tests** (`tests/**/*.spec.ts`) on
**Vitest** (happy-dom + `@vue/test-utils`), mounting real components for behavior +
accessibility.

The same trait spine powers a **dev-only demo-run mode** (`src-tauri/src/demo.rs`,
behind a `demo-run` Cargo feature, out of `default`/`tauri build`): "Generate now"
drives the *real* `run_job` pipeline through the live GUI against paced streaming
stand-ins — run tracker and report rendering end-to-end with no network, keys, or
cost (`npm run tauri:demo`).

## Local analysis suite

A second capability set: two on-demand, **local-model-only**, deliberately
**prescriptive** features (grades, actions, targets — a departure from the
report's no-buy/sell stance) — **Portfolio Analysis** (grades the user's Schwab
holdings and recommends actions + price targets; a typed role/risk read where a
vehicle class is unpriceable) and **Trade Opportunities**
(researches new ideas across a 3×3 risk×horizon matrix). Full design lives in
`docs/local-models.md`, `web-research.md`, `schwab-integration.md`,
`portfolio-analysis.md`, `portfolio-workflow.md`, and `trade-opportunities.md`.
**As-built:** the shared substrate, the narrow single-equity Portfolio slice
(fixture Schwab + FMP + SEC + local models, offline-verified; live validation is
M5-gated), the live Schwab OAuth adapter + token lifecycle + Connect surface,
the deterministic holdings-snapshot diff, and the Portfolio page with the
presence-only local warning categories. **Full Portfolio (funds) and Trade
Opportunities remain designed, not built.** The load-bearing decisions:

- **A local-only model layer, distinct from the cloud report (built).** A
  flexible local-model adapter (`local_model.rs`) calls one **user-installed,
  app-supervised** Ollama daemon over its native `/api/chat`
  (grammar-constrained `format` for schema-valid output; token / reasoning
  streaming on the existing `progress` seam), through the same
  `reqwest::blocking` / `spawn_blocking` seam the cloud agents use — **added
  rather than extending the closed cloud `AgentModel` enum**, so the roster
  changes through configuration. The app **bundles neither the daemon nor the
  models**; it makes setup turnkey *around* a user-installed Ollama (guided
  install + in-app pull with progress). The suite gate holds the report's
  **presence-not-connectivity** posture: *presence* of config gates
  **proactively** (locked Run buttons + a persistent warning) while
  *connectivity* is checked only at the **run-gate** and on a manual Test
  Connection, **never at startup** — a config-set-but-daemon-down state is blind
  on re-open, the deliberate cost of no startup probe. A `LocalEmbedder` reuses
  the existing `Embedder` trait so `vector_memory` is unchanged. The roster
  default is **settled**: one frontier reasoner (Qwen3.5-122B-A10B) **plus the
  embedder stay resident**, the 122B filling *every* reasoning role by thinking
  mode (schema-constrained distillation stays thinking-enabled until Ollama bug
  #14645 is verified fixed); the 35B fast tier is **demoted to a
  benchmark-gated option**. Its serving path is an **M5 pre-flight risk** — the
  122B runs on the llama.cpp Metal/GGUF fallback, not MLX
  (`docs/local-model-operations.md`).
- **Per-job isolation (learnings only).** Each feature stores its own runs
  (last-N retention) and its own vector-memory partition; no job reads another's
  *learnings*. The Market Signal Report stays a read-only shared input, loaded
  deterministically (not vector-searched), additionally isolated by embedder
  dimensionality.
- **A cost-free web tool.** Self-hosted, keyless SearXNG for search plus a Rust
  fetch/readability-extract layer, Tavily as fallback; the orchestrator runs the
  tool, the model only requests it — holding the pure-stage boundary. SearXNG
  isn't bundled — the app *ships configuration, not the server* (a pinned
  `docker-compose.yml` with the two load-bearing settings baked in). The fetch
  is a plain GET with realistic browser-like headers; thin extraction trips a
  **selective rendered-retrieval tier reusing the already-embedded Tauri
  webview** — not a bundled browser or Python sidecar — gated on telemetry so
  rendering stays **measured, never blanket**. SearXNG sits **off the execution
  gate**: unreachable means a degraded run (Tavily fallback; fewer candidates on
  the SearXNG-only TO discovery lane) behind a pre-run notice, never a block.
  The per-item research loop is bounded (per-topic passes, depth ≤2, a
  fetch+wall-clock budget that binds first), SSRF-guarded, every finding keeping
  its source URL + timestamp; consolidation is one shared **distillation
  primitive** — single pass by default, map-reduce chosen deterministically by
  evidence-ledger size, tier-1 always seeing *complete* findings. Optional
  **Connected Sources** (in-app login → Keychain session, on the Schwab
  credential rails) enrich fetching and are **never part of the execution
  gate**.
- **Holdings & options ingestion (built).** Schwab Trader API via an OAuth
  loopback (30-min access / 7-day refresh → a weekly re-login), supplying
  holdings *and* live option chains, from which a deterministic put/call +
  IV/skew signal is computed — an activity proxy, not positioning truth, kept
  out of grade sub-scores until calibrated. **A connected Schwab account is
  required to run either local job** — manual CSV/paste import only supplements
  holdings. The live source is chosen over the offline fixture by a connection
  gate (`MARKET_SIGNAL_SCHWAB_FIXTURE` keeps the fixture for offline runs). The surface is **read-only by construction** — the adapter
  implements only holdings/positions/option-chain `GET`s and never an
  order/trading endpoint. This is a code-enforced guarantee, not a token scope:
  the Trader API bundles trading into the same product with **no read-only
  scope**, and it exposes **no money-movement endpoints at all** (money
  movement is a separate Advisor Services API), so the read-only boundary lives
  in our code while the worst-case blast radius of a leaked credential stays
  bounded to in-account trades the app never issues. Access/refresh tokens and
  the app secret ride the Keychain and **never enter logs or the run tracker**;
  the client id is a non-secret in `app_settings`. The loopback's HTTPS server
  is an **in-house one-shot rustls acceptor** (`loopback_https`): the security
  audit found the original tiny_http server hard-pinned an EOL rustls/ring
  stack (RUSTSEC-2024-0336 unfixed) and no maintained minimal blocking-HTTPS
  crate exists, so the ~150-line acceptor rides the same rustls + ring stack
  outbound HTTP already uses, and the capture loop is offline-tested over real
  TLS. Only the interactive browser round-trip stays a live `#[ignore]` smoke.
- **Reuses the spine.** Each feature is a new Tauri command + job under the
  **single global run slot** (report + both local jobs are mutually exclusive,
  matching the latest-run-only tracker), reusing the `progress`/run-tracker seam
  and the `vector_memory` / `Embedder` modules. Local-gate failures get their
  own warning categories (`schwab_gate` + `local_gate`), kept **off the cloud
  `validate` gate** — a disconnected account blocks only the local jobs, never
  the report. (Settled, engine update pending: FMP / FRED presence joins the
  local gate through the shared missing-credentials category — the as-built
  gate merges only the local-model and Schwab checks —
  `portfolio-workflow.md §Step 1`.) Both jobs are personalized by a **fixed default investor-profile
  preset** (user config deferred) that frames the prescription, never which
  holdings or ideas qualify — nor the intrinsic verdict (profile-independence
  is declared).
- **Invariants governing the designed features** (full specs in the docs; a
  plan must not work against these):
  - **Deterministic finance, primary-source evidence** — a shared Rust engine
    over FMP + keyless SEC EDGAR / Stooq / FINRA / CBOE computes every
    sub-score, risk tier, metric, and scenario target; **the model interprets,
    never invents numbers**. One shared FMP key, upgraded to paid (`*-bulk`,
    transcripts, 13F-institutional, fund-holdings, and press-releases are
    **off-plan** → SEC EDGAR / 8-K / web-loop / N-PORT fallbacks); the report's
    data-source logic is unchanged. An **evidence floor** returns
    `insufficient-evidence` over a low-conviction guess — with **debut
    semantics**: a carried live name's inconclusive re-read holds its last
    verdict, never a turn-away; long jobs **checkpoint/resume** (resume is its
    own entry path on the run's pinned snapshot); early runs are
    **shadow/calibration**.
  - **Anti-reflexivity / no-double-count** — research may *raise* conviction
    only via a typed, app-validated `validated_leading_indicator` (≤ one band),
    never via price or narrative; an unconfirmed price gain caps conviction,
    never boosts it; the archive never self-promotes.
  - **Source quality informs conviction, never gates discovery** — tiers grade;
    only the explicit deny list drops.
  - **Only a deep re-evaluation can archive an opportunity; the cheap
    re-derivation never does** — it refreshes the quant read and raises a
    non-destructive attention warning.
- **Portfolio Analysis (designed — `docs/portfolio-analysis.md`,
  `portfolio-workflow.md`; strategy audit converged 2026-07-10, with one
  named input deliberately open — the fund-form target methodology, the
  first decision of the fund slice's plan).** The
  intrinsic verdict is a **discriminated union**: the **`priced`** branch is
  the four-part read — deterministic grade (momentum settled out of the
  letter), first-class forward outlook, bidirectional conviction, portfolio
  action — and a structurally unpriceable vehicle class returns
  **`role_risk_only`** (no letter / targets / lean / conviction; a reduced
  {sell all, trim, hold} spine) so no fabricated number rides an unpriceable
  fund. The intrinsic verdict stays separated from the portfolio action: the
  per-holding loop emits the verdict plus a standalone lean (`priced`
  branch only), and a
  post-roll-up construction stage (deterministic aggregates → model
  reconciliation, joint-feasibility-checked) sets the final action + sizing —
  the engine **bounds the feasible action set and the model chooses within**,
  so "A-grade business, trim because oversized" is expressible (an allocation
  optimizer is **deferred, not adopted**). Capital efficiency tests **total
  return** against a DGS2-anchored, tier-scaled, **three-state hurdle** —
  only *fails* is dead money (exit-side hysteresis); **new money passes its
  own base-case admission test**. The persisted per-holding **thesis ledger**
  (typed by verdict branch; observation-identity persistence semantics) is
  evaluated deterministically each run and kept live between runs by an
  engine-only **quick check** (warn-don't-decide attention flags);
  **selective re-analysis** re-runs a chosen subset under three safety rules
  (force-include on flags / `unknown` degraded sweeps / side reversals /
  deterministic evidence events, the carried-action transition rule,
  over-age add-demotion). **Outcome
  learning** records
  recommendation-state-keyed decision episodes (matured archive +
  calibration-feature snapshot) under engine-computed labels — total-return
  primary, price-only common basis, each derived read on a declared basis and
  cohort layer, intrinsic calibration keyed on the standalone lean, never the
  construction-shaped final action — feeding a propose-only calibration.
  Funds are strategy-classified at loop time and routed (exposure-priced
  proxy valuation for ≥70%-US equity funds; their structurally absent quality
  axis uses the shared neutral-50 imputation; honest gaps elsewhere).
- **Trade Opportunities (designed — `docs/trade-opportunities.md`,
  `trade-opportunities-workflow.md`).** Discovery through three feeders —
  **model-led hypothesis research** (the edge: hypothesis cards + a score
  gating promotion *before any ticker*), stratified structured feeders (the
  screener stratifies — stratification IS the breadth mechanism, no bulk
  pre-scoring), and a persisted **opportunity-graph watchlist** — then
  per-candidate validation under an archetype lens, a mandatory bear case, and
  a leading-metric hard gate. Runs as two jobs sharing one page (**Discover** /
  **Audit**, the latter forking Quick/Deep); a reserved, maintenance-priority
  **rotation slice** of the deep budget keeps the live matrix's research
  bounded-stale (non-disableable — floored at one slot); deterministic outcome
  labels on prior picks (recorded onto durable, **lifecycle-keyed picked
  episodes** that outlive matrix / archive / run retention) **and a shadow
  scorecard over every name the funnel turned away** (typed decision episodes,
  a strict measurement contract) feed a **propose-only, never auto-applied**
  calibration; departed picks land in a price-tracked archive.

## What remains

In order: **full Portfolio (funds)** (`docs/portfolio-analysis.md §Asset
eligibility`; strategy audited to convergence 2026-07-10 — the fund-form
scenario-target methodology is the plan's named blocking input, decided first)
→ the **Local-analysis-models Settings section** (also the in-app
clear path for the shipped presence warning) and the **sidebar Portfolio-runs
history** → **Trade Opportunities** (design settled — full strategy audit plus
three external review rounds to convergence, 2026-07-09; investment logic ready
for implementation planning when the queue reaches it). Hardware-gated on the M5: live local-suite
validation, the model-serving pre-flight, and the calibration knobs.
