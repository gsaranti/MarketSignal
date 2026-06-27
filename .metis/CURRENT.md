# Current session handoff

## What happened

The **cleanup pass** on the Trade Opportunities design docs (docs-only, [[job-doc-deepening-initiative]]). Landed deferred Codex **#3–#7**: cross-lens **contradiction check** (folded into distillation+scoring, no new model call; high-severity capped at the gate; new `key falsifiers` field); discovery **diversity caps** (cap/feeder/archetype/theme floors+ceilings at consolidation); undercovered-names **operating-reality-vs-price** fallback when estimates are thin; **economic value-chain** (margin capture / bargaining power / capacity / pricing power, not mere exposure); **outcome-learning labels** (deterministic — return vs sector/market, drawdown, leading-metric continuation, decision-tree failure mode; calibration forward-staged). Then a **design correction** (user-driven): **lifted the per-cell output cap** — gates set a cell's count, not a quota; every gate-clearer listed, ranked by conviction — and reframed the Step-4 candidate cap as a **configurable compute budget** (Settings → *Trade Opportunities discovery breadth*), never a quality cap. Two Codex review rounds hardened it: carry-forward is **stateless re-discovery** (no persisted backlog; only the validated matrix carries via storage); **Step 6 completeness is app-validated** (model only ranks + proposes dedup; app checks every survivor present or collapsed-with-reason); `storage.md` synced + dedup-collapse decisions recorded in all audit lists.

## Current state

All committed + pushed on `docs/local-suite-portfolio-design-decisions` (**PR #46**). This session's commits: `57b93ef` (#3–#7), `bb6625b` (output-cap/budget), `60b6cc8` (carry-forward + Step-6 validation + storage sync), `617c6fa` (dedup audit) — atop `87643c5`/`528a7a1`. Working tree clean; docs-only (TO pipeline still planned, no code). The cleanup pass is **done except #8 (FMP tier audit)**, deliberately carried forward — it needs the user with the actual paid plan in hand. **BUILD.md assessed: no change needed** (the #3–#7 substance is already in its TO bullet; the later refinements are docs/-level).

## Open questions

- **#8 FMP endpoint tier audit (next-session lead)** — annotate each endpoint `required`/`useful`/`optional` + fallback in `data-sources.md`; bulk / holder-13F / congressional / transcripts / structured-news / segmentation are the likely higher-tier families.
- **Merge PR #46** — docs-only, clean; carries report enrichment + Portfolio sweep + TO sweep + all Codex fixes.
- **BUILD.md trim** — ~5.5k vs ~4.5k ceiling; fold the one "diversity quotas" phrase sharpen into the trim, no standalone edit warranted.
- **Candidate-backlog (deferred design choice)** — chose stateless re-discovery as the first impl; add a persisted backlog only if live calibration shows genuinely-good names getting lost.
- **Implement report enrichment** (paid-FMP-gated) — calendar consensus/surprise builder + prompt-landmine fix.
- **Implement local suite** — build order: live-Schwab OAuth → full Portfolio (funds) → Opportunities.
- **Live validation hardware-gated** on M5 ([[local-suite-hardware-gated]]) — verdict quality + runtime + FMP-tier + local-model.
- **Carried:** register Schwab developer app; `market_clock` holidays (FMP `holidays-by-exchange`); Cadence Run B (yields vs 2s10s + COT); report-side nits (COT extreme-weighting, opus-main leaning, no PDF `@page` margins).

## Where to start

**#8 FMP endpoint tier audit** — user-driven. Lay out the endpoint inventory from `data-sources.md` (the TO + Portfolio surfaces) family by family; the user verifies each against the actual paid FMP plan; annotate `required`/`useful`/`optional` + fallback behavior. Then **merge PR #46** (clean, docs-only).
