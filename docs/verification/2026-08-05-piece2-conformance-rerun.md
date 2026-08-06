# Pre-big-run review piece 2 — code-vs-docs conformance re-run (2026-08-05)

The re-run of the piece-2 conformance walk ([2026-08-04-piece2-conformance-walk.md](2026-08-04-piece2-conformance-walk.md)) against post-`portfolio-v7` main (`dc91b85`), covering the churn landed after the original walk:
the two-arm verdict (`portfolio-v7`, PR #61 squash `78187cb`), the listing-resolution guard (`22534fd`), the investor-profile alignment (B7, `79e781c`), the B10+B13 card slice (`1bc21d2`), the B12 context-fit instrumentation (`a5992f4`), and the FMP rate-limit hardening (`94e31b1`).

## Method

The original record's method re-applied: seven parallel conformance passes over the same scopes — (A) gate / holdings / diff / eligibility, (B) dossier / engine / targets / endpoint surface, (C) 6f–6g interpretation + ledger + two-arm, (D) pre-profit / quick check / selective, (E1) construction 7a/7b, (E2) outcome learning / storage / portability, (F) frontend/interface — with the known designed-not-built and deliberately-dormant items and the 2026-08-04 ruled dispositions excluded up front.
Every surviving finding was re-verified against the code by the orchestrating session before batching; the batch was then user-ruled in-session.
The bulk of the walk **conformed** — most notably the v7 machinery under its dedicated passes: the boundary statement genuinely single-homed (every other mention a one-line pointer), no model-arm value reaching any engine number, the retrospective's excluded-never-guessed anchor-close bridge with effective-vintage dating, the 6g identity-carry machinery property-by-property, the construction coherence-vs-annotation split and the v7 sizing ownership, the pre-profit consequence re-scope to the engine arm alone (no schema narrowing, no post-interpretation clamp, `clamp_conviction`'s sole production caller `engine_view`), the FMP 429 ladder at both chokepoints with quick-check inheritance, and the B3 / B7 / B10 / B12 / B13 slices.

## Dispositions

**14 verified divergences** (vs the original walk's 39) — 4 A + 4 B + 6 C — ruled in-session and applied the same day; the STI doc-completeness clause and the one no-action note ride outside the count as extras:

- **A (4) — code fixes, applied:**
  A1 the TTM forward-dividend pull's 12-row cap raised to the shared 60-row history margin (`fmp.rs` — future-dated declaration rows consume limit slots and a monthly payer alone fills 12, so a truncated pull silently understated `TR`'s payout leg with hurdle-flip risk), the limit test-pinned;
  A2 the over-age add-family rule-demotion extended to the `role_risk_only` arm (`job.rs` — newly reachable since v7 opened the 7b choice with departures annotated, never schema-barred; the stale-strong-action rule is branch-unscoped), test-pinned;
  A3 the paired card's model Outlook row gains its quiet `≠ engine` divergence tag (any differing horizon tags the row — the doc's conviction / outlook / lean tag triple now complete);
  A4 the model-letter chip's dead `grade-*` class binding replaced with the engine chip's `gradeClass()` binding, restoring the grade-scale tint the design system's `.grade.<letter>` compounds define.
- **B (4) — open design rulings, all four ruled to the recommended disposition and applied:**
  B1 a half-published consensus spread now holds **both** driver legs at mid (either leg missing reads as a missing spread), so `flat_driver` describes the drivers exactly and a clamp collapse stays distinguishable from a consensus-flat band;
  B2 the engine stand-in conviction's gap leg now reads the dossier's **assembled** degraded-input list — fund-metadata gaps, the DGS10-history gap, the listing-guard unverified note — matching the documented "any dossier gap" (tier gaps keep their own counter, so nothing double-counts);
  B3 the guard-conflict abstention now persists a **carrying overlay record** through the same `compute_overlay` path (eligibility-unscorable — the guard-terminal skip fetched no statements — with the prior period-keyed observation history carried), so one conflicted, possibly transient run can no longer reset a holding's history; the two overlay-survival sentences (storage.md, §Evidence floor) now name both exit shapes;
  B4 the import-side embedder-identity comparison marked **designed, not built** (the manifest records the per-namespace ids for it; the check lands with the M5-deferred re-embed machinery), with the as-built insert-unchecked state stated plainly in data-portability.md and the storage.md echo.
