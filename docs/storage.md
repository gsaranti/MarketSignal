# Storage

## Storage Location

All persisted state lives **outside the application bundle**, in the per-user
application-data directory resolved from the app's bundle identifier (not its
product name):

```text
~/Library/Application Support/com.georgesarantinos.market-signal/
    market_signal.db        the SQLite database (see below)
    reports/                canonical Markdown reports
    research-inbox/         documents awaiting processing
    research-archive/       processed documents
```

Because the location is keyed by the **bundle identifier**, it is stable across
versions: rebuilding or replacing the installed app (a new `tauri build`) reads
and writes the same store, so existing reports, metadata, and vector memory are
preserved across updates. The bundle never contains data, so replacing the
`.app` cannot lose any.

**Development isolation.** Debug builds (`tauri dev`) nest their store under a
`dev/` subdirectory of the path above, so a development session never touches
production data; release builds (`tauri build`) use the directory as-is. The
`MARKET_SIGNAL_DATA_DIR` environment variable overrides both — pointing any
build at an explicit directory — for tests, automation, and isolated live runs.

## Markdown File Storage

Canonical Markdown reports are stored as files on the local filesystem. Each file is named with the report date plus an 8-character `report_id` suffix, so a same-date rerun never overwrites an earlier run's file:

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

HTML is deliberately not among the stores (amended 2026-06-12 from the original spec, which kept a stored HTML copy alongside each report): the HTML view is a presentation artifact rendered on demand in the webview from the canonical Markdown, and PDF export prints that same rendered view, so a stored copy would have no reader. See [report-structure.md §Presentation Format](report-structure.md#presentation-format-html).

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

The main agent selects the label that best fits each axis when synthesizing the report. Free-form regime commentary belongs in the report Markdown itself (see [report-structure.md](report-structure.md)), not in the labels.

### Report Summary Metadata Schema

The structured report summary metadata is a JSON object stored with each report. The application stamps the identity fields (`report_id`, `report_type`, `created_at`); the main agent authors the remaining fields when writing the final report.

Required fields:
- `report_id` — UUID for the report.
- `report_type` — always `market_signal` (the report kind; the legacy value `weekly_market` is migrated to it — see [§Legacy Naming Migration](#legacy-naming-migration)).
- `created_at` — ISO-8601 timestamp.
- `title` — a short, specific per-issue headline the main agent writes (e.g. "Rotation, not rupture"), distinct from the constant `report_type` product name. Surfaced as the report's label in the UI (the Recent Reports list). Stored on the summary with a serde default, so summaries persisted before this field decode with an empty title and the UI falls back to the product name "Market Signal Report".
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
- associated vector-memory summary references
are deleted together. (There is no HTML to remove — HTML is rendered on demand, never stored; see [§SQLite](#sqlite).)

### Legacy Naming Migration

The report's stored identifiers were renamed when the product moved from a fixed weekly schedule to on-demand generation. Reports created under the earlier convention are migrated in place on first launch after the upgrade:

- the `report_type` metadata value `weekly_market` is rewritten to `market_signal`;
- report files named `YYYY-MM-DD-market-signal-weekly-report.md` are renamed to `YYYY-MM-DD-market-signal-report.md`, and any stored file paths are updated to match;
- the `job_runs.job_type` value `weekly_market` is rewritten to `market_signal` — a separate single-column migration covering the job-run history's slug, distinct from the `report_type` rewrite above.

The migration is one-time and idempotent: a report (or job-run row) already carrying the new identifiers is left untouched. No report content changes — only the type slug, the job-run slug, and the filename.

### Baseline Snapshots

Each report stores a snapshot of the baseline market-data scan that produced it (the Step-3 gather, serialized as JSON). On the next report, the application diffs the current scan against the most recent prior snapshot to produce a per-report change view — the level moves since the previous report — handed to the main agent so the thesis can ground "what changed" in measured deltas rather than the prior report's prose.

The most recent 14 snapshots are retained, pruned independently of the 30-report report-retention window. The cadence is report-indexed, not calendar-indexed: because reports can be generated on demand at any time (see [scheduling.md §Generating a Report](scheduling.md#generating-a-report)), the change view reports the actual elapsed interval since the previous report rather than assuming a week.

A missing or unreadable prior snapshot is non-fatal: the report is generated without a change view. Snapshots are additive context, never a precondition for a report.

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

The store is implemented inside the application's SQLite database (a `vector_memory` table holding each item's embedding as bytes) with exact cosine search in Rust — a deliberate engine choice over the originally specified LanceDB (amended 2026-06-11). At this corpus's scale — at most 30 retained report summaries plus durable learnings — an unindexed vector database performs the same exhaustive scan, with a materially heavier dependency footprint. Everything else in this section is engine-agnostic and unchanged; the store sits behind a single module so the engine could be swapped if the corpus ever outgrows exact search.

Deleting older reports does not remove durable learnings already stored in vector memory.

This allows the system to preserve long-term analytical continuity even while older report files are removed from local storage.

### Embeddings

Embeddings are generated with OpenAI `text-embedding-3-large`, using the configured OpenAI API token (see [configuration.md §API Tokens](configuration.md#api-tokens)).

Each item is embedded as a single atomic unit:
- one embedding per report summary
- one embedding per durable learning

Report Markdown is not split into fixed-size or section-based chunks for vector memory; the report-summary metadata is the unit that enters vector memory.

## Local Analysis Suite Storage

The local analysis suite (see [local-models.md](local-models.md)) persists its own runs, separately from report storage. Each feature stores its run history in the SQLite database:

- **Portfolio Analysis** — per run, the per-holding verdicts (grade and sub-scores, action, horizon outlook, price targets, financial-analysis summary, and the continuity diff) and the portfolio roll-up. The most recent pull of holdings is also stored so the portfolio is viewable without re-fetching (see [schwab-integration.md](schwab-integration.md)).
- **Trade Opportunities** — per run, the 3×3 matrix of opportunities (each with its thesis, catalyst, horizon, risk tier, conviction, entry consideration, and carry-forward status).

Each feature retains its most recent N runs, pruned independently of the 30-report report-retention window and of each other (the same additive-history pattern as baseline snapshots).

### Local Vector Memory

Both features use vector memory for run-to-run continuity, through the same `vector_memory` table and module as the report. Three rules keep the spaces separate:

- **Per-job partitions.** The report, Portfolio Analysis, and Trade Opportunities each write and read **only their own** memory kind; no job retrieves another's learnings. Holding-grading calibration and opportunity-discovery context are job-specific, so cross-job recall would be noise.
- **A distinct embedder.** The local features embed with a local model (see [local-models.md §The model roster and per-task routing](local-models.md#the-model-roster-and-per-task-routing)), not OpenAI `text-embedding-3-large`. The report's vectors are therefore a different dimensionality and cannot be compared against the local ones; the two local features share an embedder, so their isolation is enforced by partition, not dimensionality.
- **Own retention.** A feature's memory rows follow that feature's run retention, independent of the 30-report cascade.

The store stays exact cosine search in Rust, unchanged (see [§Vector Memory](#vector-memory)).
