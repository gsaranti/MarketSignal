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

Only the 30 most recently **generated** Market Signal reports are retained — the keep window is insertion order, not the reports' own dates, so a clock-stepped report cannot be evicted by the very run that wrote it.
The sidebar's list stays date-ordered (a report is a dated document), so the two windows disagree about **which** 30, never about how many: a clock-stepped report is preserved by evicting the oldest *insertion* instead, and the sidebar then lists all 30 with that report **last**, since its own date is the oldest.
Insertion order is a local database artifact and is **not** carried by the portability archive, which orders reports by date on export and reinserts them in that order — so after an import the keep window follows the archive's date order, and a clock-stepped report that survived on the source machine can be evicted there ([data-portability.md §Import flow](data-portability.md#import-flow)).
What the rule guarantees is narrower than an equivalence between the two windows, and is unaffected by an import: a run can never evict the report **it just wrote**, because that report always takes the highest insertion order.

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

- **Portfolio Analysis** — per run, the per-holding verdicts as **two arms**: the engine arm — grade and sub-scores, the bear / base / bull bands at three months, twelve months and three years with their method clauses, the risk tier, the hurdle read, the hard-forensic state and the engine's own action rung — and the model arm — the **thesis document**, the typed appendix's conviction and three expected prices — or a fund's typed **`role_risk_only`** record where the vehicle class is unpriceable: its engine reads and a thesis document carrying the role read, with no prices ([portfolio-analysis.md §Asset eligibility](portfolio-analysis.md#asset-eligibility)); the **rung-only portfolio action** with the action call's rationale, its `action_source` and its `action_annotations` ([portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action)); and the holdings snapshot the run ran against.
  Beside the run store sits the **interrupted-run checkpoint trail** — a pinned header plus one row per completed holding, each row carrying the holding's verdict, action and audit record so a resume restores its documents whole — transient machine-local state outside the portability archive; the contract is canonical at [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).
  Each run also stores the portfolio roll-up — the deterministic counts and reads plus the data-health aggregate, including the completed audits' research-degraded holding and total-gap counts ([portfolio-analysis.md §Portfolio roll-up](portfolio-analysis.md#portfolio-roll-up)), and a chosen rung outside the engine's per-holding set is annotated on the holding's audit instead (`action_annotations`).
  A priced stock's quick-check basis stores both the rolling NTM consensus mid used for valuation and the raw fiscal-period EPS mids with their authoring-time weights; revision consumers match the latter by period end so a changing NTM calendar weight cannot masquerade as evidence.
  A run row is `run_id` / `created_at` / `run_json` only — an unparseable blob is skipped from `latest_run` and every baseline read but still **lists as unreadable** (identity from its SQL columns), so a corrupt row never blanks the history or silently becomes a baseline ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).
  The write itself refuses a record that would not read back, naming the holding where the value sits in a per-holding record — canonical at [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).
  A store-level read error on the prior run or the quick-check state degrades the same way (the run proceeds as a first run) and is logged; the quick-check state read skips an unparseable row loudly like `latest_run`.
  The attention flag itself (which monitor raised it, cleared by the next full pass over the holding) and any quiet **unexamined-evidence-event** note live **only** in the single-row quick-check store below — the run record holds no copy, and nothing overlays onto a carried verdict ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
  After a selective re-analysis a carried verdict also carries its **analysis vintage**, the run that last re-analyzed it ([portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering)).
  **Every priced stock** persists its **pre-profit overlay record** — the three-state eligibility read with any missing-input gaps (a not-entered stock's record names which arm was uncomputable; still recorded on an insufficient-evidence exit — a fresh engine read at the engine-stage floor exit, eligibility-unscorable at the guard-routed exit, which fetches no statements — [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable), [§Evidence floor](portfolio-analysis.md#evidence-floor)), the statement-derived runway / margin / capex / dilution inputs, and the engine-derived financing, economics, dilution and severe-deterioration states with their matched action rules; the execution leg persists as `unscorable`, having no producer.
  **The episode store** persists the accuracy pass's **price records**: per episode the symbol, the creation date, the spot that day and the anchor close with its bar date, and the model's expected share price and the engine's base value at each of the three horizons; written onto each once, its **checks** — horizon, check date, the close read, and both arms' expected price and per-check score — or its unscorable states with their cause.
  The store is append-only — an episode is never updated or deleted — and persists **independent of the 30-run retention**, since a three-year horizon outlives any count-based window ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration)).
  Each holding's **accuracy scores** — per horizon, per arm, each with the date of the last check that moved it — persist with the holding.
  **A single-row quick-check store** holds the between-run sweep's own state — its evaluation-parameter stamp, the per-holding family sweeps with their typed `fresh_clear` / `flagged` / `unknown` reads, each flag's raising monitor, the rate-print cache it re-anchored against, and the run it swept — deliberately **never** `portfolio_runs`, so a sweep can neither enter the run history nor become the next full run's `latest_run` or diff baseline ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
  On an evaluation-stamp boundary, the reader retires only the flags whose measurement semantics changed; unrelated durable flags and events survive.
  Only the latest row is kept: a sweep supersedes its predecessor wholesale, and a newer full run supersedes the sweep.
  It is durable analytical state rather than a regenerable cache — its flags do not come back on the next sweep — so it joins the portability archive ([data-portability.md §The archive](data-portability.md#the-archive)).
  The most recent standalone **Pull holdings** snapshot is also stored (with its pulled-at timestamp) so the portfolio is viewable without re-fetching — a **view-only** store, distinct from the holdings snapshot persisted *inside* each run: the run's snapshot is the diff baseline and the audit record's basis, while the standalone pull never feeds the job ([portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering), [schwab-integration.md](schwab-integration.md)).
- **Trade Opportunities** — per run, the 3×3 matrix of opportunities with its run audit record, **plus four carried stores — the opportunity graph, the discovery-coverage ledger, the archive and the episode store — five persisted structures** ([trade-opportunities-workflow.md §Step 9](trade-opportunities-workflow.md#step-9-persist-run-and-audit)).
  Each run record carries its **mode** (`discover` / `audit-quick` / `audit-deep`) under the feature's one `trade_opportunities` job identity, so history and retention read a single mode-labeled pool.
  Each opportunity is a **two-arm record** ([trade-opportunities.md §The opportunity](trade-opportunities.md#the-opportunity)): its ticker and app-assigned **lifecycle id**; the archetype, with the prior label beside the current one on a carried name; the detection mode; the **thesis document**, persisted as written and validated by nothing; the leading operating metric with its **re-check class** (`structured` / `filing` / `research`); **both arms' risk tier and horizon** — the model's pair placing the card, the engine's rule-derived pair beside it with the horizon's derived basis (`transaction-close` / `multi-year-compounding` / `default`); the model's conviction; the model's **expected price at three months, twelve months and three years** beside the engine's bear / base / bull bands at the same horizons with their `TargetMeta` provenance flags; the engine's narrative-vs-reality and implied-expectations reads, its composite and sub-scores with their provenance flags, and its soft forensic annotations; the hypothesis lineage; the **admission provenance** (`admitted_by`); the carry-forward status; the **attention-warning state** (the cheap re-derivation's non-destructive flag plus which trigger raised it — tripwire / upside-exhaustion / re-surfacing — cleared by the next floor-clearing deep pass); `became_opportunity_at` and `last_deep_researched_at`; the **stamped sector identity** (sector label + resolved SPDR benchmark symbol — the live record's copy, refreshed at each deep pass, the since-flagged read's benchmark carrier); for a carried-forward idea the engine-attached **since-flagged read** (running return since `became_opportunity_at`, absolute and vs sector / market, maximum drawdown, and the engine metric family's continuation state); and the lifecycle's **accuracy scores** for both arms ([trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)).
  The **opportunity graph** holds **hypothesis nodes** — each a hypothesis document's section as the model wrote it, with the typed fields its appendix carried: the title, the decision, the candidate symbols, the leading metric with its class, and for an event-impact hypothesis each name's side and the gate fields — and **company nodes** with a status [picked / watchlist / retired / departed — the terminal tombstone a picked node takes when its opportunity is deeply invalidated and archived, [trade-opportunities.md §Discovery memory](trade-opportunities.md#discovery-memory-the-opportunity-graph)], the document the watchlist review reads — the hypothesis section the node expresses or, for an ordinary gate-rejected debut, its thesis document carried as the node's own text ([trade-opportunities-workflow.md §Step 7](trade-opportunities-workflow.md#step-7-continuity-check--carry-forward)) — the leading metric by name with its re-check class, the hypothesis link where one exists, the last successful refresh timestamp with the watchlist review's own date recorded beside it, and the latest **refresh write-up** with its vintage where the research-watchlist lane has produced one ([trade-opportunities-workflow.md §Step 3c](trade-opportunities-workflow.md#step-3c-carried-forward-watchlist-re-check)).
  The graph is **distinct from the matrix**: it is *upstream* discovery memory carried forward so a deferred-but-compounding name is re-read by the watchlist review and not silently lost.
  Its **watchlist and `departed` populations are bounded** — a watchlist retention cap (its bind eviction deterministic: the node with the oldest successful refresh first, ticker tie-break, retired `capacity-evicted`, no episode opened) plus the review's retirements and the carry horizon, with `departed` pick tombstones pruned on the archive's retention ([configuration.md §Local Analysis Suite Configuration](configuration.md#local-analysis-suite-configuration)).
  **Live `picked` nodes carry no cap of their own**, mirroring the deliberately uncapped matrix lifecycle.
  **A discovery-coverage ledger** persists separately from the graph: one row per canonical route class and one per broad-industry / active-theme coverage subject, with first-seen, last-attempted, last-successfully-completed, last route id, completion / gap state, and computed calendar-time coverage debt; completed no-idea routes advance the units they actually researched, while failed / exhausted attempts retain the prior success timestamp ([trade-opportunities-workflow.md §Step 3b](trade-opportunities-workflow.md#step-3b-model-led-hypothesis-research)).
  **An archive of departed picks** (*downstream* of the matrix and distinct from the matrix, graph, and coverage ledger) persists the most recent **100** opportunities a failed re-evaluation removed from the matrix (the single `failed-reevaluation` trigger → `invalidated` status, set **only by a deep re-evaluation** — the cheap re-derivation never archives) — each a **frozen verdict snapshot**: the thesis document with its bear case, the archetype, the leading metric, `became_opportunity_at`, the departure date, the failing signal, the admission provenance (`admitted_by` alone — both arms' gate legs live on the run audit, never the archive row), conviction at exit, the stamped sector identity its per-run vs-sector refresh reads, on a hard-trigger forced archival the status-override divergence, and any standing narrative divergence — pruned oldest-first.
  No forward prediction and no since-flagged numbers are stored: the **since-flagged read is recomputed statelessly each run** from price history (the same reconstruction the matrix uses), so an archived pick needs no per-run price snapshot, and re-discovery simply removes the row ([trade-opportunities.md §Archived opportunities](trade-opportunities.md#archived-opportunities)).
  **The episode store** is the job's own, with Portfolio's record and mechanics plus two fields: per episode the symbol and its **lifecycle id**, the **decision class** — `picked` / `gate-reject` / `excluded` — the creation date, the spot that day and the anchor close with its bar date, and the model's expected price and the engine's base value at each of the three horizons; written onto each once, its **checks** — horizon, check date, the close read, and both arms' expected price and per-check score — or its unscorable states with their cause ([trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)).
  The store is **append-only** — an episode is never updated or deleted — and persists **independent of run retention, the archive's 100-cap and matrix presence**: a departed lifecycle's episodes keep maturing, and a re-entry opens episodes under its new lifecycle while the old ones run to their horizons.
  Each lifecycle's **accuracy scores** — per horizon, per arm, each with the date of the last check that moved it and whether the prior analysis read it — persist with the opportunity; a turned-away name's checks sit in the store with their decision class and reach no prompt and no card.
- **Run audit record** — each run also stores what it was based on.
  What follows is the **full design across both jobs**, narrowed to the as-built subset at this bullet's end.
  The record carries the holdings snapshot (portfolio), and the report(s) and sources used with retrieval timestamps and their app-computed source-quality annotations (evidence tier / extraction quality / recency — [web-research.md §Source quality and evidence weighting](web-research.md#source-quality-and-evidence-weighting)).
  For Portfolio it carries the research documents — the write-ups as written, the distillation shape and call count, the analysis, the review and the thesis document — and the **page roster** of every page shown to the model (address, title, publication date, retrieval time and source tier — never page text), the durable provenance ([web-research.md §Safety and provenance](web-research.md#safety-and-provenance)).
  For Trade Opportunities it carries the same research documents per candidate — the write-ups as written, the distillation shape and call count, the analysis, the review on a carried name and the thesis document with its appendix values — with the candidate's page roster and research gaps; no typed research field exists, and a lead is never recorded as provenance ([trade-opportunities-workflow.md §Step 5h](trade-opportunities-workflow.md#step-5h-deterministic-risk-tier-gate-validation--checkpoint)).
  It carries the **Trade Opportunities discovery and screening inputs**: which screens, routes, and themes surfaced each candidate; the route plan with the pre-plan coverage-debt snapshot, the app-inserted coverage route, attempted / completed units, and post-run ledger state; each route's hypothesis document and appendix with the route's write-ups and page roster; the watchlist review and its decisions; and the refresh write-up where the lane ran, with every research-watchlist node considered / selected / skipped and its result and timestamp decision.
  It carries the computed financial metrics and the derived reads.
  **Trade Opportunities' contract**, per pick, is **both arms' values side by side**: the model's conviction and expected prices as authored beside the engine's bands, with no cap, no stand-in and no app-computed final value; a tripped hard trigger behind a debut exclusion or a forced carried archival persists as a typed record ([trade-opportunities-workflow.md §Step 5h](trade-opportunities-workflow.md#step-5h-deterministic-risk-tier-gate-validation--checkpoint)).
  Portfolio's conviction persists as authored, with no cap and no engine conviction beside it; a tripped hard-forensic state and any matched overlay rule persist as **engine-arm annotations** ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
  It carries **both jobs' accuracy records** — the episodes this run opened, the checks and unscorable states it wrote, and each holding's or lifecycle's accuracy scores ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration), [trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)) — and, for Trade Opportunities, the floor's hold-out reasons and freshness states and the cheap sweep's warnings.
  It carries **each Trade Opportunities pass's reused-vs-freshly-fetched document split** (the document-level cache's audit leg — [trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture)); Portfolio's page-level reuse shows on its roster as each page's retrieval time.
  It carries the price-target methodology, including its discount-rate assumption and the stored **anchor-window spread percentiles and drivers** the engine-only quick paths later re-anchor against ([portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
  The methodology leg records the **run-time FMP `quote` price the targets were computed from** — a transient job-time input logged for traceability, **not** a persisted current-price field; displayed price reads reconstruct from the cached dated-EOD bars instead ([trade-opportunities.md §Storage and display](trade-opportunities.md#storage-and-display)).
  It carries the pre-profit overlay's state and matched rule, plus the model ids and quantizations, the prompt/schema version, and any degraded-input flags.
  Each per-holding field is recorded where the holding's verdict branch carries it: a `role_risk_only` holding has no implied-expectations range, dead-money read, target methodology, or priced-stock overlay to record ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)).
  So a run is traceable and reviewable (URLs, timestamps, the documents or distilled findings, and computed metrics, not full page snapshots), and the next run can audit it.
  The **Portfolio** record carries the source labels, the computed metrics and derived reads, the band methodology with the stored anchor percentiles / drivers, the quick-check / fund-exposure / pre-profit / hurdle bases, the **hard-forensic filings-sweep record** (its state plus the engine-matched rule where tripped), the **technology-event pre-flag record** where evaluable, the **narrative-vs-reality read**, the **implied-expectations range** per priced stock, the **FINRA short-interest row** where one resolved, the **same-underlying option-overlay record** where one was assembled, the research documents and the page roster above with the research gaps, the accuracy records, model ids + prompt/schema and evidence-floor versions, and the degraded-input flags.
  The run-level data-health record also retains the complete ordered physical-attempt observation list, including app packet size, elapsed time, thinking-character completeness, request metadata, adapter outcome, and raw API counters ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)).
  Its checkpoint ownership follows [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture); successful checkpoint cleanup does not remove the finished run's observations.
  Both provenance fields are recorded from the work actually done, never from configuration: the source labels name every adapter the holding consulted (the FMP profile lookup and the FRED rate anchors included, and a consulted-but-empty SEC read labels as empty), and the model ids drain each outbound request's routed model in first-call order, deduplicated in place.
  A no-model exit persists none; on a distinct live roster research normally puts the reasoner before the fast-tier distillation, while an oversized distillation or its expanded output retry records the reasoner it actually used instead of the configured fast slot.
  Both jobs' research-derived artifacts are the documents and the roster above with the distillation shape and call count; neither job stores a raise or an app-derived final conviction, and every Trade Opportunities leg above is designed with that job.
  **Both jobs' records are two-arm**: each stores the engine arm's values and the model arm's beside them, never merged.
  For Trade Opportunities that is both arms' risk tier and horizon, the model's conviction and three expected prices beside the engine's three bands, and any hard-trigger **exclusion / forced-archival record**.
  It further stores the **admission provenance** (`engine-and-model` / `engine-only` / `model-only`) with **both arms' full entry-gate vectors** whatever the outcome.
  It stores the recorded **divergences** too: the prior and current archetype labels on a carried name; the tier / horizon pair (the model's placement against the engine's derived pair); the **narrative divergence** — the engine's anchorless `hype` read beside the model's `still-valid` status on a carried name; and a hard-trigger **status override** — model-proposed status, app-forced status, matched trigger, filing — the field set at [trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable).
  The divergences ride the run audit; the status override and a standing narrative divergence additionally ride the archive record.
  For Trade Opportunities, the audit's target-methodology leg additionally records the archetype driver override's rung, the engine horizon's derived basis and every limited-history eligibility decision, so neither the engine's derivation nor the app's response to it is hidden.
- **Web-research source state (shared across both local features, not a per-run store)** — a learned, persisted layer the fetch loop accumulates across runs: each domain's resolved **`extractionProfile`** (`api_or_html` / `html` / `js_required`) and the **extraction telemetry** behind it (per-domain full-text-vs-thin-stub recovery counts, with the failed and denied fetch-attempt counts beside them — [web-research.md §Source quality and evidence weighting](web-research.md#source-quality-and-evidence-weighting)), a derived **render-first flag** (a domain repeatedly thin to a plain GET skips straight to the WKWebView render tier next time, sparing a wasted GET — [web-research.md §Fetch and extraction](web-research.md#fetch-and-extraction)), and each **Connected Source's health state** (`connected` / `connected_but_thin` / `expired` / `unsupported`).
  This is **shared infrastructure, deliberately not job-partitioned** — extraction behavior is a property of the domain, not of a job's learnings, so it sits outside the per-job vector partitions below.
  It is a **thin learned layer over heuristic defaults** (an unseen domain just uses the default profile), parallel to the registry: the registry *defaults* are seed config and the user's **registry overrides** live in the settings store ([configuration.md §Web Research](configuration.md#web-research)), while a Connected Source's **session credential stays in the macOS Keychain, never SQLite** ([§SQLite](#sqlite); [configuration.md §Connected Sources (subscriptions)](configuration.md#connected-sources-subscriptions)).
- **Price-bar cache (shared across both local features, not a per-run store)** — the daily bars (FMP dated EOD) the render-time since-flagged floor and the engine read, cached so the matrix display needn't re-fetch on every page open ([trade-opportunities.md §Storage and display](trade-opportunities.md#storage-and-display)).
  Like the web-research source state above it is **shared infrastructure, deliberately not job-partitioned** — price history is a property of the **symbol**, not a job's learnings — keyed by symbol, holding the cached daily bars plus each symbol's **`last_requested_at`** (UTC) and **latest-bar as-of date**.
  (As-built the cache stores the bars alone; the per-symbol `last_requested_at` meta rides the Trade Opportunities render-time lazy rule's slice, which is what reads it.)
  The render-time floor recomputes return / drawdown locally from this cache and re-requests a symbol's bars only **after 8 PM ET (DST-aware `America/New_York`) and not within the prior 24 hours**, fail-soft (a failed refresh keeps the cached series with its as-of date; the 8 PM hour and the 24-hour window are calibratable).
The **archive-price refresh holds a job-time rule of its own**: each Trade Opportunities run requests every distinct archived symbol through this same cache — deduped against the swept and accuracy-check populations, the endpoint surface's third maintenance population ([data-sources.md §Trade Opportunities — endpoint surface](data-sources.md#trade-opportunities--endpoint-surface)) — so an archived pick's since-flagged read is current as of each run whether or not the archive view is opened; the render-time lazy rule above covers page opens between runs.
  **Both jobs' accuracy checks** — Portfolio's before the per-holding loop, Trade Opportunities' at job start in every mode — request every episode symbol with a due horizon — held, exited, unselected, departed or turned away alike — through the same cache, read the close on the horizon date or the last session within its drafted proximity, and write the check; a failed refresh leaves the horizon pending, while a served series with no close inside the proximity writes it unscorable at once ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration), [trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)).
  The cache backs the **price-only** display and accuracy-check reads; the live **FMP `quote`** the engine's gate/target math uses at job time is **not** cached here — it is a transient job-time input, logged in the run audit record.
- **Web-research document cache (shared across both local features, not a per-run store; built with the research-loop slice)** — the cross-run per-fetch research cache both jobs' failure postures name: fetched, readability-extracted documents keyed by the **normalized requested URL**, with the normalized post-redirect final URL retained separately for provenance and direct-final lookup.
  A redirecting seed therefore hits on repeat instead of spending another live fetch; each row carries its **original retrieval timestamp** — the immutable evidence vintage, never rewritten on reuse — and is reusable within the shared **~4-week** freshness window (entries age out past it).
  For Trade Opportunities the cache is **document-level by construction** — reuse never substitutes for a live search, and the floor-bearing freshness reads must be met from currently searched results ([trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture)); Portfolio's cross-run research memory is no cache but the holding's **analysis** on its audit record, riding the next run's research prompts whatever its age, so this cache and the holding-scoped pages-already-retrieved block are its only page-level reuse ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
  Shared infrastructure like the stores above — a fetched document is a property of the URL, not a job's learnings.
- **Factor-distribution store (shared across both local features, not a per-run store)** — the accumulated per-factor observations (winsorized factor values bucketed by cap-band × sector) that ride beside the Trade Opportunities quant composite as **diagnostic context only — never a score input** ([trade-opportunities-workflow.md §Step 5c](trade-opportunities-workflow.md#step-5c-deterministic-analysis-archetype-weighted-engine)) — the score's basis is sector-adjusted absolute bands + the company's own history; a selected sample of what the app happened to analyze is never a market percentile at any weight, so the store graduates into the score only when fed by the representative-universe snapshot.
  **One current observation per issuer per factor** — a re-analysis replaces, never appends, so frequently revisited names can't overweight the distribution — contributed by both jobs' engine passes (one shared engine).
  Shared infrastructure like the two stores above — a factor observation is a property of the market cross-section, not a job's learnings — and bounded: observations are time-stamped and age out (drafted: ~24 months, so the basis stays regime-current), with a per-bucket **unique-issuer floor** below which the bands stand alone and the composite flags low confidence ([trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable)).
  A periodic stratified snapshot of a representative universe is the named upgrade path (calibration-tier, unscheduled).

Each feature retains its most recent N runs, pruned independently of the 30-report report-retention window and of each other (the same additive-history pattern as baseline snapshots).
The Schwab app secret and OAuth tokens are the one exception to SQLite credential storage — they live in the macOS Keychain (see [schwab-integration.md](schwab-integration.md), [configuration.md](configuration.md)).
Each durable store above joins the whole-corpus portability archive with the slice that lands it — a required manifest entry plus a format-version bump ([data-portability.md §Build-order placement](data-portability.md#build-order-placement)).

### Local Vector Memory

Vector memory is the report's alone: **neither local job writes or reads a row**.
Portfolio Analysis's continuity is the deterministic load of the holding's own documents ([portfolio-analysis.md §Continuity and isolation](portfolio-analysis.md#continuity-and-isolation)), and Trade Opportunities' the deterministic load of the lifecycle's own documents by lifecycle id ([trade-opportunities.md §Continuity and isolation](trade-opportunities.md#continuity-and-isolation)); the suite makes no embedding call and carries no embedder in its roster ([local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)).
The `vector_memory` table and its exact cosine search in Rust are the report's ([§Vector Memory](#vector-memory)); no local namespace holds a row.