- **C (6, plus the STI extra) — doc / comment corrections, applied:**
  workflow §Step 2's "rejected if stale" present-tense residue aligned with schwab-integration.md's 2026-08-04 as-built marker (no as-of timestamp retained, no staleness rejection runs);
  §Outcome learning's retrospective-prompt clause scoped to the memory partition + roll-up scoreboard (the retrospective prompt carries the holding's own deterministic matured window lines, per workflow §Step 6f);
  the data-sources statements row's re-pull clause scoped to the income + balance-sheet legs (as-built no cash-flow re-pull);
  the episode-snapshot enumeration's "composite" and "tier premium" reworded as re-derivable (neither is a literal snapshot field);
  `SizingSpineRow.lean`'s stale rustdoc corrected (a carried row's lean is `None`; the stale lean rides `prior_lean` and the carried verdict);
  configuration.md's "fast tier never gates" scoped to presence (a *configured* fast id is verified like any rostered model by the connectivity check);
  plus the STI-absent-reads-zero liquid-resources convention added to the §Starting parameters formula line (previously recorded only in code).
- **No action (1):** the roll-up section's overlay-delta design-voice sentence — its as-built marker is single-homed at §The per-holding pipeline per the corpus's single-home convention.

## Review

Internal review (the Metis task reviewer over the applied working-tree batch): **approve, no findings** — per-ruling fidelity, edge-case correctness (the B1 `(mid, mid)` clamp interaction, B2 double-count, B3's statement-empty `compute_overlay` and the exit audit confirming the guard conflict was the only priced-stock gap, A2's demotion-before-sizing ordering), old-code-failing tests, sentence-per-line + single-home consistency, and scope all passed.
Its one outside-the-diff observation — schwab-integration.md's canonical chain sentence still reading as though the app retains the wire timestamp — was tightened in the same batch (wire-payload vs app-retention made explicit).

External review (two Codex rounds, every finding verified against code before adoption) — round 1's two Medium findings the walk had missed, both confirmed and ruled:
the **chain no-value contract** (docs promised a typed gap on *any* no-value condition; as-built only fetch-faults and top-level-malformed bodies recorded one) took the split ruling: a genuinely un-optioned name (empty chain / 404) is ruled a **quiet market fact, no gap** — deliberately kept out of the degraded-input reads, which since the B2 fix feed the engine stand-in conviction — while a non-numeric per-contract strike / volume / open-interest now **errs onto the existing malformed→gap path** (`collect_contracts` returns `Result`; the IV −999 sentinel keeps its tolerant read; test-pinned), and the stale item in both docs' no-value lists is qualified to the designed bound;
the **embedder-change contract** (configuration.md and storage.md promised present-tense re-embedding from retained content; as-built `save_local_models` clears both local namespaces and defers re-embedding) corrected to the as-built stale-cohort clear with the re-embed marked the designed, M5-deferred path — completing the import-half correction the B4 ruling made.
Two Low findings likewise adopted: the B1 regression test extended to the opposite missing leg and the revenue rung, and this record's disposition arithmetic clarified (14 = 4 A + 4 B + 6 C, the extras outside the count).
Round 2 (three residuals, all confirmed and adopted): a **present non-object expiration map** now errs instead of silently reading as a partial or empty successful chain — absence stays the tolerated one-sided case, wrong type is drift (`collect_contracts` splits the two, test-pinned);
the second stale INDEX row (per-job isolated vector memory) corrected to the stale-cohort-clear wording;
and the adapter's comments and one test name swept off the gap-language for the now-quiet un-optioned read (empty chain / 404 = no signal, no gap).

## Verification

At the converged batch (post the two Codex rounds): cargo 944 lib + 32 integration / 0 fail (7 new tests — the dividend-limit pin, the role-risk demotion, the half-spread flat read extended to the opposite leg + the revenue rung, the `engine_view` gap wiring, the guard-conflict carrying overlay, the non-numeric-contract-field chain error, and the present-non-object-map chain error), clippy 0 warnings, `npm run build` clean, 40 node + 218 vitest (1 new spec — the outlook-tag alignment case — plus the tag-count and chip-class assertions updated).
