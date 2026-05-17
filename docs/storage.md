# Storage

## SQLite

Stores:
- reports
- report metadata
- HTML output
- job history
- warning states

Each report stores:
- report type
- creation timestamp
- associated market session metadata

The most recent 30 full reports per report type are retained. Premarket, Postmarket, and Weekly Review reports each have their own 30-report cap. Older reports within a type are deleted automatically.

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
- useful recurring patterns

The vector DB acts as long-term semantic memory for the main agent.

### Memory Entry Types

Two distinct kinds of entries live in LanceDB. They differ by what they represent and by lifecycle.

**Report summary** — a condensed semantic representation of a single report, used to surface the report itself in future memory queries. Each report has at most one summary, keyed to the report. Written by the main agent at step 15 of the [main agent workflow](agents/main-agent-workflow.md). Lifecycle: tied to its report. When the report is deleted from SQLite, its summary reference is deleted from LanceDB (see [SQLite](#sqlite) above).

**Durable learning** — a generalizable analytical lesson (for example, a recurring market pattern, a historical analog, or a past analytical mistake). Not tied to any single report. Written by the main agent at step 16 of the [main agent workflow](agents/main-agent-workflow.md) and by the weekly review process (see [reports/weekly-review.md](reports/weekly-review.md)). Lifecycle: independent of any report. Deleting older reports does not remove durable learnings already stored in vector memory.
