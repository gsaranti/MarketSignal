# Overview

Market Signal is a local-first desktop application built with:
- [Tauri](https://tauri.app/start/) ([Rust API docs](https://docs.rs/tauri/latest/tauri/))
- [Vue](https://vuejs.org/guide/quick-start.html)
- [SQLite](https://www.sqlite.org/docs.html)
- [LanceDB](https://docs.lancedb.com/quickstart) ([Rust API docs](https://docs.rs/lancedb/latest/lancedb/index.html))

The app runs scheduled weekly market-analysis jobs, produces evolving market reports, stores recent report history, and uses memory retrieval to improve future analysis.

The weekly cadence is intentionally designed to prioritize signal over noise, structural market analysis, thesis continuity, and forward-looking market preparation rather than reactive daily commentary.

The application is not a trading bot. It acts as a professional market-analysis and thesis-generation system focused on:
- market regimes
- evolving macro theses
- geopolitical and economic developments
- sector analysis
- forward-looking market preparation
- investment strategy guidance

The application runs entirely on the user's machine except for external API/model requests.
