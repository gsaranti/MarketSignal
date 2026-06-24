# Current session handoff

## What happened

**Spec'd the next major feature set — the local analysis suite: two on-demand,
local-model-only features, Portfolio Analysis and Trade Opportunities.**
Researched the pieces (Schwab API, local models for an M5 Max, local serving,
free data APIs, options data) and wrote the design into the corpus: **5 new docs**
(`local-models`, `web-research`, `schwab-integration`, `portfolio-analysis`,
`trade-opportunities`) + ~8 touched + `BUILD.md`/`INDEX.md`. Hardened across
**3 Codex review rounds**. **Four commits to main** (`790fb5b`→`351279a`), all
pushed. **No code yet** — docs are the spec, written as-built; BUILD.md's trailing
section carries the *planned* status.

Load-bearing calls locked (so they aren't re-litigated): local-only **Qwen3.5
roster** via **Ollama-on-MLX**; a **deterministic finance engine** (Rust computes
metrics / sub-scores / risk-tiers / scenario targets, the model *interprets* —
never invents numbers); data = **Schwab** (holdings + option chains, **required to
run either job**) + **SEC EDGAR** + **Stooq** + **Finnhub** + FMP-free-niche, with a
**SearXNG** web tool; **per-job isolated** vector memory; a full **investor
profile**; options put/call + IV/skew as an **activity proxy** (kept out of grades
until calibrated). FMP stays on the free tier.

## Current state

Spec phase complete and committed; `main` at **`351279a`**, in sync with origin,
tree clean. **No implementation started.** The cloud Market Signal Report (daily
app **v1.1.1**) is unchanged. Next thread is the build, via `/metis-plan-task`.
Build order: **substrate** (Ollama supervision + a flexible `local_model.rs`
adapter, decoupled from the closed `AgentModel` enum) → a **narrow single-equity
Portfolio slice against a fixture Schwab source** (stub holdings + chains, offline)
→ full Portfolio (live OAuth, funds) → Opportunities.

## Open questions

- **Plan-time parameters (do at `/metis-plan-task`)** — *pin in the docs:* N/X
  (run retention, reports-as-context), horizon lengths, default investor profile.
  *Keep calibratable* (method documented, numbers shadow-tuned, like COT
  weighting): grade-weight formula, risk-tier thresholds, options-methodology params.
- **Register the Schwab developer app** — multi-day approval is the external long
  pole; the hard gate means manual import can't substitute for real-data runs.
- **Cadence Run B** (carried) — report #2 still un-run: first run with a prior
  snapshot + summary embedding; validates the delta engine + memory recall. On it,
  sanity-check yield levels vs the 2s10s claim and the COT read
  ([[manual-pivot-cadence-windows]], [[report-curve-number-consistency]]).
- **Standing report-side nits** (carried) — COT extreme-weighting calibration
  ([[skills-forcing-function-only]]); `market_clock` mislabels market holidays /
  early closes (needs an NYSE calendar); opus-main leaning accumulating
  ([[live-config-opus-main-leaning]]); do NOT reintroduce PDF `@page` margins.

## Where to start

**`/metis-plan-task` to slice Phase 1 of the local analysis suite — the substrate
first** (Ollama-on-MLX supervision + the flexible `local_model.rs` adapter), since
everything builds on it; settle the durable plan-time parameters there. Kick off
**Schwab developer-app registration in parallel** (long approval lead). **Cadence
Run B** (report #2) remains an independent, quick alternative if you'd rather
validate the live build first.
