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

**Data portability (built — `portability.rs`).** A whole-corpus
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
  `Retry-After`-aware retry/backoff, parameterized per provider:
  **FMP rides a minute-crossing 429 ladder** (63 s cumulative
  over seven attempts — probe-verified that the paid plan's per-minute limit
  arrives as an HTTP 429 with a burst bucket tripping well under the headline
  200/min; 5xx keeps the short schedule so a down provider fails fast, and
  backoff sleeps poll the run's cancel flag in ~250 ms slices) while every
  other adapter keeps the short default; GDELT is excluded — its escalating
  IP lockout makes retrying harmful, so it stays single-shot fail-soft. **Fixed
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
envelope is accumulated and parsed exactly as the non-streaming path. A
**debug-gated thought-log sink** decorates the live reporter and captures
every thinking stream to per-run text files under the data dir
(`thought-logs/`, newest ten kept, pruned only after a run's first delta
lands) — the deliberate, bounded exception to reasoning staying ephemeral:
release builds stay silent unless `MARKET_SIGNAL_THOUGHT_LOG` opts in, and
the files are best-effort diagnostics outside SQLite, portability, and every
store retention rule (`docs/run-tracking.md §Thought-log capture`). The
frontend renders this as the run tracker — one shared component placed on the
**running job's own page** (a report run replaces the report pane, a portfolio
run the Portfolio page; latest-run-only), the report's fixed /8 progress
fraction applying to report runs only. Cancellation is cooperative — a shared flag
polled at step/request boundaries and mid-stream, never interrupting an in-flight
request. Two load-bearing UI invariants: a **run is never a report** (a cancel/fail
removes nothing that was shown), and the terminal `run-finished` event is
emitted **before** any job-history write error can propagate, so a DB failure
can't strand the UI mid-run. The first invariant carries one deliberate,
**built** exception on the Portfolio-runs history: a run whose Step 7b
construction fails — an infeasible plan and a failed construction call alike —
persists its per-holding work as a **degraded run**: still terminally failed,
excluded from `latest_run`, and marked by a **persisted `constructed` marker
authored at the persist seam** — mirrored into a store column so the exclusion
filters in SQL without parsing a blob, and shipped on the listing row and the
full-run payload so the UI's "no book" tag reads the marker rather than
re-deriving run health from field shapes. The history therefore reads as
persisted work rather than succeeded runs, and a degraded row's
pre-construction actions can never become the next run's diff/carry/quick-check
baseline. Corrupt run rows hold the same posture at read: an unparseable blob
costs its own surface (skipped with a logged warning), never the history
listing or the next run's baseline, and the refusal surfaces distinguish a
degraded-only store from an unreadable one.
The full runtime contract is in `docs/run-tracking.md`.

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
report's no-buy/sell stance). **Portfolio Analysis** grades the user's Schwab
holdings and recommends actions + price targets, typing a role/risk read where a
vehicle class is structurally unpriceable. **Trade Opportunities** (designed, not
built) researches new ideas across a 3×3 risk×horizon matrix. Full design lives in
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
  rendering stays **measured, never blanket**. SearXNG sits **off the execution
  gate**: unreachable means a degraded run behind a pre-run notice, never a
  block. The per-item research loop is bounded and SSRF-guarded, every finding
  keeping its source URL + timestamp, and consolidation is one shared
  **distillation primitive** whose mode is chosen deterministically by
  evidence-ledger size. Optional **Connected Sources** (in-app login → Keychain
  session, on the Schwab credential rails) enrich fetching and are **never part
  of the execution gate**.
- **Holdings & options ingestion (built).** Schwab Trader API via an OAuth
  loopback (30-min access / 7-day refresh → a weekly re-login), supplying
  holdings *and* live option chains, from which a deterministic put/call +
  IV/skew signal is computed — an activity proxy, not positioning truth, kept
  out of grade sub-scores until calibrated. **A connected Schwab account is
  required to run either local job**; manual CSV/paste import (designed, not
  built) only supplements holdings. Same-symbol rows across granted accounts
  **net at snapshot assembly** into one signed book-level position per symbol
  (`docs/schwab-integration.md §What is pulled`); per-source rows survive for
  display and audit, and a net-short book-level equity takes the not-rated
  treatment. The surface is **read-only by construction** — the adapter
  implements only holdings/positions/option-chain `GET`s and never an
  order/trading endpoint. This is a code-enforced guarantee, not a token scope:
  the Trader API bundles trading into the same product with **no read-only
  scope**, so the boundary lives in our code while the worst-case blast radius
  of a leaked credential stays bounded to in-account trades the app never
  issues. Tokens and the app secret ride the Keychain and **never enter logs or
  the run tracker**. The loopback's HTTPS server is an in-house one-shot rustls
  acceptor on the same stack outbound HTTP already uses — no maintained minimal
  blocking-HTTPS crate exists that isn't pinned to an EOL stack.
