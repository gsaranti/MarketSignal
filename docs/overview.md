# Overview

Market Signal is a local-first desktop application built with:
- [Tauri](https://tauri.app/start/) ([Rust API docs](https://docs.rs/tauri/latest/tauri/))
- [Vue](https://vuejs.org/guide/quick-start.html)
- [SQLite](https://www.sqlite.org/docs.html) — also hosts the vector memory (embeddings stored in-table with exact cosine search in Rust; see [storage.md §Vector Memory](storage.md#vector-memory) — amended from the original LanceDB plan)

The app generates Market Signal reports on demand, produces an evolving market thesis, stores recent report history, and uses memory retrieval to improve future analysis. Report generation is user-initiated — there is no automatic scheduler — so the user decides the cadence.

Each report is deliberately framed as a considered market read that prioritizes signal over noise, structural market analysis, thesis continuity, and forward-looking market preparation rather than reactive daily commentary. The editorial framing — not a fixed schedule — is what keeps the analysis structural rather than reactive, however often the user chooses to generate a report.

The application is not a trading bot. It acts as a professional market-analysis and thesis-generation system focused on:
- market regimes
- evolving macro theses
- geopolitical and economic developments
- sector analysis
- forward-looking market preparation
- investment strategy guidance

## Local Analysis Suite

Alongside the cloud-generated report, the application includes a **local analysis suite** that runs on local open-weight models on the user's machine — no external model provider and no per-call model cost. It adds two on-demand features:
- **Portfolio Analysis** grades the holdings in the user's Charles Schwab portfolio and recommends an action and price targets for each, grounded in the current report's house view (see [portfolio-analysis.md](portfolio-analysis.md)).
- **Trade Opportunities** researches new investment ideas across a fixed risk-by-horizon matrix (see [trade-opportunities.md](trade-opportunities.md)).

Both are deliberately prescriptive — they issue grades, actions, and targets — but the application remains an analysis system, not a trading bot: it never places orders. The substrate they share (local model serving, the web-research tool, and isolated per-job run memory) is described in [local-models.md](local-models.md).

The report pipeline runs entirely on the user's machine except for external API/model requests; the local analysis suite makes **no external model calls** (its models run on-device), but it still reaches the network for the data its analysis needs — Charles Schwab for holdings, FMP for financials, and the web (via SearXNG and page fetches) for research.
