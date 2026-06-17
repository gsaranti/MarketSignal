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

The application runs entirely on the user's machine except for external API/model requests.