- **Reuses the spine.** Each feature is a new Tauri command + job under the
  **single global run slot** (report + both local jobs are mutually exclusive,
  matching the latest-run-only tracker), reusing the `progress`/run-tracker seam
  and the `vector_memory` / `Embedder` modules. Local-gate failures get their
  own warning categories, kept **off the cloud `validate` gate** — a
  disconnected account blocks only the local jobs, never the report. Both jobs
  are personalized by a **fixed default investor-profile preset** (user config
  deferred) that frames the prescription, never which holdings or ideas qualify
  — nor the intrinsic verdict, whose profile-independence is declared **and
  input-isolation-enforced**: the intrinsic prompt carries no profile, which
  reaches the model at whole-book construction only.
- **Invariants governing the suite** (full specs in the docs; a plan must not
  work against these; each states its own reach):
  - **Deterministic finance, primary-source evidence** — a shared Rust engine
    over FMP + keyless SEC EDGAR / FINRA / CBOE (the last two designed,
    unbuilt; Stooq removed 2026-08-12 — FMP dated-EOD is the only price rung)
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
deterministic finance), plus `fund`, `listing`, `quick_check`, `construction`,
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
- `construction::merge_validated_actions` — the one place the final action,
  sizing and attribution are set.
- `pre_profit::clamp_conviction` — the overlay's engine-arm ceiling.
- `store::load_episodes` / `prune_matured_episodes` — episode identity and
  lifecycle.

BUILD cites version constants rather than duplicating their current values,
so this brief cannot go stale as they move:
`portfolio::PROMPT_VERSION`, `engine::GRADE_PARAMETER_VERSION`,
`engine::SCENARIO_TARGET_PARAMETER_VERSION`,
`pre_profit::PRE_PROFIT_PARAMETER_VERSION` and `portability::FORMAT_VERSION`.
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
- **The pre-profit producer stays dormant** until the research-loop slice adds
  holding-identity and source-text observation validation, and normalizes
  reported periods to one convention per issuer. Both obligations are recorded
  in `pre_profit.rs` — the validation pair on the validator's doc comment, the
  normalization rule on the `period` field's.
- **Model-arm values never bind the engine baseline.** The typed validated
  channels are the only model→engine inputs: ledger conditions today, with the
  dormant pre-profit observations and the research loop's forward assumption
  joining them when those slices land.
- **The Schwab adapter implements no order or trading endpoint.** The read-only
  boundary is code-enforced, not scope-enforced, so it survives a token scope
  change.

### What each built slice left for the next

The ledger and sweep supply the selective machinery's triggering surface —
validated conditions with eval state and cadence tags, app-stamped monitor
bands, and the acknowledgment transition. Selective re-analysis adds the seams
later slices consume: per-holding vintages (`effective_vintage`), the persisted
`action_source` vocabulary, and the subset sweep. Outcome learning adds the
episode store and the `HoldingAudit.hurdle` snapshot the calibration-proposal
slice will consume, and reserved the episode `lean` / `lean_divergence` pair
that the construction stage now populates without a schema migration.

Deliberate reductions in the quick check, surfaced rather than latent so they
are not mistaken for defects: FMP quote plus dated-EOD only (never the shared
price-bar cache), no cash-flow re-pull, no breadth-flip sub-leg, material
filings are the 10-K/10-Q/8-K prefix, and the FINRA leg is structurally
unreachable from the closed ledger series surface.

