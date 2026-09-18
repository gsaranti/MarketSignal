# BUILD — Market Signal

*Architecture brief: the load-bearing decisions and their rationale — the
durable shape future work builds on. The body is as-built unless marked
planned or designed. Construction history lives in git and the verification
records, per-feature specifics live in `docs/`, and the open work list is
named in `CURRENT.md`. §Seams and §Standing constraints are what a plan reads
before it touches the local suite; §What remains is the build queue.*

## What it is

Market Signal is a local-first macOS desktop app (Tauri 2 / Rust backend, Vue 3
frontend) that generates a **Market Signal Report on demand** — a professional,
evolving market *thesis* rather than reactive daily commentary — and, beside
it, a **local analysis suite** that grades the user's own holdings with local
models. A deterministic Rust pipeline gathers market data, macro data and news;
a constrained set of LLM agents reason over a curated packet; the app renders
Markdown to HTML for display and PDF and keeps continuity through vector
memory. Everything runs on the user's machine except external API and model
calls. The 18-step report flow is `docs/report-workflow.md`.

## The spine: the app layer orchestrates; agents are pure stages

The deterministic Rust application layer owns the control flow, all I/O, all
limits and all persistence; agents never touch the network, the database or
the filesystem. Each agent stage is a pure function — structured input →
schema-validated output — behind a Rust trait (`MainAgent`, `AnalystAgent`,
`HeadlineFilter`, `ResearchRouter`), swappable for a deterministic stub. **The
trait methods are synchronous**: the blocking provider call is offloaded via
`spawn_blocking` at the Tauri-command seam, so `tokio` lives only in app-layer
I/O; the Bull/Bear/Balanced trio runs over the same packet on scoped OS
threads, holding the same discipline.

Three consequences:

- **Research planning is the router's job, not the main agent's.** The fixed
  routing model emits the executable plan (Step 8); the app executes it (Step
  9) and assembles the condensed packet deterministically (Step 11). The main
  agent gets no live tool loop.
- **Research execution is hard-bounded in the executor, not the model** — ≤50
  requests, ≤30 minutes, branching depth ≤2, polled against an injectable
  `Clock`; dynamic follow-ups are deterministic delta-rules with thresholds
  normalized to the run's actual elapsed interval, never an assumed week.
- **Failure posture splits by stage role.** The research half is fully
  fail-soft — a thinner packet, never a failed run; only the Step-3 coverage
  floor gates a run — and the analyst layer is deliberately fail-hard.

Why it is load-bearing: this boundary decides the module graph, the testing
strategy (agents are offline-stubbable), the data contracts (the packet and
each output schema are the API between halves) and the safety model (no
unbounded agent I/O).

## Data model & storage

Three stores by responsibility (`docs/storage.md`): the **filesystem** for
canonical Markdown reports (`YYYY-MM-DD-market-signal-report-<id8>.md`) and
the research inbox / archive; **SQLite** for report records, job history,
warning state, baseline snapshots and the vector table — structured blobs as
serde_json text under the load-bearing `float_roundtrip` feature, so carried
numerics compare bit-exactly; and **vector memory** as a `vector_memory` table
in the same database with exact brute-force cosine in Rust, chosen over
LanceDB because at this corpus's scale an unindexed vector DB runs the same
scan while costing an async-only dependency tree against the synchronous
spine. Two seams contain that choice: the `vector_memory` module owns store
access and the `Embedder` trait owns text → vector.

Report-summary metadata is app-owned identity plus model-authored judgment,
with `risk_posture` and `market_cycle` as two orthogonal axes, never one
regime field. The Step-3 baseline packet carries a `gaps` manifest — a series
an adapter cannot resolve rides into the prompt as a tagged `DataGap` rather
than aborting the run — and persists per run so the next run computes a
cadence-honest change view over the actual elapsed interval. **Retention is
asymmetric**: the most recent 30 reports (a delete cascades Markdown, metadata
and the summary vector; HTML is never persisted), but durable learnings
survive report deletion by `kind`; baseline snapshots keep their own cap of
14.

