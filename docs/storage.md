# Storage

## Markdown File Storage

Canonical Markdown reports are stored as files on the local filesystem. Each file uses the same naming convention as exports:

```text
YYYY-MM-DD-market-signal-weekly-report.md
```

See [export.md §Export Naming](export.md#export-naming) for the canonical filename convention.

## SQLite

Stores:
- report records
- report metadata
- HTML output
- job history
- warning states

Each report stores:
- creation timestamp
- structured report summary metadata
- market regime metadata

The market regime metadata holds a single `market_regime` label drawn from this fixed vocabulary:
- `risk-on`
- `risk-off`
- `mixed`
- `late-cycle`
- `recessionary`
- `recovery`

The main agent selects the label that best fits the current regime when synthesizing the report. Free-form regime commentary belongs in the report Markdown itself (see [report-structure.md](report-structure.md)), not in the label.

### Report Summary Metadata Schema

The structured report summary metadata is a JSON object the main agent populates when writing the final report.

Required fields:
- `report_id` — UUID for the report.
- `report_type` — currently always `weekly_market`.
- `created_at` — ISO-8601 timestamp.
- `market_regime` — one of the fixed regime labels above.
- `thesis_stance` — one of: `bullish`, `bearish`, `mixed`, `uncertain`.
- `header_summary_bullets` — array of 3–6 strings, matching the report's `## Header Summary` section.

Optional fields (may be empty arrays):
- `key_risks` — top risks identified in the report.
- `unresolved_questions` — open thesis questions to revisit in subsequent reports.
- `forward_outlook_themes` — themes flagged in the `## Forward Outlook` section.

Detailed analysis remains in the canonical Markdown report; this schema captures only the queryable fields used for cross-report retrieval and continuity.

Only the most recent 30 Weekly Market reports are retained.

Older reports are deleted automatically.

When a report is removed:
- its Markdown
- generated HTML
- metadata
- associated vector-memory summary references
are deleted together.

## LanceDB Vector Memory

Stores:
- report summaries
- durable learnings
- thesis evolution
- important historical analogs
- past mistakes
- retrospective audit learnings
- useful recurring patterns

The vector DB acts as long-term semantic memory for the main agent.

Deleting older reports does not remove durable learnings already stored in vector memory.

This allows the system to preserve long-term analytical continuity even while older report files are removed from local storage.

### Embeddings

Embeddings are generated with OpenAI `text-embedding-3-large`, using the configured OpenAI API token (see [configuration.md §API Tokens](configuration.md#api-tokens)).

Each item is embedded as a single atomic unit:
- one embedding per report summary
- one embedding per durable learning

Report Markdown is not split into fixed-size or section-based chunks for vector memory; the report-summary metadata is the unit that enters LanceDB.
