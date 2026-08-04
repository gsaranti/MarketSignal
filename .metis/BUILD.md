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
  serde_json text, and the crate's **`float_roundtrip` feature is load-bearing**
  (2026-08-03): round-trips are bit-exact, so carried numerics compare exactly
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
- **`frontend` (Vue 3)** — Latest Report View, the **Run Tracker**, the
  **shared-history sidebar** (recent reports; on the Portfolio view its list
  swaps to the Portfolio-runs history, an older run opening **read-only** —
  `docs/interface.md §Main Layout`), Research Documents, the Persistent
  Warning Area, Settings, and the **Portfolio page** (the first
  analytical-register surface) (`docs/interface.md`). Markdown→HTML rendering uses **markdown-it** on the
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
reasoning** — the main agent on its own channel, each analyst per-posture, and
a local job's per-holding interpretation **step-scoped** onto that holding's
own step (thoughts-only in each case; a review body or structured verdict
never streams). The streamed
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
**As-built:** the shared substrate, the single-equity Portfolio slice
(fixture Schwab + FMP + SEC + local models — and since 2026-07-31 **live-verified
end-to-end**: the first live run over a real 47-position book completed
successfully in the dev app, all disposition branches exercised, evidence in
`docs/verification/2026-07-31-first-live-portfolio-run.md`), the live Schwab
OAuth adapter + token lifecycle + Connect surface,
the deterministic holdings-snapshot diff, the Portfolio page with the
presence-only local warning categories, and the **full Portfolio (funds) slice**
(2026-07-16): book-level netting, the full ticker→CIK resolver, the per-symbol
FMP / Stooq / FRED evidence surface (dated DGS10 anchor-window history —
fail-soft to the raw-percentile fallback; the two run-level prints hard-fail),
the **v2 rate-anchored scenario-target function** with per-branch risk tiers,
the three-state hurdle + new-money admission and the engine-bounded feasible
set (momentum-free letter, rolling-window target names, add-family floors),
the strategy-classified fund path (exposure-priced composite under both ≥ 70%
guards, priced-fund grade contract, fund-form v2 targets, option-overlay
structural flag; net-short → not-rated), the `priced` / `role_risk_only`
verdict union rendered as the Portfolio page's two card branches, and
FMP / FRED presence on the local gate — triaged through two external review
rounds to convergence — and the **Local-analysis-models Settings section +
sidebar Portfolio-runs history** (2026-07-16): the provider-credential save
split (the two-token gate scoped to the cloud submission alone; FMP / FRED /
Tavily and the local-model config save **ungated** — the cloud-keyless setup
path), the local-models save with the **atomic embedder-identity guard** (an
identity change clears both local vector namespaces in the same transaction
as the write; re-embed-from-content stays M5-deferred), the manual daemon
**Test Connection** (untested / unreachable / model-missing / connected — the
in-app clear path for the shipped presence warning), and the read-only
past-run view — two Codex rounds to convergence — and the **thesis ledger
slice** (2026-08-03, `portfolio-v4`; five review rounds to convergence):
the persisted per-holding standing thesis, as-built under Portfolio
Analysis below — and the **quick check slice** (2026-08-03, PR #56; eight
external review rounds to convergence), the engine-only between-run sweep,
as-built under Portfolio Analysis below — and the **selective re-analysis
slice** (2026-08-04, PR #57; internal + two Codex rounds to convergence),
the chosen-subset re-run, as-built under Portfolio Analysis below — and the
**pre-profit overlay slice** (2026-08-04, PR #58; internal + four Codex
rounds to convergence), the deterministic execution/financing overlay,
as-built under Portfolio Analysis below — and the **outcome-learning
slice** (2026-08-04, PR #59; internal + four Codex rounds to convergence),
the recommendation-state-keyed decision-episode machinery, as-built under
Portfolio Analysis below. **Trade Opportunities and the remaining
Portfolio depth slices (held-name refresh lane, the 7b construction stage,
the live research loop) remain designed, not built.** The load-bearing
decisions:

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
  mode (bug #14645 verified fixed live on the pinned Ollama v0.32.5, so
  non-thinking distillation is unlocked on this version — it re-locks on any
  unverified bump); the 35B fast tier is **demoted to a benchmark-gated
  option**. Its serving path is **verified live** (M5 pre-flight, 2026-07-28):
  the 122B serves on the llama.cpp Metal/GGUF fallback, not MLX, fitting 128 GB
  with ~40 GB headroom at full native context, with effective context measured
  clean to 160.6 K (`docs/local-model-operations.md`; evidence record in
  `docs/verification/2026-07-28-m5-preflight.md`).
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
  holdings. Same-symbol rows across granted accounts (and future manual
  supplements) **net at snapshot assembly** into one signed book-level position
  per symbol — the holdings-normalization contract
  (`docs/schwab-integration.md §What is pulled`), built with the fund slice;
  per-source rows survive for display and audit, and a net-short book-level
  equity takes the not-rated treatment. The live
  source is chosen over the offline fixture by a connection
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
  the report. (Built with the fund slice: FMP / FRED presence joins the
  local gate through the shared missing-credentials category —
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
- **Portfolio Analysis (per-holding spine + fund path + thesis ledger built;
  remaining depth slices designed — `docs/portfolio-analysis.md`,
  `portfolio-workflow.md`; strategy
  audit converged 2026-07-10; the fund-form target methodology settled
  2026-07-16 — the v2 function over the exposure-priced composite).** The
  intrinsic verdict is a **discriminated union**: the **`priced`** branch is
  the four-part read — deterministic grade (momentum settled out of the
  letter; since the 2026-08-03 shadow-tune computed on a one-basis-per-holding
  **TTM statement basis** — the four newest quarterly income prints summed, a
  quarterly balance-sheet leg for debt / equity, SEC company-facts the
  **same-concept annual fallback**, fund holdings skipping the facts call
  entirely — under the recentered **`grade-v2`** bands, each audit stamped
  with its **grade-parameter version** so a recalibration stays attributable;
  weights and A–F cutoffs untouched, reserved for the normalization slice),
  first-class forward outlook, bidirectional conviction, portfolio action —
  and a structurally unpriceable vehicle class returns
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
  only *fails* is dead money (exit-side hysteresis) — and under the
  **`portfolio-v3`** interpretation contract (2026-08-03) a *fails* read
  reaches the model as a **weighed exit input**, set against the targets'
  **typed provenance** (the `TargetMeta` derivation flags — rate-anchored vs
  current-multiple carry, flat / clamp-flattened driver, dispersion floor —
  rendered into every priced interpretation, a floor-widened band inheriting
  its base's signal quality) and the data quality, never an exit instruction;
  the same contract defines **conviction against the action's decisiveness**,
  scopes the **house view** to horizon reads / market-setup context (never by
  itself a per-holding exit reason), and fires a **band-recalibration
  continuity NOTE** when the prior verdict's stamped grade version differs
  from the current bands; **new money passes its own base-case admission
  test**. The persisted per-holding **thesis ledger** (built 2026-08-03,
  `portfolio-v4`) is the standing thesis typed by verdict branch (`priced`
  full shape; `role_risk_only` a condition-only monitor + trim/sell-only
  triggers, enforced in the schema **and** at validation), its quantitative
  falsifiers / triggers machine-evaluable over a **closed 12-series engine
  surface** (cadence derived from the series; consecutive counts drafted
  1-filing / 2-market-data) under **distinct-observation streak identities**
  — market series keyed to the marks' trading day, filing series to the
  newest period end, the expense ratio to the changed print itself, **never
  calendar-keyed**; a no-dated-print read is typed unevaluable, with a
  margin noise guard and an acknowledgment transition blocking re-raise off
  the examined observation. The prior ledger + confirmed crossings render
  into both interpretation prompts (the first prior-run content the prompts
  carry); the model's rewritten ledger (required in both response schemas;
  it authors no ids, eval state, or target numbers) passes **6g
  validation** — executability downgrades rather than drops, identity carry
  is **order-independent** (exact-core reservation, then a global min-cost
  supersession assignment per (role, family, series) over the complete
  machine core; trigger families never exchange identity or crossing
  lineage), tripped/fired claims are honored only against a confirmed
  crossing on the carried id, superseded/closed conditions are preserved
  whole in the typed `LedgerAudit`, and monitor targets are app-stamped
  from the engine scenario set (structurally `None` on `role_risk_only`).
  The ledger rides `HoldingVerdict` in the run blob (pre-ledger runs decode
  as the debut path; insufficient-evidence retains it unchanged), anchored
  on the Portfolio card by the kit's ThesisAnchor (3-line clamp, measured
  reveal). Between runs the **quick check** keeps it live (built
  2026-08-03, PR #56; eight review rounds to convergence) — engine-only,
  no model / web / Schwab call, a single-row `portfolio_quick_checks`
  store deliberately **never** `portfolio_runs` (history, `latest_run`,
  and the diff baseline stay uncontaminated): per-holding typed
  `fresh_clear` / `flagged` / `unknown` family sweeps (an
  allowed-but-unresolvable condition **downgrades its family's claimed
  clear** via the typed `unevaluable_series` channel; `unknown`
  force-includes), the four flag triggers — confirmed falsifier breach,
  fired trigger, hurdle newly failing on the **closed-form re-anchor**
  (`engine::reanchor_scenarios` over the stored `QuickCheckBasis`
  percentiles / drivers; the filing re-pull's dividend leg refreshes the
  payout under the adapter's None-with-no-gap = confirmed-non-payer
  contract), and — since the 2026-08-03 open-questions sweep — a **change
  in spot's relation to the frozen monitor band** (inside / below / above,
  tested against the authoring-time relation 6g stamps beside the engine
  targets, `ThesisLedger.authored_band_relation`; leave / re-enter /
  side-cross flags, an authored-outside standing state never re-flags, a
  pre-stamp ledger reads authored-inside) —
  merge-not-replace flag carry with eval-state chaining sweep-to-sweep,
  equity + fund evidence-event legs (the fund mandate / label /
  overlay-flag comparisons independently gated on what each actually
  derives from — `FundExposureBasis` carries the overlay
  `structural_flag` as `Option<bool>`, a legacy `None` degrading rather
  than fabricating; degraded retrievals type `unknown`, never a
  fabricated change; blank `etf/info` strings normalize to `None` at the
  adapter), and the full-run seam: sweep eval states overlay at ledger
  carry, confirmed crossings are consumed and acknowledged at 6g, and
  clearing is **per successful pass** — an abstention retains its
  carried state re-stamped to the new run, its rate cache following the
  run's prints. The store rides data portability as **format v2**
  (versioned closed entry set); quick-check job rows are excluded from the
  footer's last-run stamps (`job_status` filters `portfolio_quick_check` —
  a sweep is not analysis freshness; failures still reach the failed-jobs
  warning). Deliberate reductions, all surfaced: FMP
  quote + dated-EOD (no Stooq cache exists), no cash-flow re-pull, no
  breadth-flip sub-leg, material filings = 10-K/10-Q/8-K prefix, the
  FINRA leg structurally unreachable (no short-interest series in the
  closed 12-series surface).
  **Selective re-analysis** (built 2026-08-04, PR #57): Run analysis with a
  per-card selection analyzes the work-list — selection ∪ new-since-last-run
  ∪ no-prior-verdict, side reversals (`PositionDelta::side_reversed`),
  over-age exit-family carries, plus the **in-run tail sweep's** flags /
  `unknown` families / unexamined evidence events (the quick-check core made
  subset-capable, `quick_check::sweep_tail`; it reuses the run's fresh rate
  prints, no second FRED call) — and carries the rest forward
  **vintage-stamped**: `HoldingVerdict.analyzed_at` (a fresh pass stamps the
  run's `created_at`; an insufficient-evidence exit preserves its prior
  vintage), with the **evidence-event boundary now the per-holding vintage**,
  never the run's `created_at`, in the standalone quick check too. A carried
  verdict keeps its ledger with the sweep's eval states overlaid, its prior
  audit row whole (`quick_basis` / `fund_exposure` survive the carry), its
  `position_change` refreshed from this run's diff, and its **action sizing
  recomputed at current weights** on both branches (sizing is engine
  context, never carried stale); an over-age carried add-family action
  rule-demotes to *hold*, stamped **`action_source: rule-demoted`** (the
  canonical `model-chosen` / `rule-demoted` vocabulary, persisted on every
  verdict). The persist seam retains carried holdings' sweep state
  re-stamped to the new run, beside the abstention retention. Deliberate
  waits: the transition rule's model-facing validation (incl. the
  context-trim carve-out) rides the 7b stage — with no construction call,
  carried actions only re-affirm or demote toward hold, so the rule holds by
  construction — and the refresh-lane force-include leg rides its lane.
  Frontend: per-card selection + select-all/clear, the "Analyze N selected"
  trigger, carried / stale vintage tags and the demotion tag in the quiet
  badge family. The **held-name research refresh lane** checks at most two
  otherwise-reused-or-carried holdings per run against one named qualitative
  ledger driver / falsifier and can only force the normal full pass; it never
  changes a verdict. The **pre-profit execution / financing overlay** (built
  2026-08-04, PR #58, `portfolio/pre_profit.rs`; prompt contract
  **portfolio-v5**): every priced stock records an overlay — eligibility
  (TTM operating income ≤ 0, or no positive forward-EPS consensus with
  negative TTM FCF; missing inputs = **not entered, gap recorded**) gating a
  statement leg off the new quarterly cash-flow pull + balance-sheet cash
  lines — liquid resources (an absent STI line reads zero, recorded), TTM
  burn, runway months under the 24/12-month financing bands, capex intensity,
  split-adjusted YoY diluted-share change (FMP's retroactive split adjustment
  live-verified on NVDA's 10:1), and the two-quarter gross-margin
  progression. The observation machinery — typed rows, structural validation
  with typed rejections, period-keyed dedup merge, the ≥5% /
  ≥2-of-latest-four-periods / ≥20% miss rules, the conjunctive severe state
  (financing + dilution alone never suffices) — is built **producer-dormant**
  (research stubbed, candidate list empty; the research-loop slice **must**
  add holding-identity + source-text validation before activating the
  producer). Consequences are engine-owned and triple-enforced — schema
  narrowing of the action AND conviction enums, feasible-set bars
  (constrained runway / severe strips the add family; severe restricts to
  {trim, sell all} — action = lean until 7b), and a post-interpretation
  min-clamp with `clamped_from` recorded; repeated miss caps Medium, severe
  caps Low, the letter and targets never move. The full record rides
  `HoldingAudit.pre_profit` (boundary-epsilon-tolerant thresholds; carried
  whole by the selective carry; retained through abstention like the standing
  ledger), and statement canonicalization is a **shared policy** with a named
  home — `engine::canonicalize_statements`, applied in place at the
  `dossier::apply_ttm_statement_basis` choke point (2026-08-04, `eb9295c`), so
  every statement-consuming engine read — the TTM basis, the driver ladder's
  growth-clamp trailing prints and share basis, the anchor windows — sees
  both quarterly statement vecs sorted (period_end, filing_date) descending +
  period-end-deduped, and a served-twice restatement resolves to the latest
  filing, never wire order (`pre_profit::statement_inputs` keeps its local
  sort — order-independence there is a test-pinned standalone contract). **Outcome
  learning** (built 2026-08-04, PR #59, `portfolio/outcome.rs`; internal +
  four Codex rounds to convergence): branch-typed decision episodes carrying
  the calibration-feature snapshot open on **observable state changes only**
  — debut (incl. the upgrade and lost-active-row re-seed seams), branch
  flip, action change, weight-range change, rule demotion; the
  standing-thesis leg and the self-correction read ship **dormant** behind
  the unbuilt 6g attribution validator — extend the **latest** active
  episode on re-affirmation / carry / abstention, and persist in their own
  store independent of the 10-run retention (matured archive capped;
  per-row fail-soft load — an unreadable row is skipped and reported, never
  deleted). Engine-computed 1/3/6/12-month labels read through the shared
  `price_bars` cache: session-proximity-bounded next-session entry,
  total-return primary off a label-time dividends re-pull (a failed pull =
  the labeled price-only fallback), typed price-coverage / terminal
  closures past the shared grace — and every authored-price comparison
  (band calibration, the falsifier lead-time line) crosses bases through
  the **anchor-close bridge** (`anchor-session close × price ⁄ authoring
  spot`), split- and gap-safe, **excluded rather than guessed** when the
  spot or anchor bar is missing. Derived reads are unique-holding-counted —
  lean-keyed cohorts (vintage-fresh, model-chosen; rule-demoted and
  role-risk their own classes; a missing TR leg quotes price-only, the
  labeled-mix rule), target calibration split per parameter version
  (return-space Winkler interval score), falsifier lead times, and the
  typed below-bar eligibility record (proposal statistics deferred behind
  the ≥ 30-unique-matured-holdings bar) — intrinsic calibration keyed on
  the standalone lean, never the construction-shaped final action, feeding
  a propose-only calibration. Alignment tags come from the next run's
  deterministic diff; sector identity is entry-stamped via a fail-soft FMP
  `/profile` read + the static sector→SPDR map; the run's records ride
  `PortfolioRun.outcome`, and the episode store + price-bar cache join
  portability as **format v3**.
  Funds are strategy-classified at loop time and routed (exposure-priced
  proxy valuation for ≥70%-US equity funds; their structurally absent quality
  axis uses the shared neutral-50 imputation; honest gaps elsewhere).
- **Trade Opportunities (designed — `docs/trade-opportunities.md`,
  `trade-opportunities-workflow.md`).** Discovery through three feeders —
  **model-led hypothesis research** (the edge: hypothesis cards + a score
  gating promotion *before any ticker*), with an app-owned **calendar-time
  coverage rotation** backed by a discovery-coverage ledger; stratified
  structured feeders (the screener stratifies — stratification IS the breadth
  mechanism, no bulk pre-scoring); and a persisted **opportunity-graph
  watchlist**, whose structured / filing metrics refresh at class cadence and
  whose research-class nodes have a small, current-search refresh lane. It then
  performs per-candidate validation under an archetype lens, a mandatory bear
  case, and a leading-metric hard gate. App-eligible new-listing,
  spin-off/carve-out, or new-economic-perimeter cases may contribute identity-,
  perimeter-, and comparability-validated direct or recast **limited-history
  evidence** without weakening any normal floor or gate; proxies remain
  corroboration only. The engine retains a structured-only
  target counterfactual while the app validates any sourced, claim-id-based
  **research target scenario** and computes the resulting targets; a validated
  thesis-milestone DAG supplies the `milestone-chain` horizon basis and carries
  condition-identity-keyed evaluation state. Runs as two jobs sharing one page
  (**Discover** / **Audit**, the latter forking Quick/Deep); a reserved,
  maintenance-priority **rotation slice** of the deep budget keeps the live
  matrix's research bounded-stale (non-disableable — floored at one slot).
  Deterministic outcome labels on prior picks (recorded onto durable,
  **lifecycle-keyed picked episodes** that outlive matrix / archive / run
  retention) **and a shadow scorecard over every name the funnel turned away**
  (typed decision episodes, a strict measurement contract) feed a
  **propose-only, never auto-applied** calibration. Persistence separates six
  structures: matrix, opportunity graph, discovery-coverage ledger,
  price-tracked departed-pick archive, shadow ledger, and picked-episode store.

## What remains

The queue is governed by the **locked pre-test block** (user decision
2026-08-02): **no further live runs until the block is fully built** — the
calibration tier, the two result-review UI micro-slices, and every remaining
Portfolio depth slice except the live research loop and the held-name refresh
lane (which rides with it) — then one **single big confirmation run** banks
all the stacked runtime confirmations (grade-v2 letter distribution and
whether ordering holds; TTM-basis adoption + the balance-sheet leg live;
target provenance vs the sell-all cascade; the recalibration NOTE and its
what-changed attribution; conviction/action pairing; the fails →
indeterminate action distribution; fund-SEC-skip noise reduction; the fund
carry-path floor; debut ledger authorship quality at 47-position scale;
live condition carry / supersession behavior; tripped-claim discipline; the
ThesisAnchor + clamp render; the data-health render; the reasoning panes;
the first live quick-check sweep at 47-position scale (flag / badge /
degraded-note render, the card overlay) and the debut-gap self-resolution
(the rate-anchor and pre-basis fund families read `unknown` until the big
run re-persists); the first live **selective run** (selection UI, the
in-run tail sweep, carried-card vintage / stale / demotion render at
scale — and the transition-only `PriceOutsideBand` flag's live behavior:
leave / re-enter / side-cross flag rates against the stamped authoring
relation, since the authoring-time-outside design question was settled in
code 2026-08-03 — the standing state no longer flags; note pre-sweep
ledgers carry no stamp and read authored-inside until re-analyzed); the
first live **pre-profit overlay** read at 47-position scale (eligibility
rates, financing-state distribution, unscorable-gap rates — including the
STI-absent-reads-zero liquid-resources convention and the YoY share-change
quarter-contiguity assumption); the first live **outcome-learning** pass
(episode-debut volume at 47-position scale, sector-resolution rates
through the fail-soft profile read, the below-bar eligibility note); 128 K
runner
stability; distill speed; whether Stooq's PoW gate is permanent). **Trade
Opportunities waits behind the whole block** (design settled — full strategy
audit plus three external review rounds to convergence, 2026-07-09; the paid
FMP key's shapes live-verified 2026-07-16, so implementation planning codes
against verified shapes).

The **calibration tier is done** — four slices tuned against the first live
run's persisted dataset (run `3b21ae85`; the run completed 2026-07-31 over a
real 47-position book with zero mechanical failures — findings in
`docs/verification/2026-07-31-first-live-portfolio-run.md`; the model-serving
pre-flight completed 2026-07-28, Ollama pinned v0.32.5 —
`docs/verification/2026-07-28-m5-preflight.md`):

- the **adapter options-wiring slice** (2026-08-01) — explicit tri-state
  per-stage `think` (F3 closed — `Some(false)` always serializes, distill
  runs non-thinking on the verified pinned Ollama), per-mode sampling
  profiles, per-stage `num_ctx` under the **one-`num_ctx`-per-model** rule
  (an Ollama `num_ctx` change reloads the resident runner despite
  `keep_alive`), `keep_alive:-1` residency including the embedder, and the
  **step-scoped live thinking channel** (F8);
- the **target-function calibration slice** (2026-08-01, `targets-v3` —
  F1/F2 closed) — the **NTM consensus read** (the two nearest forward
  fiscal-year rows blended time-weighted by twelve-month overlap, the
  selection persisted on the target meta), the **volatility-scaled
  widen-only dispersion floor** plus the recorded growth-clamp collapse,
  **Stooq resilience** (typed daily-hits notice, run-wide breaker,
  politeness pacing, the FMP dated-EOD second rung), and the **run-level
  data-health roll-up** rendered with an attention state;
- the **grade-band shadow-tune** (2026-08-03, `grade-v2` — F4/F5 closed) —
  the sub-score formulas **certified exact** against run `3b21ae85`'s
  persisted audits before any retune, the statement-input gaps closed (the
  **TTM statement basis**, the quarterly **balance-sheet leg**, the SEC
  **same-concept prior-year** annual fallback with latest-filed dedup, the
  **fund facts-call skip**), the calibration surface refreshed via a
  user-approved bounded probe, and the recentered bands user-picked off a
  sweep — **weights and A–F cutoffs deliberately untouched** (no A letters
  yet — META 84.0 vs the ≥ 85 cutoff — reserved for the sector-aware
  normalization slice or the big run's evidence); evidence in
  `docs/verification/2026-08-03-grade-band-shadow-tune.md`;
- the **interpretation-prompt adjustments slice** (2026-08-03,
  `portfolio-v3`) — the prompt contract described in §Local analysis suite
  above (target provenance, the softened dead-money weighing, the conviction
  definition, house-view scoping, the band-recalibration continuity NOTE).

The **first five depth slices are done** — the **thesis ledger**
(2026-08-03, `portfolio-v4`), the **quick check** (2026-08-03, PR #56),
**selective re-analysis** (2026-08-04, PR #57), the **pre-profit
overlay** (2026-08-04, PR #58), and **outcome learning** (2026-08-04,
PR #59) — as-built contracts in
§Local analysis suite. The ledger + sweep supplied the selective
machinery's triggering surface (validated conditions with eval state +
cadence tags, app-stamped monitor bands, the acknowledgment transition,
the engine seams — `engine::resolve_series`,
`engine::evaluate_ledger_conditions[_gated]`,
`engine::reanchor_scenarios`); the selective slice adds the seams the
later slices consume: per-holding vintages (`effective_vintage`), the
persisted `action_source` vocabulary, and the subset sweep
(`quick_check::sweep_tail`); the outcome slice adds the episode store +
`HoldingAudit.hurdle` snapshot the calibration-proposal slice will
consume, and reserves the episode `lean` / `lean_divergence` pair so 7b
diverges lean from final action without an episode-schema migration.

**Remaining in the block: the 7b construction stage** (which picks up the
carried-action transition-rule validation — toward-hold-only plus the
aggregate-validated context trim — over the now-persisted `action_source` +
vintage stamps, and diverges the episode `lean` from the final action over
the reserved `lean_divergence`) — with the two small,
display-only result-review UI fixes (the Portfolio-page polish
micro-slice; the section-scoped footer + report-nav slice) slotting
anywhere between slices as breathers. Excluded from the block and still designed-not-built: the
**live research loop** and the **held-name research refresh lane**; the
shipped schemas don't preclude them — but the research-loop slice **must**
add holding-identity + source-text observation validation (plus a period-
normalization hard rule) before activating the pre-profit producer, the
obligation recorded in `pre_profit.rs`'s validator doc comment. Two structural gaps ride the queue
rather than any one slice: **checkpoint/resume** stays docs-promised but
unbuilt (the ledger persists only at run end), and only the **ledger legs
of the 6g validator** exist — the metric-level input delta / what-changed
attribution validator remains designed, owned by no shipped slice, and now
also gates the outcome slice's dormant legs (the standing-thesis creation
leg and the self-correction read).

Watches and deferrals: **Stooq now serves a
JS-PoW interstitial to non-JS clients** (observed 2026-08-02 during the FMP light-EOD desk probe, which
closed the adjustment-basis question — FMP's basis is Stooq's exact
split-adjusted dividend-unadjusted convention —
`docs/verification/2026-08-02-fmp-light-eod-adjustment-basis.md`), so the
FMP dated-EOD rung may be de facto primary; Stooq stays the primary rung by
user decision, revisited only on the big run's data-health evidence (a
rung-order slice + FMP re-homing the contingent follow-up). A named,
unscheduled **local-suite guided-setup follow-up** carries the
Settings-slice deferrals: in-app `ollama pull` with Run-Tracker progress +
`ollama serve` start, an Install-Ollama deep-link (needs an opener
capability), reflecting the run-gate connectivity check in the Settings
daemon indicator (today it reflects manual tests only — an accepted,
recorded deviation from `interface.md §Connection status`), and embedder
re-embed-from-content (M5-gated; today an identity change clears the local
namespaces atomically). Still M5-gated: the fund slice's remaining drafted
constants (the ≥ 70% coverage / US guards, tier premiums, add floors,
CIK-cache staleness); the FMP paid-key shape checkpoint closed 2026-07-16
(`78df109`).
