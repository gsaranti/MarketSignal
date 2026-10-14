//! The local suite's web-research tool — search, fetch, and extract
//! (`docs/web-research.md`). The application layer performs every network
//! call on the model's behalf; the model never touches the network. SearXNG
//! only — the metered Tavily key serves the report job alone.

pub mod fetch;
pub mod registry;
pub mod search;
pub mod store;
