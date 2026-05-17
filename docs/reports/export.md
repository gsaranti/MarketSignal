# Export System

Reports are stored internally as:
- Markdown
- HTML

## Export Options

- Export Markdown
- Export PDF

PDF export is generated from the HTML report version using the [`headless_chrome`](https://docs.rs/headless_chrome/latest/headless_chrome/) Rust crate. The application drives a headless Chromium instance to render the HTML report and write the PDF directly to a user-chosen location — no OS print dialog. `headless_chrome` requires a Chrome or Chromium installation on the user's machine; if one is not available, PDF export fails with an error message instructing the user to install Chrome.
