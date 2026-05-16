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

Only the most recent 30 full reports are retained. Older reports are deleted automatically.

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

Deleting older reports does not remove durable learnings already stored in vector memory.
