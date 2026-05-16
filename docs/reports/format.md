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
