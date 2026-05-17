# Overview

Market Signal is a local-first desktop application built with:
- Tauri — [start guide](https://tauri.app/start/), [Rust API docs](https://docs.rs/tauri/latest/tauri/)
- Vue — [quick start](https://vuejs.org/guide/quick-start.html)
- SQLite — [docs](https://www.sqlite.org/docs.html)
- LanceDB — [Rust API docs](https://docs.rs/lancedb/latest/lancedb/index.html), [quickstart](https://docs.lancedb.com/quickstart)

The app runs scheduled market-analysis jobs, produces evolving market reports, stores recent report history, and uses memory retrieval to improve future analysis.

The application is not a trading bot. It acts as a professional market-analysis system focused on:
- market regimes
- evolving macro theses
- geopolitical and economic developments
- sector analysis
- investment strategy guidance

The application runs entirely on the user's machine except for external API/model requests.
