# Export System

Reports are authored and stored internally as Markdown. The application also generates an HTML version for in-app display and PDF generation. For the canonical authoring format and the Markdown-vs-HTML distinction, see [report-structure.md](report-structure.md).

## Export Options

Users can export a Market Signal report as:
- Markdown
- PDF

## Markdown Export

Markdown export uses the canonical Markdown report.

Markdown exports preserve:
- report structure
- headings
- source links
- written analysis
- and any Markdown-compatible tables or lists

## PDF Export

PDF export is generated from the HTML report version using the Tauri webview's built-in print-to-PDF capability. Because the same webview engine renders both the in-app HTML and the exported PDF, presentation fidelity is preserved.

PDF exports preserve:
- rendered report styling
- charts, graphs, and tables included in the HTML report
- source links when supported by the underlying webview's print-to-PDF
- and the full written report content

## Export Naming

Exported files use the report date and report title in the filename.

Example:
```text
2026-05-24-market-signal-report.md
2026-05-24-market-signal-report.pdf
```

## Export Behavior

Exports are generated from the stored report artifacts.

Exporting a report does not re-run the agent workflow, regenerate analysis, or modify the stored report.