**Portfolio Analysis — built**, less two depth slices. Built: the per-holding
spine and fund path, the persisted **thesis ledger** with machine-evaluable
falsifiers over a closed engine series surface, the engine-only **quick check**
between runs, **selective re-analysis** with vintage-stamped carries, the
deterministic **pre-profit execution/financing overlay** (producer-dormant;
§Standing constraints carries what activating it requires), **outcome
learning** over branch-typed decision episodes, the
whole-book **construction stage** setting the final action and sizing, and the
**two-arm verdict** across all of it. The intrinsic verdict is a discriminated
union — a `priced` branch (grade, forward outlook, bidirectional conviction,
portfolio action) and a `role_risk_only` branch for structurally unpriceable
vehicle classes (no letter, targets, lean or conviction; a reduced {sell all,
trim, hold} spine), so no fabricated number rides an unpriceable fund. The
intrinsic verdict stays separated from the portfolio action, which is why
"A-grade business, trim because oversized" is expressible; an allocation
optimizer is **deferred, not adopted**. Designed and unbuilt: the **live
research loop** and the **held-name research refresh lane**.

**Trade Opportunities — designed, not built** (`docs/trade-opportunities.md`,
`trade-opportunities-workflow.md`). Discovery runs through three feeders —
**model-led hypothesis research** (the edge: hypothesis cards + a score gating
promotion *before any ticker*) under an app-owned coverage rotation, stratified
structured feeders (the screener stratifies — stratification IS the breadth
mechanism, no bulk pre-scoring), and a persisted **opportunity-graph watchlist**
refreshing at class cadence. Per-candidate validation runs under an archetype
lens, a mandatory bear case and a leading-metric hard gate. It runs as two jobs
sharing one page (**Discover** / **Audit**, the latter forking Quick/Deep).
Judgment fields carry in the same **two arms** as Portfolio, and **admission is
either-arm** — both arms run the same entry-asymmetry gate and a name clearing
either enters, with both gate vectors persisted — the grant scoped to that gate
alone: the evidence floor, the forensic hard triggers and anchorless `hype` bind
absolutely on both arms. Three classes stay single-valued: facts and their
arithmetic, tier/horizon placement, and the outcome machinery. Deterministic
outcome labels on prior picks **and a shadow scorecard over every name the
funnel turned away** feed a **propose-only, never auto-applied** calibration.
Persistence separates six structures: matrix, opportunity graph,
discovery-coverage ledger, price-tracked departed-pick archive, shadow ledger
and picked-episode store.

## What remains

The queue is governed by the **locked pre-test block** (standing user decision):
no further live runs until the block is fully built, after which one **single big
confirmation run** banks every stacked runtime confirmation at once. **The block
is now fully built.**

### Built

- **The cloud Market Signal Report** — the full 18-step pipeline, the agent
  layer, vector memory and continuity, the run tracker, and whole-corpus data
  portability.
- **The local suite's shared substrate** — the local-model layer and its
  Settings section, Schwab OAuth with holdings and options ingestion, the
  deterministic holdings-snapshot diff, the Portfolio page, and the
  Portfolio-runs history.
- **Portfolio Analysis** — the per-holding spine, the fund path, the thesis
  ledger, the quick check, selective re-analysis, the pre-profit overlay,
  outcome learning, the construction stage, and the two-arm verdict.
- **The calibration tier** — adapter options wiring, the target function, the
  grade bands, and the interpretation-prompt contract, each tuned against the
  first live run's persisted dataset.
- **The pre-run correctness program** — the conformance walks and their ruling
  rounds, the per-doc sweeps, and the long-doc-line cleanup.
- **The progress step-ownership contract** — request events are stamped with
  their owning step at the progress seam's single choke point, and the tracker
  attaches rows by the stamp (the bracket-inference and synthesized-step
  fallbacks are retired; an unowned row renders unattributed, never as a
  failed step). Every FMP suite row carries the shaped ok / empty / malformed
  outcome with its cause on the row.
- **The Stooq-removal slice** — Stooq deleted everywhere (user decision
  2026-08-12; the record, with evidence and inventory, is
  `docs/verification/2026-08-12-stooq-removal-decision.md`): FMP dated-EOD is
  the only deep-price rung for the per-holding history and the outcome pass
  alike, the market benchmark is `^GSPC` (episodes never persisted the old
  `^spx`; the dead `^SPX` cache key is cleaned idempotently at store init),
  data-health's fallback counter is retired — any deep-history failure now
  trips attention — and the benchmark / sector / commodity identity table is
  re-homed to `docs/data-sources.md §Financial Modeling Prep` as FMP symbols.

### Remaining, in order

