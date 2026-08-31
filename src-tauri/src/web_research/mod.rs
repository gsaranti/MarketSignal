//! The local suite's web-research tool — search, fetch, and extract
//! (`docs/web-research.md`).
//!
//! The Rust orchestrator runs this tool on a model's behalf: a research stage
//! *requests* a search or a page, the application layer performs the network
//! I/O, and clean text comes back — the model never touches the network,
//! holding the same pure-stage boundary as the report pipeline. The tool is
//! keyless, local-first, and cost-free — the local suite is SearXNG-only (the
//! metered Tavily key serves only the report job).
//!
//! Module split:
//! - [`registry`] — the source registry and evidence tiers: per-domain
//!   metadata, the deny drop, and the app-computed evidence annotation
//!   (`docs/data-sources.md §Source registry and evidence tiers`).
//! - [`fetch`] — the SSRF-guarded page fetch and readability extraction
//!   (`docs/web-research.md §Fetch and extraction`, §Safety and provenance).
//! - [`search`] — the SearXNG JSON-API backend (SearXNG-only; Tavily is
//!   reserved for the report job) and the deny/syndication filtering applied
//!   at rank time
//!   (`docs/web-research.md §Search backend`, §Tavily fallback).
//!
//! The rendered-retrieval escalation tier and Connected Sources are deferred
//! by ruling (2026-08-23): the fetch layer records the extraction telemetry
//! that will gate the render tier, but no webview render or authenticated
//! fetch ships in this slice.

pub mod fetch;
pub mod registry;
pub mod search;
pub mod store;
