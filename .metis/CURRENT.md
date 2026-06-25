# Current session handoff

## What happened

Built and merged **Phase 1 of the local analysis suite — the substrate**, via the
full Metis loop (plan → implement → metis-review → Codex → merge). Landed
`src-tauri/src/local_model.rs` (new): a flexible `LocalModelClient` parameterized by
`{endpoint, model_id, messages, tools, format_schema, options}` over Ollama-native
`/api/chat`, **decoupled from the closed cloud `AgentModel` enum**; daemon supervision
(`health_check` / `available_models` / `probe_daemon`); the independent `local_gate`
(reusing `ValidationReport` + a new `WarningKind::LocalModels`); and an NDJSON stream
decoder on the existing `progress` seam. Plus local config keys/fields (deliberately
**absent from the cloud `validate` gate**), a `LocalEmbedder` behind the existing
`Embedder` trait (`/api/embed`), and a `test_local_daemon` spawn_blocking command.
**Squash-merged to `main` as PR #44 (`c39a3f7`).** Codex caught two real Medium issues,
both fixed: `normalize_endpoint` accepts both the daemon host and the documented
`…/api` base (no `/api/api/...`); and streaming cancel/truncation now return `Err`
(not a partial `Ok`), so a prose stage can't mistake a cut-off stream for success. The
cloud report pipeline is unchanged.

## Current state

`main` at **`c39a3f7`**, tree clean, in sync with origin; the side branch is deleted.
`cargo test` 478 pass / clippy clean. **No work in flight.** The substrate is as-built
and **BUILD.md now reflects it** (substrate marked built; Portfolio & Opportunities
still planned). Deferred to later phases: the local Settings UI, the Persistent Warning
Area frontend for
`WarningKind::LocalModels`, vector-memory job-namespace partitioning, and
schema-mismatch retry. The local adapter is deliberately a *primitive* (it does **not**
implement `MainAgent`/`AnalystAgent`); per-feature stages wrap it in Phase 2.

## Open questions

- **Durable plan-time parameters belong to the Phase-2 plan, not the substrate**
  (confirmed this session): pin at the Portfolio-slice `/metis-plan-task` — N/X (run
  retention, reports-as-context), horizon lengths, default investor profile. *Keep
  calibratable — do NOT pin:* grade-weight formula, risk-tier thresholds,
  options-methodology params (shadow-tune against live runs, like COT weighting).
- **Register the Schwab developer app** — multi-day approval is the external long pole;
  the hard gate means manual import can't substitute for real-data runs.
- **Cadence Run B** (carried) — report #2 still un-run: validates the delta engine +
  memory recall; sanity-check yield levels vs the 2s10s claim and the COT read
  ([[manual-pivot-cadence-windows]], [[report-curve-number-consistency]]).
- **Standing report-side nits** (carried) — COT extreme-weighting calibration;
  `market_clock` mislabels holidays / early closes (needs an NYSE calendar); opus-main
  leaning accumulating ([[live-config-opus-main-leaning]]); do NOT reintroduce PDF
  `@page` margins.

## Where to start

**`/metis-plan-task` for Phase 2 — the narrow single-equity Portfolio slice against a
fixture Schwab source** (stub holdings + option chain, offline) + FMP + SEC + the local
models, validating quality/runtime offline; settle the durable plan-time parameters
there. **Kick off Schwab developer-app registration in parallel** (long approval lead).
Cadence Run B remains an independent quick alternative if you'd rather validate the live
build first.