The on-disk home is the Tauri app-data dir keyed by bundle identifier (debug
builds nest under `dev/`, `MARKET_SIGNAL_DATA_DIR` overrides). The macOS
Keychain sits outside that split — app-scoped, shared by debug and release —
and its ACL re-prompt on ad-hoc rebuilds can block first paint. The Light/Dark
preference is the one config in webview `localStorage`, read pre-mount.

**Data portability** (`portability.rs`; `docs/data-portability.md`): a
whole-corpus, versioned, checksummed archive. Durable analytical data moves,
secrets and machine-local state stay behind, it is never a raw DB copy, import
validates everything before its destructive phase, and both directions hold
the run slot. Accepted residue: a mid-import I/O failure can leave partial
files; stage-and-swap is unscheduled.

## Module boundaries

- **`app`** — the pipeline, the bounded executor, gating, warning state,
  baseline persistence and delta computation, and the `progress` seam.
  Determinism lives here.
- **`adapters`** — `data_sources` (FMP / FRED / BLS / CFTC; Tavily, GDELT and
  FMP Articles for news) and `models` (OpenAI + Anthropic); the series catalog
  is `docs/data-sources.md`. Provider tiering is live-verified: FMP's free tier
  gates the dollar index, oil, gas and the calendar, so those moved to FRED and
  the calendar carries names + dates only. Data honesty is the stance — a stale
  or out-of-band value drops to a gap, never a fabricated level. Gated adapters
  share a bounded `Retry-After`-aware backoff parameterized per provider;
  GDELT stays single-shot because its IP lockout makes retrying harmful. Fixed
  internal models (GPT-5 mini headline filtering, Claude Sonnet routing,
  `text-embedding-3-large`) are non-configurable and distinct from the four
  user-selectable agent models; inbox document parsing runs no model.
- **`agents`** — the main agent and the three analysts, run concurrently, plus
  a 16-lens skills library that is **forcing-function-only** (never parsed or
  persisted). Analyst reviews are ephemeral. The main agent is
  **conviction-first**: a directional base case that carries forward and
  pivots only on materially changed evidence (`docs/thesis-continuity.md`);
  `mixed` / `uncertain` is the earned exception, not a safe default.
- **`frontend`** — the report view, the run tracker, the shared-history
  sidebar, research documents, the warning area, Settings and the Portfolio
  page (`docs/interface.md`). markdown-it renders on demand and is never
  persisted; PDF is the webview's print-to-PDF; charts enter only as a
  validated fenced `chart` block. All UI is built against
  `market-signal-design-system/` — the report's reading register and the
  suite's instrument-grade register, bridged by shared chrome — and suite
  sorting / view controls are display-only.

## Runtime, observability & failure posture

Generation is on demand only — no scheduler, timer or tray; closing the app
quits it. A run ends **successful**, **failed** (with a failed-job warning),
**skipped** (a second concurrent run) or **cancelled** (no report, no
warning). Reachability is not a pre-run gate. The **execution gate** requires
all four agent models, both provider tokens and the Tavily / FMP / FRED
credentials; failures surface in the Persistent Warning Area's four
de-duplicating categories, only failed-jobs dismissible, and a dismiss targets
the **rendered** failure identity, never a click-time re-derived one.

Observability rides a **Tauri-free `progress` seam** (`ProgressReporter` plus a
per-run `RunContext` via `with_context`, no trait-signature changes): per-step
progress, one request row per HTTP call stamped with its owning step at the
seam's single choke point, the report token-by-token and thoughts-only
reasoning — all a side-channel that cannot corrupt the report; a debug-gated
thought-log sink is the bounded exception (`docs/run-tracking.md`).
Cancellation is cooperative at step and request boundaries. Two UI
invariants: **a run is never a report** (a cancel or fail removes nothing
that was shown), and `run-finished` is emitted before any job-history write
error can propagate. A corrupt run is a loud skip on its own surface only.

