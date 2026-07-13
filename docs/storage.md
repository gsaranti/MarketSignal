# Storage

## Storage Location

All persisted state lives **outside the application bundle**, in the per-user application-data directory resolved from the app's bundle identifier (not its product name):

```text
~/Library/Application Support/com.georgesarantinos.market-signal/
    market_signal.db        the SQLite database (see below)
    reports/                canonical Markdown reports
    research-inbox/         documents awaiting processing
    research-archive/       processed documents
```

Because the location is keyed by the **bundle identifier**, it is stable across versions: rebuilding or replacing the installed app (a new `tauri build`) reads and writes the same store, so existing reports, metadata, and vector memory are preserved across updates.
The bundle never contains data, so replacing the `.app` cannot lose any.

**Development isolation.**
Debug builds (`tauri dev`) nest their store under a `dev/` subdirectory of the path above, so a development session never touches production data; release builds (`tauri build`) use the directory as-is.
The `MARKET_SIGNAL_DATA_DIR` environment variable overrides both — pointing any build at an explicit directory — for tests, automation, and isolated live runs.

## Markdown File Storage

Canonical Markdown reports are stored as files on the local filesystem.
Each file is named with the report date plus an 8-character `report_id` suffix, so a same-date rerun never overwrites an earlier run's file:

```text
YYYY-MM-DD-market-signal-report-<id8>.md
```

