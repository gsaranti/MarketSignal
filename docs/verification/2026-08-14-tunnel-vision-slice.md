# Tunnel-vision slice — rulings, build, review rounds, and disposition (2026-08-14)

This record owns the tunnel-vision slice's full context: the user rulings, what was built and removed, every review round's findings and dispositions, the standing pushbacks, and the open queue.
The slice is implemented and reviewed in the working tree as of this record; it is **not yet committed**.

## The rulings (all user decisions, 2026-08-14)

1. **The Portfolio Analysis job must not compare holdings.**
   Every per-holding value comes from the loop's engine arm + model arm; each action is decided from that holding alone.
   Whole-book reasoning (cash, sector weighting, concentration, overlap, funding, sizing, deployment) moves to a future fourth job, the **portfolio planner**, which will read this job's report beside the market report and Trade Opportunities.
   The planner is deliberately undesigned here.
2. **Actions are rung-only** — the fixed ladder plus a one-line rationale; target-weight ranges and share/dollar adjustments removed wholesale.
3. **`role_risk_only` holdings get a per-holding action with the full ladder open** (add family included); the engine set for the branch stays the reduced {sell all, trim, hold} as annotate-only evidence.
4. **The investor profile stays in the job** and informs each holding's action (an aggressive tolerance can justify the aggressive rung); it enters at the action call and nowhere else.
5. **Isolation is input-enforced by a split call**: 6f interpretation stays profile-blind authoring the intrinsic verdict; a separate action call follows (verdict + per-holding evidence + profile → rung + rationale).
6. **The 7a whole-book aggregates code is completely removed, not mothballed** (the user expects the planner's needs to differ); the data-health roll-up and closed-positions acknowledgment re-home to the persist step.
7. **The thesis ledger's pre-committed target-weight range is removed**; episode identity drops its weight-range leg.
8. **The `portfolio-weight` ledger series is retired completely** from the closed engine series surface (ruled during Codex round 2's re-raise).
9. **Any future numeric tied to an action must be holding-based, never book-based** — share counts ("trim from X to Y shares"), decided from the holding alone.
   Recorded in `portfolio-analysis.md` §The position thesis ledger; **unbuilt** — a later slice codes against it.

## What was built

- **`portfolio-v9`** (the version constant's changelog carries the contract).
- Step 7 deleted whole: `construction.rs` (4,390 lines), the job's Step-7 block, `persist_degraded_run` + its macro, the `construct` trait method and impls, the joint-feasibility solve, the divergence-cause vocabulary, `ActionWhatChanged`/`ContextCause`, sizing (`size_action`/`rung_band`/`sizing_from_range`), `EngineView.action_sizing`, `GradedVerdict.lean`.
- The **per-holding action call**: `HoldingAnalyst::decide_action` on the existing stage pattern (schema-constrained, thinking, `NUM_CTX_INTERPRET`), prompts stating tunnel vision as the contract, the engine's per-holding set as evidence with its own pick withheld (the ruled 6f precedent), the profile rendered **without its cash row**, the prior action as a continuity baseline **labeled as history when pre-v9** (`whole_book_era_version`), and a one-sentence-rationale instruction.
  An outside-engine-set rung persists as authored with an app-stamped `HoldingAudit.action_annotations` entry.
- `engine::feasible_actions` is per-holding-only (concentration/buying-power terms dropped); `engine_view` takes no position/profile.
- **One-time migration force-include**: a selective run never carries a pre-v9 verdict — the work-list force-includes on a whole-book-era prompt stamp (conservative on missing/unparseable), self-neutralizing after one full pass.
- **`portfolio-weight` retirement mechanics**: off `LedgerSeries::ALL` (schema, prompt vocabulary, parseable claims — fresh drafts downgrade to qualitative); the enum variant survives for decode with a `retired()` predicate; evaluation **skips retired series whole** — deliberately never the unevaluable path, which would type the sweep family `unknown` and force-include the holding on every selective run; a legacy condition dies at its next 6f rewrite.
  All weight plumbing removed: `resolve_series` / `evaluate_ledger_conditions[_gated]` lost the weight param, `analyze_holding` lost `account_total` (**no book input reaches the per-holding loop**), the quick check no longer computes book totals.
- **Legacy compatibility, display-only loss**: the `constructed` marker column, `latest_run` filter, refusal ladder, and sidebar no-book tag survive for retained construction-era degraded rows; `roll_up.aggregates`/`construction` decode as opaque values so the pre-marker shape derivation still discriminates; episodes keep `lean`/`lean_divergence`/weight fields for recorded history (new episodes write `lean = action`, divergence `None`); old runs' retired panels stop rendering.
- Frontend: rung + rationale renders on both branches; construction panel, weight bands, share/dollar figures, lean tags, action-half line removed; `ThesisLedger` TS mirror dropped the weight fields (round 3).
- Docs swept: `portfolio-workflow.md` (6f carries two call blocks; Step 7 = outcome pass + persist, Step 8 = page), `portfolio-analysis.md` (§Portfolio action rewritten around the action call; §Portfolio roll-up replaces §Portfolio roll-up and construction, with the retirement and the allocation-optimizer deferral recorded), `storage.md`, `interface.md`, `configuration.md`, `local-models.md`, `data-sources.md`, `schwab-integration.md`, `run-tracking.md`, `logic-flow-docs/portfolio-analysis-logic-flow.md`, and both design-system READMEs.

## Review rounds

**Internal reviewer: approve-with-nits** — all eleven criteria passed; nits (stale doc comments, one `interface.md` sentence, test indentation) fixed same session.

**Codex round 1 (6 findings)** — fixed: the cash row rendered in the action prompt against the system prompt's promise; the role/risk user prompt still naming construction; the one-sentence rationale instruction; the pre-v9 history label on prior actions; doc/design leftovers (`portfolio-analysis.md` feasible-set inputs, `interface.md`, design README, logic-flow Step 7).
Pushed back: the carry leg of finding 1 (era-preserving carries are the standing vintage semantics — later superseded by round 2's accepted migration gate) and `portfolio-weight` (escalated to the user; ruled in round 2).

**Codex round 2 (4 findings)** — fixed: the ledger REWRITE prompt still requesting a target-weight range (real prompt/schema drift); the migration force-include (accepted once refined to a one-time pre-v9 check); the `Action` rustdoc and other residual comments; logic-flow display block; ui_kits README annotation.
The `portfolio-weight` re-raise produced ruling 8.

**Codex round 3 (3 Medium findings)** — fixed: the TS `ThesisLedger` wire drift (retired weight fields still typed as non-null numbers); the stale whole-book prose in `portfolio-analysis.md` (outcome cohort rationale, net-short section), `schwab-integration.md` (action-sizing spine), and two source comments (`pipeline.rs` net-short, `quick_check.rs` weight-read).
Round 3 confirmed no High findings remain.

## Standing pushbacks (recorded positions, not open work)

- **Rationale enforcement stays prompt-only** (Codex raised three times).
  The Ollama grammar subset cannot express `minLength`, and no prose field anywhere in the suite is value-validated (`self_assessment`, `what_changed`, `financial_summary` ride the same posture); an app-side reject would introduce a new failure path for display prose.
- **`Portfolio.jsx` is not hand-edited.**
  The kit is a generated Claude Design export under the graft-not-swap discipline; the deviation is recorded in both design READMEs, and the construction panel comes out in the next Claude Design iteration.
- **`.metis/BUILD.md` and `INDEX.md` stay construction-era until the user-run alignment** (the standing `.metis`-writes rule); the edit list is in §Disposition.

## Verification state (end of session)

`cargo test` 1,034 passed / 0 failed (28 ignored live smokes); `cargo clippy --all-targets --all-features` warning-free; `npm run build` clean; `npm test` 46 + 238 passed.
Codex independently re-ran all gates each round.

## Disposition

The queue, in order; nothing else stands in front of it.

1. **Commit and push the slice** — the whole body of work is uncommitted in the working tree.
   A further Codex round is optional; round 3 left only the held-pushback re-raise and items now fixed.
2. **User-run BUILD/INDEX alignment.**
   BUILD: record the tunnel-vision contract and the planner as the fourth job; retire the construction bullets from §Local analysis suite, §Seams (`construction::merge_validated_actions`), §Built, and §What each built slice left (the `lean`/`lean_divergence` reservation); reword the `hard_forensic_bar` consumer seam (now the action call + engine set) and retire the cash-residual-drawdown item (its solve no longer exists); note sizing/optimizer as planner-domain.
   INDEX: update or retire the rows citing §Portfolio roll-up and construction, §Step 7/7a/7b, the action-sizing spine, the what-changed action half, and the degraded-run persistence row (now legacy); add this record to §Verification records.
3. **Revise `docs/verification/big-run-watch-set.md` before attempt 3** — several watches target deleted machinery (divergence annotations, constructed-book expectations, construction context-fit); attempt 3 exercises the v9 shape, and its first run is structurally full (the migration gate enforces it).
4. **Share-based action sizing** (ruling 9) — a candidate slice when wanted; nothing blocks on it.
5. **`logic-flow-docs/portfolio-analysis-logic-flow.md` needs its own reconciliation pass** — pre-existing v7-era drift (the retired conviction-raise machinery is still described); this slice fixed only the construction-specific content.
6. Carried from before this slice, unchanged: prod residue cleanup (3 local-model settings + failed `job_runs` id 11, prod-only session); digest compression and `NUM_PREDICT_*` calibration behind a produced-book run.
