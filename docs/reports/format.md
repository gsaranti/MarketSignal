# Report Format

Reports are written in Markdown by the main agent. An HTML version is generated for application display.

Reports are authored and stored internally as Markdown.

Markdown is the canonical report format used for:
- agent memory
- report continuity
- vector memory ingestion
- report retrieval
- future report synthesis

HTML reports are generated from Markdown and are presentation-only artifacts used for:
- in-app rendering
- styling
- chart display
- PDF generation

Agents never ingest or reason over HTML reports.

Report structures themselves are documented separately: see [standard-structure.md](standard-structure.md) for the premarket/postmarket template, and [weekly-review.md](weekly-review.md) for the weekly review template.

## Sources Section Format

Both report templates end with a `## Sources` section. The format below applies to both.

Sources are grouped by type using `###` subheadings, in this order when present:

- News
- Research Documents
- Data Providers

Each entry within a group is a single bulleted line:

```text
- [Title](URL) — Publisher (YYYY-MM-DD)
```

- **Title**: the article, document, or dataset name.
- **URL**: a link to the source. For ingested research documents from `/research-archive`, use the local file path.
- **Publisher**: the originating publication, organization, or data provider (e.g., "Bloomberg", "Federal Reserve", "OpenBB / FMP").
- **YYYY-MM-DD**: publication date (for news/research) or retrieval date (for data providers).

Groups with no entries for a given report are omitted. The Sources section itself is required in every report.