## Testing approach

Agents and adapters are traits, so the orchestrator runs end-to-end against
stubs and fixtures with no keys, covering every limit, retention, gating and
failure-posture contract; each gated adapter has a test-only base-URL seam for
a localhost mock; live smokes are `#[ignore]`d. The frontend gate is two
runners under `npm test` (`CLAUDE.md`). A dev-only **demo-run mode**
(`demo-run` feature, `npm run tauri:demo`) drives the real `run_job` through
the GUI against paced stand-ins.

## Local analysis suite

A second capability set: two on-demand, **local-model-only**, deliberately
**prescriptive** features (grades, actions, targets — a departure from the
report's no-buy/sell stance). **Portfolio Analysis** grades the user's Schwab
holdings and recommends actions + price targets, typing a role/risk read where
a vehicle class is structurally unpriceable. **Trade Opportunities** (designed,
not built) researches new ideas across a 3×3 risk×horizon matrix. A future
**portfolio planner** — the whole-book reasoning the tunnel-vision ruling
moved out of Portfolio Analysis — is the suite's named fourth job, not yet
designed. Full design lives in `docs/local-models.md`, `web-research.md`,
`schwab-integration.md`, `portfolio-analysis.md`, `portfolio-workflow.md` and
`trade-opportunities.md`; this section carries only the decisions a plan must
not work against.

- **A local-only model layer, distinct from the cloud report (built).** A
  flexible local-model adapter calls one **user-installed, app-supervised**
  Ollama daemon over its native `/api/chat`, through the same
  `reqwest::blocking` / `spawn_blocking` seam the cloud agents use — **added
  rather than extending the closed cloud `AgentModel` enum**, so the roster
  changes through configuration rather than code. The app **bundles neither the
  daemon nor the models**. The suite gate holds the report's
  **presence-not-connectivity** posture: *presence* of config gates
  **proactively** (locked Run buttons + a persistent warning) while
  *connectivity* is checked only at the **run-gate** and on a manual Test
  Connection, **never at startup** — a config-set-but-daemon-down state is
  blind on re-open, the deliberate cost of no startup probe. A `LocalEmbedder`
  reuses the `Embedder` trait so `vector_memory` is unchanged. One frontier
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
  fetch/readability-extract layer — **SearXNG-only** (Tavily is reserved for the
  report job, so a local job never spends that quota; a blocked SearXNG degrades
  to thinner research, never a fallback call); the orchestrator runs the tool,
  the model only requests it. SearXNG isn't bundled: the app *ships
  configuration, not the server*. Thin extraction trips a **selective rendered-
  retrieval tier reusing the already-embedded Tauri webview** — not a bundled
  browser or Python sidecar — gated on telemetry so rendering stays
  **measured, never blanket** (the tier is deferred to its own slice; its
  gating telemetry landed). SearXNG sits **off the execution gate**:
  unreachable means a degraded run behind a pre-run notice, never a block. The
  per-item research loop is bounded and SSRF-guarded, every finding keeping its
  source URL + timestamp, and consolidation is one shared **distillation
  primitive** whose mode is chosen deterministically by the full consolidation
  input's size. Optional **Connected Sources** (in-app login → Keychain
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
  option-chain `GET`s, a code-enforced guarantee rather than a token scope,
  bounding a leaked credential's blast radius to in-account trades the app
  never issues. Tokens ride the Keychain and **never enter logs or the run
  tracker**; the loopback's one-shot rustls acceptor rides the stack outbound
  HTTP already uses.
- **Reuses the spine.** Each feature is a new Tauri command + job under the
  **single global run slot** (report + both local jobs are mutually exclusive,
  matching the latest-run-only tracker), reusing the `progress` / run-tracker
  seam and the `vector_memory` / `Embedder` modules. The slot is claimed
  **before any external fetch** — the SEC CIK map loads lazily inside it — so
  the local-only daemon probe is the one pre-slot check. Local-gate failures
  get their own warning categories, kept **off the cloud `validate` gate** — a
  disconnected account blocks only the local jobs, never the report. Both jobs
  are personalized by a **fixed default investor-profile preset** (user config
  deferred) that frames the prescription, never which holdings or ideas qualify
  — nor the intrinsic verdict, whose profile-independence is declared **and
  input-isolation-enforced**: the intrinsic prompt carries no profile, which
  reaches the model at the per-holding action call only.
- **Invariants governing the suite** (full specs in the docs; each states its
  own reach):
  - **Deterministic finance, primary-source evidence** — a shared Rust engine
    over FMP + keyless SEC EDGAR / FINRA / CBOE (CBOE serves venue-level data
    only, the per-stock options read being Schwab chains; FMP dated-EOD is the
    only price rung and `^GSPC` the market benchmark since Stooq's removal)
    computes the engine arm for both jobs. Both are **two-arm**: the engine's
    values are the incorruptible baseline beside an **unrestricted model arm**
    — validated structurally and on each field's declared domain, never
    against the engine — the two scored head-to-head by a deterministic
    scoreboard. The per-job field schemas are enumerated once at
    `docs/local-models.md`. The boundary — **model-arm judgment values never
    alter or bind the engine baseline** — is single-homed per job at
    `docs/portfolio-analysis.md §The holding verdict` and
    `docs/trade-opportunities.md §The opportunity`. Both jobs hold an
    **evidence floor** that returns insufficient evidence over a
    low-conviction guess, each specified in its own doc with its own exit
    semantics.
  - **Anti-reflexivity / no-double-count** — conviction is the model's own, so
    the cap-only since-flagged stance is prompt-side discipline, and the guard
    binds only where it has deterministic consumers: the confirmed-crossing
    validation over each job's own stored conditions, and the cheap
    re-derivation's tripwires. Trade Opportunities adds one rule of its own —
    re-entry is a fresh start, and the archive never promotes itself.
  - **Source quality informs conviction, never gates discovery** — tiers grade;
    only the explicit deny list drops.
  - **Model prompts inform within governed contracts, never add an ungoverned
    conclusion** — governed content is traceable to a canonical project
    contract; app or downstream-consumer architecture, meta-reasoning,
    author-added financial preferences, and weighting or conclusion nudges
    with no canonical contract stay out. Since the 2026-09-17 prompt rewrites
    no Portfolio prompt names an app concept: one message in two parts, data
    with its glosses and then the task, and a placeholder-only return shape.
    Canonical at `docs/local-models.md §Prompt posture`.
  - **Only a deep re-evaluation can archive an opportunity; the cheap
    re-derivation never does** — Trade Opportunities' framing invariant.
    Portfolio's quick check borrows the same warn-don't-decide split, having
    no archive to write to.

**Portfolio Analysis is built in full and is tunnel-vision by contract**
(`portfolio-v9`, ruled 2026-08-14): it never compares holdings — each action
is a rung plus a one-line rationale from the finished verdict, the holding's
own evidence and the investor profile (the profile's only entry point; 6f
interpretation stays profile-blind) — and whole-book questions, sizing and any
optimizer belong to the future portfolio planner. A selective run analyzes
**strictly the selection**, safety triggers surfacing as non-blocking card
badges, never force-includes; a selective request with no readable prior run
runs the whole book. The intrinsic verdict is a **discriminated union** —
`priced`, and `role_risk_only` for structurally unpriceable vehicle classes,
so no fabricated number rides an unpriceable fund — kept separate from the
portfolio action. A hard per-holding model or grade failure **isolates** into
a run-level `failed_holdings` list and a failed card (the prior verdict
carried vintage-stamped where one exists) while the run continues; the run
fails outright only run-level or when every attempted holding fails. This
flips the cloud report's posture for the Portfolio job alone — the analyst
layer stays fail-hard. Deliberate reductions in the **quick check**, surfaced
so they are not mistaken for defects: FMP quote plus dated-EOD only (never the
shared price-bar cache), no cash-flow re-pull, no breadth-flip sub-leg,
material filings are the 10-K / 10-Q / 8-K prefix, and the FINRA sweep leg is
structurally unreachable from the closed ledger series surface.

**Trade Opportunities is designed, not built** (`docs/trade-opportunities.md`,
`trade-opportunities-workflow.md`, and the logic-flow doc whose inline markers
name the constants still to draft). Discovery runs through three feeders —
model-led hypothesis research (hypothesis cards scored *before any ticker*),
stratified structured feeders (stratification is the breadth mechanism), and a
persisted opportunity-graph watchlist — with per-candidate validation under an
archetype lens, a mandatory bear case and a leading-metric hard gate. It runs
as two jobs sharing one page (Discover / Audit) under **one
`trade_opportunities` job identity**, every run record mode-labeled. Judgment
fields carry in the same **two arms** as Portfolio; **admission is
either-arm** (both gate vectors persisted), the evidence floor and forensic
hard triggers binding absolutely on both; placement is **model-authored**,
the engine's derivations staying the baseline and the gate's shared legs.
Deterministic outcome labels plus a shadow scorecard over every turned-away
name feed a **propose-only, never auto-applied** calibration. Its grade slice
(value-creation quality reads, sector-adjusted bands, sector-appropriate
metric selection, the soft-forensic cap) inherits TO's Step-5c shared engine
and lands with TO, never as independent Portfolio work.

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
  frontend mirror `src/etDate.ts` is a separate implementation,
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
  lifecycle; the episode store and the `HoldingAudit.hurdle` snapshot are what
  the calibration-proposal slice will consume.
- The selective machinery's triggering surface: validated ledger conditions
  with eval state and cadence tags, app-stamped monitor bands, the
  acknowledgment transition, per-holding vintages (`effective_vintage`), the
  persisted `action_source` vocabulary and the subset sweep.

BUILD cites version constants rather than duplicating their current values,
so this brief cannot go stale as they move:
`portfolio::PROMPT_VERSION`, `engine::GRADE_PARAMETER_VERSION`,
`engine::SCENARIO_TARGET_PARAMETER_VERSION`,
`pre_profit::PRE_PROFIT_PARAMETER_VERSION`, `engine::EVIDENCE_FLOOR_VERSION`,
`store::CHECKPOINT_FORMAT_VERSION` and `portability::FORMAT_VERSION`.
Persisted records carry the stamp they were written under, so a recalibration
stays attributable and old rows never silently re-grade.
The checkpoint trail resumes on the five `portfolio` / `engine` /
`pre_profit` stamps plus its own `store::CHECKPOINT_FORMAT_VERSION`, the
roster and the prior-run id — never a build identity — so a slice that
changes what a completed holding's verdict or audit means moves the axis it
changed, and one that changes the trail's shape moves the format stamp
(ruled 2026-08-29; canonical at `docs/portfolio-analysis.md §Failure
posture`).

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
  (canonical at `docs/portfolio-workflow.md §Step 6e` and `§Step 6g` and
  `docs/portfolio-analysis.md §The position thesis ledger`).
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
  2026-08-28).
- **The research gathering loop is bounded before every request, never by the
  server's own truncation.** The 6c gathering conversation grows across tool
  turns, so it is sized against the shared interpret input guard before every
  model call and before each retained assistant/tool message — with a per-turn
  tool-call cap and untrusted-metadata caps — and a bound ends gathering as a
  recorded degradation that still takes the separate clean synthesis call;
  that call's evidence packet in turn jointly selects headers and bodies
  within the same guard, reclaiming omitted-header space rather than starving
  every page. The two phases also keep one output protocol per request — tools
  and no grammar while gathering, grammar and no tools/history while
  synthesizing, the synthesis prompt showing the object's shape since the
  grammar never reaches the model (every schema-constrained Portfolio call
  shows a schema-derived shape template, and the synthesis cites sources by
  pass-local id, resolved app-side to the persisted URL) — and the app
  re-validates the grammar-required fields and nonblank prose/claim semantics
  before a pass can complete. Every research gap
  persists on the holding audit and contributes typed counts to the run-level
  data-health summary; the runtime prompt-size read shares the gathering
  message-and-tool serializer.
  This extends the never-rely-on-front-truncation posture from the
  distillation/synthesis prefix to the gathering half — a future edit adding a
  content source to the loop must size it the same way (canonical at
  `docs/web-research.md §The research loop and context management`).
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
  (ruled 2026-08-27; canonical at
  `docs/portfolio-analysis.md §Starting parameters`). The scenario-target
  stamp holds the same rule since `portfolio-v23`:
  `engine::SCENARIO_TARGET_PARAMETER_VERSION` moves only with a row appended
  to `engine::SCENARIO_TARGET_PARAMETER_HISTORY` naming the horizons the bump
  can have moved on each branch — a single `targets-v5` anchor row today, so
  the attribution is dormant until the next bump (ruled 2026-08-29).
- **A condition's continuity stamps are written at authoring.** Step 6g
  writes the prompt's statement basis and, on the two balance-sheet
  instants, the equity source onto every new or superseding quantitative
  condition (`ContinuityStamps`), so the first full-pass evaluation after a
  debut has a stamp to disagree with — "first evaluation adopts" is blind to
  a flip between authoring and that evaluation — and the between-run sweep,
  never the authority on either stamp, evaluates debt/equity only when its
  streak is stamped with the sweep's own FMP-quarterly source, withholding
  another source or none (ruled 2026-08-29; canonical at
  `docs/portfolio-analysis.md` §The position thesis ledger).
- **A price or NAV is usable only when finite and strictly positive.** The
  evidence floor tests usability, never presence — at the FMP quote parse
  (the one seam every quote consumer rides, the quick check's price refresh
  and the commodity quote included), at the dated-EOD parse (both pulls)
  and again at both engine floors, an unusable fund quote
  falling to a usable NAV — and the floor rule is
  stamped (`engine::EVIDENCE_FLOOR_VERSION`) on the checkpoint header and
  every audit record, so a resume never crosses a floor-rule change (ruled
  2026-08-28; canonical at `docs/portfolio-analysis.md §Evidence floor`).
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
  prompt stamp (`portfolio::PROMPT_VERSION`) (ruled 2026-08-28, the semantic
  classifiers of earlier rounds each leaking by ordering). One residual shape
  is closed syntactically — a value that is itself the period, a 1900–2099
  year printed without a thousands separator right after `for / in / of / by
  / through / fiscal / fy`, rejects — while the competing-noun shape stays
  with the persisted excerpt for audit and the stem table's post-run
  calibration, a negative lexicon declined (ruled 2026-08-29; canonical at
  `docs/portfolio-workflow.md §Step 6e`). Each accepted row carries the
  prompt stamp it was admitted under (`admitted_under`), and the history is
  never re-admitted through a later filter — a stricter contract reads the
  stamp to tell old rows apart, never re-filters them (ruled 2026-08-29).
- **No local-suite data compat pre-release.** The dev store is wiped before
  a run, so a new persisted field is required and always written — no serde
  default, no "a record persisted before the field reads as X" branch, no
  archive rung for a format no shipped build wrote; only model-written and
  provider-written JSON stays lenient, and only shipped-report compat and the
  loud-skip stay (ruled 2026-08-29, superseding the 2026-08-17 kept class;
  this brief states the rule, and the refused pre-release archive rungs it
  implies are at `docs/data-portability.md`).

## What remains

The queue is governed by the **pre-run completion bar** (standing user
decision): no further live runs until **the entire Portfolio Analysis job is
built** — every designed leg except work gated on realized-outcome evidence
(grade normalization, the calibration proposals, the derive-reads strata),
which can only come from a run — after which one **single big confirmation
run** banks every stacked runtime confirmation at once.

### Built

The cloud Market Signal Report in full; the local suite's shared substrate
(the local-model layer and its Settings section, Schwab OAuth with holdings
and options ingestion, the deterministic snapshot diff, the Portfolio page and
the runs history); and Portfolio Analysis in full — its calibration tier tuned
against the first live run's persisted dataset and its pre-run correctness
program closed. Trade Opportunities is designed, not built.

### Remaining, in order

1. **The single big confirmation run** — the queue's next item now the
   Portfolio Analysis job is built in full (the pre-run bar is met). The run
   starts from a wiped store, so every holding is a debut and every read
   against a prior is a run-2 watch, a second run following only on the
   user's decision after run 1's result; and the user names the launch
   session at its start. Every attempt to date was user-ended early on
   findings that became fix slices; those findings, the fixes, the pre-run
   checklist and whatever stays open behind the run are tracked in
   `CURRENT.md`, the job docs and the verification records — never in this
   brief, which records features and load-bearing decisions only.
2. **Trade Opportunities** — designed, not built, waiting behind the entire
   Portfolio job and its confirmation run. The design is settled against
   live-verified paid FMP shapes and grounded end-to-end by the logic-flow
   doc and the placement ruling (tier / horizon / runway two-arm; the model's
   authored tier × horizon places the card), every contract single-homed in
   the TO docs. The not-yet-drafted constants (screener floors, archetype
   weight vectors, per-sector factor bands, the commodity-turn threshold, the
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

- **ADR financial-unit normalization** — dated FX conversion plus verified
  ordinary-share-to-ADS ratios and provider per-share conventions, applied
  consistently to statement, consensus, historical and market-price inputs.
  This deferred slice is the path to readmitting depositary receipts under
  `docs/portfolio-analysis.md §Asset eligibility`.
- **Configurable investor profiles** — user config for the profile preset,
  deferred.
- **Paid-FMP baseline enrichment** — three additive report signals the paid key
  unlocks (calendar consensus + surprise, historical valuation percentile/band +
  performance trend, IPO/M&A froth), all engine-derived and outside the
  level-delta engine; true index breadth was ruled out (FMP exposes no breadth
  metric), so the movers group stays the proxy.
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
**sector-aware grade normalization slice** was retired by ruling (2026-08-13):
the no-A distribution is honest — quality and valuation sub-scores
anticorrelate structurally — so normalization returns only on
realized-outcome evidence, never on a letter distribution; that grade slice
lands with Trade Opportunities (§Local analysis suite). An **allocation
optimizer** is deferred, not adopted — sizing and the optimizer question are
the portfolio planner's domain since the tunnel-vision ruling. The
**flat-driver fund target form** is the settled design — a
scenario-differentiated formula returns only on realized-outcome evidence —
and **N-PORT** stays deferred; the CEF leg is detection plus the gap-honest
price-vs-NAV seam. Trade Opportunities' blind-first diagnostic is reserved
diagnostic-only, its execution deliberately unspecified until built. The fund
slice's remaining drafted constants — the coverage and US guards, tier
premiums, add floors, and CIK-cache staleness — stay pinned until the run
supplies evidence to move them. The **engine stand-in arm** rides the same
rule: its outlook windows and flat thresholds, the conviction
degradation-count mapping and the action rung rule are drafted, calibratable,
and none yet calibrated against live evidence. The scoreboard must score each
forecast at its authored horizon; that matching contract is fixed at
`docs/portfolio-analysis.md §Outcome learning`, not a calibration parameter.
