# Storage

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