1. **The single big confirmation run** — the gate everything else waits
   behind, with nothing left standing in front of it. Its checklist is
   `docs/verification/big-run-watch-set.md` (its two retired Stooq lines are
   now the FMP quota-consumption and 429-ladder watches); read `data-health`
   early, since several items resolve off that surface alone. The first attempt
   failed at Step 7b after completing the per-holding pass, and persisted
   nothing — evidence in `docs/verification/2026-08-10-big-run-attempt-1.md`.
   The repair and the pre-run work behind it are fully built: a repeat
   failure at 7b preserves its evidence as a degraded run instead of
   discarding the pass, the named-violation re-run repairs only the violating
   names, and the output budget and adapter seams are instrumented. What
   stays open behind the run (digest compression) is owned by that record's
   §Disposition, not this brief.
2. **Trade Opportunities** — designed, not built, and waiting behind the whole
   block. The design is settled and the paid FMP shapes are live-verified, so
   implementation planning codes against verified shapes. Five hard-trigger
   acceptance cases are parked for this slice and have no other home: a carried
   pick with a deep hard trigger archives with no shadow entry; a name arriving
   identically through all three deep-pass routes; a cheap-pass hard signal
   raises a warning only; a debut hard trigger becomes a shadow rejection; and a
   soft trigger caps the stand-in while preserving conviction, with no forced
   archival.
3. **The two remaining Portfolio depth slices** — the **live research loop** and
   the **held-name research refresh lane**, which rides with it. The shipped
   schemas don't preclude either, but the research loop carries the pre-profit
   producer's activation obligation (§Standing constraints) and must discharge
   it before connecting the producer.

### Owned by no slice

These ride the queue rather than any one slice. They are collected here because
each is unbuilt work that no scheduled slice will pick up on its own.

- **Checkpoint/resume** — docs-promised but unbuilt; the ledger persists only at
  run end. The narrower case of a run whose *construction* fails is now ruled and
  owned by the Step 7b repair, so what stays unowned here is mid-run
  checkpointing proper.
- **The metric-level 6g validator** — only the ledger legs exist. The
  input-delta and what-changed attribution validator remains designed, and now
  also gates the outcome slice's dormant legs (the standing-thesis creation leg
  and the self-correction read).
- **The `hard_forensic_bar` consumer seam** — the construction spine's field is
  producer-dormant *and* consumer-unread (nothing reads it in `feasible_actions`,
  the digest, or validation). When the forensic producer lands, its consumer
  seam needs wiring in the same slice.
- **Cash-residual drawdown** — the joint-feasibility solve never draws the cash
  residual down to fund buys; funded buys enter the implied book as external
  growth. Inert while the fixed preset leaves cash unconstrained, this becomes a
  real asymmetry the moment configurable profiles land, and must be built with
  them.
- **Configurable investor profiles** — user config for the profile preset,
  deferred. The cash-residual drawdown above is gated on it.
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
  `ollama pull` with run-tracker progress, `ollama serve` start, an
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
- **The sidebar's "rated N" wording** — it reads broader than the priced-only
  count it renders.
- **`rate_prints.fetched_at`** — stamped with the run's `created_at` though the
  FRED fetch precedes the per-holding loop; consumed only by a last-resort
  fallback.
- **A reliability rule for model sub-scores and conviction** — both arms record
  them on the episode snapshot, but neither arm's are scored, because
  sub-scores have no per-episode ground truth. Portfolio's scoreboard, the only
  one built, scores the target bands (an interval scorer over each arm's
  entry-vintage values) and the outlook-direction hit-rate; Trade Opportunities
  is designed to the same target-band contract, unbuilt — the outlook-direction
  read is Portfolio-only. Extending either to sub-scores and
  conviction is a calibration-tier question neither job has settled.

### Deferred by decision

Manual CSV/paste import supplements holdings but is not built. The
**sector-aware grade normalization slice** was retired by ruling (2026-08-13,
off attempt 2's letters): the no-A distribution is honest — quality and
valuation sub-scores anticorrelate structurally — so normalization returns
only on realized-outcome evidence, never on a letter distribution. An
**allocation optimizer** is deferred, not adopted. The FINRA
and CBOE evidence legs are designed, unbuilt. Trade Opportunities' blind-first
diagnostic is reserved diagnostic-only, its execution deliberately unspecified
until built. The fund slice's remaining drafted constants — the coverage and US
guards, tier premiums, add floors, and CIK-cache staleness — stay pinned until
the run supplies evidence to move them. The **engine stand-in arm** rides the
same rule: its outlook windows and flat thresholds, the conviction
degradation-count mapping, the action rung rule, and the scoreboard's
outlook-window mapping are all drafted, calibratable, and none yet calibrated
against live evidence.