That `-<id8>` suffix is the one difference from the **export** filename, which drops it (a same-name export collision is the user's own save-dialog overwrite prompt, not the app's) — see [export.md §Export Naming](export.md#export-naming).

## SQLite

Stores:
- report records
- report metadata
- job history
- warning states
- per-report baseline snapshots (for cross-report change detection)

HTML is deliberately not among the stores (amended 2026-06-12 from the original spec, which kept a stored HTML copy alongside each report): the HTML view is a presentation artifact rendered on demand in the webview from the canonical Markdown, and PDF export prints that same rendered view, so a stored copy would have no reader.
See [report-structure.md §Presentation Format](report-structure.md#presentation-format-html).

Each report stores:
- creation timestamp
- structured report summary metadata
- market regime metadata (risk posture and market cycle)

The market regime metadata holds two labels, each drawn from a fixed vocabulary along a separate axis.

`risk_posture` — the market's risk stance:
- `risk-on`
- `risk-off`
- `mixed`

`market_cycle` — the market's cycle stage:
- `late-cycle`
- `recessionary`
- `recovery`

The main agent selects the label that best fits each axis when synthesizing the report.
Free-form regime commentary belongs in the report Markdown itself (see [report-structure.md](report-structure.md)), not in the labels.

### Report Summary Metadata Schema

The structured report summary metadata is a JSON object stored with each report.
The application stamps the identity fields (`report_id`, `report_type`, `created_at`); the main agent authors the remaining fields when writing the final report.

Required fields:
- `report_id` — UUID for the report.
- `report_type` — always `market_signal` (the report kind; the legacy value `weekly_market` is migrated to it — see [§Legacy Naming Migration](#legacy-naming-migration)).
- `created_at` — ISO-8601 timestamp.
- `title` — a short, specific per-issue headline the main agent writes (e.g. "Rotation, not rupture"), distinct from the constant `report_type` product name.
  Surfaced as the report's label in the UI (the Recent Reports list).
  Stored on the summary with a serde default, so summaries persisted before this field decode with an empty title and the UI falls back to the product name "Market Signal Report".
- `risk_posture` — one of the risk-posture labels above (`risk-on`, `risk-off`, `mixed`).
- `market_cycle` — one of the market-cycle labels above (`late-cycle`, `recessionary`, `recovery`).
- `thesis_stance` — one of: `bullish`, `bearish`, `mixed`, `uncertain`.
- `header_summary_bullets` — array of 3–6 strings, matching the report's `## Header Summary` section.

Optional fields (may be empty arrays):
- `key_risks` — top risks identified in the report.
- `unresolved_questions` — open thesis questions to revisit in subsequent reports.
- `forward_outlook_themes` — themes flagged in the `## Forward Outlook` section.

Detailed analysis remains in the canonical Markdown report; this schema captures only the queryable fields used for cross-report retrieval and continuity.

Only the most recent 30 Market Signal reports are retained.

Older reports are deleted automatically.

When a report is removed:
- its Markdown
- metadata
- associated vector-memory summary references are deleted together.
  (There is no HTML to remove — HTML is rendered on demand, never stored; see [§SQLite](#sqlite).)

### Legacy Naming Migration

The report's stored identifiers were renamed when the product moved from a fixed weekly schedule to on-demand generation.
Reports created under the earlier convention are migrated in place on first launch after the upgrade:

- the `report_type` metadata value `weekly_market` is rewritten to `market_signal`;
- report files named `YYYY-MM-DD-market-signal-weekly-report.md` are renamed to `YYYY-MM-DD-market-signal-report.md`, and any stored file paths are updated to match;
- the `job_runs.job_type` value `weekly_market` is rewritten to `market_signal` — a separate single-column migration covering the job-run history's slug, distinct from the `report_type` rewrite above.

The migration is one-time and idempotent: a report (or job-run row) already carrying the new identifiers is left untouched.
No report content changes — only the type slug, the job-run slug, and the filename.

### Baseline Snapshots

Each report stores a snapshot of the baseline market-data scan that produced it (the Step-3 gather, serialized as JSON).
On the next report, the application diffs the current scan against the most recent prior snapshot to produce a per-report change view — the level moves since the previous report — handed to the main agent so the thesis can ground "what changed" in measured deltas rather than the prior report's prose.

The most recent 14 snapshots are retained, pruned independently of the 30-report report-retention window.
The cadence is report-indexed, not calendar-indexed: because reports can be generated on demand at any time (see [scheduling.md §Generating a Report](scheduling.md#generating-a-report)), the change view reports the actual elapsed interval since the previous report rather than assuming a week.

A missing or unreadable prior snapshot is non-fatal: the report is generated without a change view.
Snapshots are additive context, never a precondition for a report.

The planned paid-tier report enrichment adds fields to the serialized scan — compact derived reads only, each `#[serde(default)]` so an older snapshot still decodes; rules in [data-sources.md §Planned report enrichment](data-sources.md#planned-report-enrichment-paid-fmp-tier).

## Vector Memory

Stores:
- report summaries
- durable learnings
- thesis evolution
- important historical analogs
- past mistakes
- retrospective audit learnings
- useful recurring patterns

The vector store acts as long-term semantic memory for the main agent.

The store is implemented inside the application's SQLite database (a `vector_memory` table holding each item's embedding as bytes) with exact cosine search in Rust — a deliberate engine choice over the originally specified LanceDB (amended 2026-06-11).
At this corpus's scale — at most 30 retained report summaries plus durable learnings — an unindexed vector database performs the same exhaustive scan, with a materially heavier dependency footprint.
Everything else in this section is engine-agnostic and unchanged; the store sits behind a single module so the engine could be swapped if the corpus ever outgrows exact search.

Deleting older reports does not remove durable learnings already stored in vector memory.

This allows the system to preserve long-term analytical continuity even while older report files are removed from local storage.

### Embeddings

Embeddings are generated with OpenAI `text-embedding-3-large`, using the configured OpenAI API token (see [configuration.md §API Tokens](configuration.md#api-tokens)).

Each item is embedded as a single atomic unit:
- one embedding per report summary
- one embedding per durable learning

Report Markdown is not split into fixed-size or section-based chunks for vector memory; the report-summary metadata is the unit that enters vector memory.

## Local Analysis Suite Storage

The local analysis suite (see [local-models.md](local-models.md)) persists its own runs, separately from report storage.
Each feature stores its run history in the SQLite database:

- **Portfolio Analysis** — per run, the per-holding verdicts (grade and sub-scores, conviction, horizon outlook, price targets — or a fund's typed **`role_risk_only`** assessment where the vehicle class is unpriceable ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)) — financial-analysis summary, the **portfolio action** with its target-weight range and share/dollar adjustment, and the what-changed audit — split into an intrinsic and an action half — with its per-value cause attribution), the per-holding **thesis ledger** (the standing thesis, the bear/base/bull monitor, key falsifiers, and add/trim/sell triggers with a target-weight range — **typed by verdict branch**: a `role_risk_only` ledger's monitor scenarios are condition-only and its triggers trim / sell only — the Portfolio analog of Trade Opportunities' opportunity graph, carried forward to seed the next run's continuity check; see [portfolio-analysis.md §The position thesis ledger](portfolio-analysis.md#the-position-thesis-ledger)), and the portfolio roll-up.
  Each holding also carries its **attention-flag state** (the quick check's non-destructive flag plus which trigger raised it, cleared by the next full pass over the holding, plus any quiet **unexamined-evidence-event** note — [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)), each ledger condition's **evaluation state** (engine state, distinct from the model-authored ledger content, keyed by the condition's stable app-assigned **`condition_id`** — fields and the structural-identity / supersession rule in [portfolio-analysis.md §The position thesis ledger](portfolio-analysis.md#the-position-thesis-ledger)) and, after a selective re-analysis, its **analysis vintage** (the run that last re-analyzed it — [portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering)).
  **An outcome-episode store** (calibration-only, the Portfolio counterpart of Trade Opportunities' shadow ledger) persists **decision episodes**, typed by verdict branch with an explicit schema per branch, each carrying its **`action_source`** marker, anchor date, **intrinsic vintage**, **entry-stamped sector identity** (the sector label at the anchor run + the resolved SPDR benchmark symbol — the relative labels' benchmark carrier after the holding exits; a holding with no valid mapping carries the typed `sector-unscorable` state — [trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)), and the next run's **`observed_net_alignment`** tag — the creation / extension semantics, the per-branch field sets, and the priced episode's **calibration-feature snapshot** are canonical in [portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration) — persisting **independent of the 10-run retention** (a 12-month outcome window can outlive it), each episode **frozen into a compact matured archive** once its 12-month labels record (row-level history kept for proposal re-testing, under its own cap).
  The most recent standalone **Pull holdings** snapshot is also stored (with its pulled-at timestamp) so the portfolio is viewable without re-fetching — a **view-only** store, distinct from the holdings snapshot persisted *inside* each run: the run's snapshot is the diff baseline and the audit record's basis, while the standalone pull never feeds the job ([portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering), [schwab-integration.md](schwab-integration.md)).
- **Trade Opportunities** — per run, the 3×3 matrix of opportunities (each with its directional thesis, leading operating metric — plus the candidate's **stored leading-metric series**, the dated observation history the engine evaluates, including the research-extracted, app-appended quarterly segment observations ([data-sources.md §Trade Opportunities — endpoint surface](data-sources.md#trade-opportunities--endpoint-surface)) — the typed catalyst claim (`{ description, date (optional), payoff_bearing }` — [trade-opportunities.md §The opportunity](trade-opportunities.md#the-opportunity)), horizon (with its rule-derived `expected_thesis_realization` basis — `dated-catalyst` / `recognition` / `multi-year-compounding` — the engine's `business_runway` durability read, and the validated **`runway_evidence`** inputs the derivation read, persisted beside the result for continuity audit), risk tier, conviction, narrative-vs-reality read, bear case, **key falsifiers** (each typed by re-check class — `structured` / `filing` / `research` — so the cheap re-derivation can evaluate the machine-checkable ones, each machine-checkable condition carrying its observation-identity-keyed **evaluation state**, keyed across rewrites by its stable app-assigned **`condition_id`** — fields and the structural-identity / supersession rule in [trade-opportunities.md §The opportunity](trade-opportunities.md#the-opportunity)), **hypothesis lineage**, the typed **`technology_read`** when the name was surfaced by the event-impact repricing lens, entry consideration, any risk / forensic flags (a tripped hard trigger's typed `forensic_event` record beside them — [trade-opportunities-workflow.md §Step 5c](trade-opportunities-workflow.md#step-5c-deterministic-analysis-archetype-weighted-engine)), carry-forward status, the **attention-warning state** (the cheap re-derivation's non-destructive flag plus which trigger raised it — tripwire / upside-exhaustion / re-surfacing — cleared by the next deep pass; [trade-opportunities.md §The opportunity](trade-opportunities.md#the-opportunity)), the **`last_deep_researched_at`** date (driving the *Deep-researched today* affordance and the next deep pass's continuity weight), its **entry-stamped sector identity** (sector label + resolved SPDR benchmark symbol, refreshed at each deep pass — the since-flagged read's benchmark carrier — [trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)), and — for a carried-forward idea — the engine-attached **since-flagged performance**: running return since it became an opportunity (`became_opportunity_at`; absolute and vs sector / market), maximum drawdown, and leading-metric continuation, with the matured-window 1/3/6/12-month labels attaching as they elapse), **plus the persisted opportunity graph — the discovery memory** (hypotheses with their economic value-chain traces, and the **watchlist** of worthy-but-unpicked names, each carrying its hypothesis lineage (with any **seed lineage** — the structured-feed headlines that surfaced or oriented it where it was feed-seeded, recorded as leads, never scored as evidence — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)), **hypothesis score**, leading metric (with its **re-check class**), falsifiers, status [picked / watchlist / retired / departed — the terminal tombstone a picked node takes when its opportunity is deeply invalidated and archived, [trade-opportunities.md §Discovery memory](trade-opportunities.md#discovery-memory-the-opportunity-graph)], and timestamps; an **event-impact hypothesis additionally persists its typed `technology_read`** — substitute / complement / mix-shift, exposed revenue / profit pool, deployment timeline, switching costs, margin-capturing node — **and the affected side** [beneficiary / feared-loser / latent], so a sized panic-vs-real read carries forward and is re-checkable like any other watchlist evidence — [trade-opportunities.md §The event-impact / value-chain repricing lens](trade-opportunities.md#the-event-impact--value-chain-repricing-lens)).
  The graph is **distinct from the matrix**: it is *upstream* discovery memory carried forward so a deferred-but-compounding name is re-checked and not silently lost ([trade-opportunities.md §Discovery memory](trade-opportunities.md#discovery-memory-the-opportunity-graph)), and its **watchlist and `departed` populations are bounded** — a watchlist retention cap (its bind eviction deterministic: lowest hypothesis score first, retired `capacity-evicted` — [trade-opportunities-workflow.md §Step 3c](trade-opportunities-workflow.md#step-3c-carried-forward-watchlist-re-check)) plus self-pruning of falsified / stale / horizon-elapsed nodes, with `departed` pick tombstones pruned on the archive's retention ([configuration.md §Local Analysis Suite Configuration](configuration.md#local-analysis-suite-configuration)) — while **live `picked` nodes carry no cap of their own**, mirroring the deliberately uncapped matrix lifecycle ([trade-opportunities.md §Discovery memory](trade-opportunities.md#discovery-memory-the-opportunity-graph)).
  **An archive of departed picks** (a third store, *downstream* of the matrix and distinct from both the matrix and the graph) persists the most recent **100** opportunities a failed re-evaluation removed from the matrix (the single `failed-reevaluation` trigger → `invalidated` status, set **only by a deep re-evaluation** — the cheap re-derivation never archives) — each a **frozen verdict snapshot** (the descriptive record — thesis, archetype, leading metric, catalyst, bear case — plus conviction-at-exit, the stamped sector identity its per-run vs-sector refresh reads, `became_opportunity_at`, the departure date, and the failing signal), pruned oldest-first.
  No forward prediction and no since-flagged numbers are stored: the **since-flagged read is recomputed statelessly each run** from price history (the same reconstruction the matrix uses), so an archived pick needs no per-run price snapshot, and re-discovery simply removes the row ([trade-opportunities.md §Archived opportunities](trade-opportunities.md#archived-opportunities)).
  **A shadow outcome ledger** (a fourth, calibration-only structure — never a discovery input) persists every name the funnel affirmatively turned away as **typed decision episodes** — one per ticker per turn-away, classed gate-reject / abstention / deferral / dedup-substitute / retired-hypothesis (a capacity eviction is a retirement-class episode carrying its persisted `capacity-evicted` reason — [trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)), a Step-5h **`gate-reject`** carrying the **full gate vector with per-gate distance-to-threshold** (not just the first failing gate; the other holdouts carry their turn-away reasons — named floor gaps, tripped trigger — never a fabricated vector, per-class content at [trade-opportunities-workflow.md §Step 5h](trade-opportunities-workflow.md#step-5h-deterministic-risk-tier-gate-validation--checkpoint)) — each with its anchor date(s) (a retirement carries both its first-surfaced and retirement anchors), surfacing / feeder lineage, archetype, and **entry-stamped sector identity** (or the typed `sector-unscorable` state — [trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)), so the outcome pass can compute the **price-only, per-class** picked-vs-rejected spread (unique-issuer counted) and tradability-discounted false-negative flags ([trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)); it is bounded by its own retention cap ([configuration.md §Local Analysis Suite Configuration](configuration.md#local-analysis-suite-configuration)) with entries frozen into a compact **matured archive** once their 12-month labels record (row-level history kept for gate-proposal re-testing, under its own cap), and a shadow name re-enters analysis only through independent fresh discovery.
  **A picked-episode store** (the fifth structure — the picks' counterpart of the shadow ledger) persists one immutable **picked decision episode** per lifecycle — opened at the `became_opportunity_at` run, keyed by a durable app-assigned **lifecycle id**, carrying **(i)** its **entry calibration snapshot** — the decision-time inputs the `resolution_mode` tree re-tests after every other store may have pruned: the entry-vintage base-case target with its methodology and the tree's parameter version, the entry valuation and estimate-revision baseline, the stable falsifier `condition_id`s with their entry states, the **entry-stamped sector identity** (sector label + resolved SPDR benchmark symbol), and the initial forensic state (the Trade Opportunities counterpart of the Portfolio episode's calibration-feature snapshot above) — **(ii)** the **dated branch-relevant events accumulated while the pick is live** (falsifier confirmations, forensic trips, metric state changes, each recorded onto the episode in the pass that observes it), and **(iii)** the matured-window outcome labels as they record (a metric label with no legal refresh path recording the typed **`leading-metric-unscorable`** state; a window whose still-relevant branches were not all evaluable — the live metric-unscorable case included, post-departure with no recorded terminal fact the limiting one — recording the typed **`resolution-unscorable`** mode — [trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)) — **independent of matrix presence, the archive's 100-cap, and run retention**; re-entry opens a new lifecycle id while the old episode matures, and episodes freeze into the same compact matured-archive treatment (row-level under the drafted **picked matured-archive cap** — [trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable)).
- **Run audit record** — each run also stores what it was based on: the holdings snapshot (portfolio), the report(s) and sources used with retrieval timestamps and their app-computed source-quality annotations (evidence tier / extraction quality / recency — [web-research.md §Source quality and evidence weighting](web-research.md#source-quality-and-evidence-weighting)), the distilled findings (carrying **seed lineage** wherever a finding or hypothesis came from a structured-feed-seeded loop — which headlines surfaced or oriented it, as leads not evidence — [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management); a finding from an unseeded loop has none), the **Trade Opportunities discovery and screening inputs** (which screens, routes, and themes surfaced each candidate), the computed financial metrics and the derived reads, the per-item conviction decomposition — per holding where the verdict branch carries one, per Trade Opportunities pick — (`base_conviction` + any `conviction_raise` with its cited `validated_leading_indicator` + the app-computed `final_conviction`, plus the **matched conviction-cap rule** where a tripped soft or hard trigger clamped the value (the shared soft ceiling — [trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable); Portfolio's hard-Low outcome — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)), so an honored-or-dropped raise — and a ceiling-clamped value — is reconstructable — [portfolio-workflow.md §Step 6g](portfolio-workflow.md#step-6g-continuity-check-and-checkpoint), [trade-opportunities-workflow.md §Step 5h](trade-opportunities-workflow.md#step-5h-deterministic-risk-tier-gate-validation--checkpoint)), the input delta and the per-value what-changed / carry-forward attribution, the **dedup-collapse decisions** behind the Trade Opportunities matrix (each merged-away candidate, the peer it merged into, the reason, the **equivalence-predicate inputs and validator result** — accepted, rejected-and-listed-both, or, where the target was a cheap-swept carry, **provisional → confirmed / reinstated**, Step 7's final-cell re-check persisting its own inputs and result beside Step 6's ([trade-opportunities-workflow.md §Step 7](trade-opportunities-workflow.md#step-7-continuity-check--carry-forward)) — and the collapse **direction** — debut-into-carry vs debut-into-debut, a live carry never collapsing away ([trade-opportunities-workflow.md §Step 6](trade-opportunities-workflow.md#step-6-matrix-assembly--completeness-validation)) — so the completeness guarantee is auditable), **the Trade Opportunities outcome labels on prior picks** (the matured-window labels — forward return vs sector / market, drawdown, leading-metric continuation, the all-outcome `resolution_mode` — plus each carried-forward idea's continuous since-flagged read) **and the shadow scorecard** (the turned-away ledger's matured price-only labels, the picked-vs-rejected spread, and any false-negative flags), **the Portfolio Analysis outcome records** (this run's appended decision episodes, newly matured 1/3/6/12-month labels, and the derived action-cohort / target-calibration / falsifier-lead-time / self-correction reads — [portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)) **and each holding's research-reuse decision** (refreshed vs carried-from-cache, with its vintage) **and each Trade Opportunities pass's reused-vs-freshly-fetched document split** (the document-level cache's audit leg — [trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture)), the price-target methodology (including its discount-rate assumption, the stored **anchor-window spread percentiles and drivers** the engine-only quick paths later re-anchor against ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)), the **run-time FMP `quote` price the targets were computed from** — a transient job-time input logged for traceability, **not** a persisted current-price field; displayed price reads reconstruct from Stooq instead ([trade-opportunities.md §Storage and display](trade-opportunities.md#storage-and-display)) — and any research-sourced forward assumption with its source, its declared `conflict_handling`, and — where it conflicted with a structured feed — the policy rule the engine matched or the failed condition that rejected it ([portfolio-workflow.md §Step 6e](portfolio-workflow.md#step-6e-deterministic-target-refinement))), the model ids and quantizations, the prompt/schema version, and any degraded-input flags, each per-holding field recorded where the holding's verdict branch carries it (a `role_risk_only` holding has no conviction decomposition, implied-expectations range, dead-money read, or target methodology to record — [portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)) — so a run is traceable and reviewable (URLs, timestamps, distilled findings, and computed metrics, not full page snapshots), and the next run can audit it.
- **Web-research source state (shared across both local features, not a per-run store)** — a learned, persisted layer the fetch loop accumulates across runs: each domain's resolved **`extractionProfile`** (`api_or_html` / `html` / `js_required`) and the **extraction telemetry** behind it (per-domain full-text-vs-thin-stub recovery counts — [web-research.md §Source quality and evidence weighting](web-research.md#source-quality-and-evidence-weighting)), a derived **render-first flag** (a domain repeatedly thin to a plain GET skips straight to the WKWebView render tier next time, sparing a wasted GET — [web-research.md §Fetch and extraction](web-research.md#fetch-and-extraction)), and each **Connected Source's health state** (`connected` / `connected_but_thin` / `expired` / `unsupported`).
  This is **shared infrastructure, deliberately not job-partitioned** — extraction behavior is a property of the domain, not of a job's learnings, so it sits outside the per-job vector partitions below.
  It is a **thin learned layer over heuristic defaults** (an unseen domain just uses the default profile), parallel to the registry: the registry *defaults* are seed config and the user's **registry overrides** live in the settings store ([configuration.md §Web Research](configuration.md#web-research)), while a Connected Source's **session credential stays in the macOS Keychain, never SQLite** ([§SQLite](#sqlite); [configuration.md §Connected Sources (subscriptions)](configuration.md#connected-sources-subscriptions)).
- **Stooq price-bar cache (shared across both local features, not a per-run store)** — the daily OHLC bars the render-time since-flagged floor and the engine read, cached so the matrix display needn't re-fetch on every page open ([trade-opportunities.md §Storage and display](trade-opportunities.md#storage-and-display)).
  Like the web-research source state above it is **shared infrastructure, deliberately not job-partitioned** — price history is a property of the **symbol**, not a job's learnings — keyed by symbol, holding the cached daily bars plus each symbol's **`last_requested_at`** (UTC) and **latest-bar as-of date**.
  The render-time floor recomputes return / drawdown locally from this cache and re-requests a symbol's bars only **after 8 PM ET (DST-aware `America/New_York`) and not within the prior 24 hours**, fail-soft (a failed refresh keeps the cached series with its as-of date; the 8 PM hour and the 24-hour window are calibratable).
The **archive-price refresh holds a job-time rule of its own**: each Trade Opportunities run requests every distinct archived symbol through this same cache — deduped against the swept and label-time populations, the endpoint surface's third maintenance population ([data-sources.md §Trade Opportunities — endpoint surface](data-sources.md#trade-opportunities--endpoint-surface)) — so an archived pick's since-flagged read is current as of each run whether or not the archive view is opened; the render-time lazy rule above covers page opens between runs.
  **Both jobs' outcome passes hold a separate, stricter label-time rule**: before maturing a window the pass requests that episode symbol — **picked or shadow alike** — through the same cache until the series covers the window end, regardless of whether the symbol is still held / live or was recently requested at render; a failed refresh leaves the label pending unless the cache already has that coverage ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration); [trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)).
  The pending state is itself **bounded by the shared price-coverage grace** (drafted **~3 months past the window end**, the constant homed in [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)): a price leg still pending past it closes as the typed **`price-coverage-unscorable`** label — excluded from spreads and calibration denominators like the other typed exclusions, **counted and logged** — so the episode's 12-month labels record and the row freezes into its matured archive rather than pending forever.
  Past the same bound a previously covered series that stopped resolves per the terminal contract (an unrecorded disappearance conservatively `terminal-unscorable`), while a symbol the source never covered takes the price-coverage state — the grace doubling as the transient-vs-disappearance discriminator.
  The cache backs the **price-only** display and outcome-label reads; the live **FMP `quote`** the engine's gate/target math uses at job time is **not** cached here — it is a transient job-time input, logged in the run audit record.
- **Web-research document cache (shared across both local features, not a per-run store)** — the cross-run per-topic research cache both jobs' failure postures name: fetched, readability-extracted documents keyed by **normalized URL**, each carrying its **original retrieval timestamp** — the immutable evidence vintage, never rewritten on reuse — and reusable within the shared **~4-week** freshness window (entries age out past it).
  For Trade Opportunities the cache is **document-level by construction** — reuse never substitutes for a live search, and the floor-bearing freshness reads must be met from currently searched results ([trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture)); Portfolio's higher-level **distilled-findings reuse** is a separate, job-partitioned layer — each holding's cached distilled findings persist with its Portfolio records under the reuse contract at [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable) — over the same underlying fetches.
  Shared infrastructure like the stores above — a fetched document is a property of the URL, not a job's learnings.
- **Factor-distribution store (shared across both local features, not a per-run store)** — the accumulated per-factor observations (winsorized factor values bucketed by cap-band × sector) that ride beside the Trade Opportunities quant composite as **diagnostic context only — never a score input** ([trade-opportunities-workflow.md §Step 5c](trade-opportunities-workflow.md#step-5c-deterministic-analysis-archetype-weighted-engine)) — the score's basis is sector-adjusted absolute bands + the company's own history; a selected sample of what the app happened to analyze is never a market percentile at any weight, so the store graduates into the score only when fed by the representative-universe snapshot.
  **One current observation per issuer per factor** — a re-analysis replaces, never appends, so frequently revisited names can't overweight the distribution — contributed by both jobs' engine passes (one shared engine).
  Shared infrastructure like the two stores above — a factor observation is a property of the market cross-section, not a job's learnings — and bounded: observations are time-stamped and age out (drafted: ~24 months, so the basis stays regime-current), with a per-bucket **unique-issuer floor** below which the bands stand alone and the composite flags low confidence ([trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable)).
  A periodic stratified snapshot of a representative universe is the named upgrade path (calibration-tier, unscheduled).

Each feature retains its most recent N runs, pruned independently of the 30-report report-retention window and of each other (the same additive-history pattern as baseline snapshots).
The Schwab app secret and OAuth tokens are the one exception to SQLite credential storage — they live in the macOS Keychain (see [schwab-integration.md](schwab-integration.md), [configuration.md](configuration.md)).
Each durable store above joins the whole-corpus portability archive with the slice that lands it — a required manifest entry plus a format-version bump ([data-portability.md §Build-order placement](data-portability.md#build-order-placement)).

### Local Vector Memory

Both features use vector memory for run-to-run continuity, through the same `vector_memory` table and module as the report.
Three rules keep the spaces separate:

- **Per-job partitions, with kind and lifecycle inside them.**
  Each memory row carries an explicit **job namespace** (report / portfolio / opportunities) — a partition dimension distinct from the entry kind (summary / learning) the report already uses.
  The report, Portfolio Analysis, and Trade Opportunities each write and read **only their own** namespace; no job retrieves another's learnings.
  Holding-grading calibration and opportunity-discovery context are job-specific, so cross-job recall would be noise.
  Within a local namespace the **entry kind is the purpose boundary**: continuity record-summaries persist as `summary` rows — the only kind a dossier recall may match — while outcome labels and false-negative flags persist as durable `learning` rows, consumed only by the calibration passes ([trade-opportunities-workflow.md §Step 5b](trade-opportunities-workflow.md#step-5b-dossier-assembly), [§Step 9](trade-opportunities-workflow.md#step-9-persist-run-and-audit-with-memory-embeddings)).
  **Trade Opportunities rows additionally persist the pick's lifecycle id**, and the dossier-recall predicate filters on it, so a re-entered ticker's fresh lifecycle can never retrieve the departed lifecycle's records (the vector-side leg of the no-influence rule — [trade-opportunities.md §Archived opportunities](trade-opportunities.md#archived-opportunities)).
- **A distinct embedder, keyed by identity.**
  The local features embed with a local model (see [local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)), not OpenAI `text-embedding-3-large`.
  The report's vectors are therefore a different dimensionality and cannot be compared against the local ones; the two local features share an embedder, so their isolation is enforced by partition, not dimensionality.
  **Embedder identity — never dimension — is the compatibility key**: the local embedder model is user-configurable and two different models can share a dimension, so when the configured embedder changes the app **re-embeds the local namespaces from their retained `content`** (or clears the stale cohort where content is absent) rather than ever cosine-comparing vectors from different models — the same identity discipline the portability import applies against the manifest's per-namespace embedder id ([data-portability.md §Vector memory is embedder-bound](data-portability.md#vector-memory-is-embedder-bound)).
- **Own retention, with the report's durable-learning carve-out.**
  A feature's **run-summary rows** follow that feature's run retention, independent of the 30-report cascade.
  Its **durable-learning rows do not**: they are tied to the episode stores that generated them (the Portfolio outcome episodes; the Trade Opportunities picked and shadow episodes), so they survive run pruning exactly as the report namespace's learnings survive report deletion — guaranteed by the row's `kind` — and are pruned only alongside their matured archive's own cap.

The store stays exact cosine search in Rust, unchanged (see [§Vector Memory](#vector-memory)).
